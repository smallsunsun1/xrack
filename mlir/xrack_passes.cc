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
#include "llvm/ADT/DenseSet.h"
#include "llvm/ADT/SetVector.h"
#include "llvm/ADT/SmallString.h"
#include "llvm/ADT/StringMap.h"
#include "llvm/ADT/StringSwitch.h"
#include "llvm/Support/JSON.h"
#include "llvm/Support/raw_ostream.h"
#include <algorithm>
#include <cmath>
#include <limits>
#include <optional>
#include <string>
#include <tuple>
#include <type_traits>

namespace xrack::dialect {
void addVisualCachePasses(mlir::OpPassManager &passes);
void addScenePlanPasses(mlir::OpPassManager &passes);

#define GEN_PASS_DEF_XRCOLLAPSEPRIVATEALIASES
#define GEN_PASS_DEF_XRFOLDCONSTANTS
#define GEN_PASS_DEF_XRFOLDRECORDPROJECTIONS
#define GEN_PASS_DEF_XRSIMPLIFYEXPRESSIONS
#define GEN_PASS_DEF_XRSIMPLIFYVISUALS
#define GEN_PASS_DEF_XRPRUNEEXPRESSIONS
#define GEN_PASS_DEF_XRPRUNECONSTANTSTATES
#define GEN_PASS_DEF_XRSIMPLIFYSTATETRANSITIONS
#define GEN_PASS_DEF_XRMERGESTATECASES
#define GEN_PASS_DEF_XRMATERIALIZESTATICSTATES
#define GEN_PASS_DEF_XRREMOVEUNUSEDPROPERTIES
#define GEN_PASS_DEF_XRSPECIALIZEHIDDENVISUALS
#include "mlir/XrackPasses.h.inc"

using namespace ::mlir;

namespace {
struct CollapsePrivateAliases
    : impl::XrCollapsePrivateAliasesBase<CollapsePrivateAliases> {
  void runOnOperation() override {
    for (auto component : getOperation().getOps<ComponentOp>()) {
      llvm::StringMap<PropertyOp> properties;
      for (auto property : component.getBody().front().getOps<PropertyOp>())
        properties.try_emplace(property.getName(), property);

      llvm::StringMap<StringAttr> aliases;
      for (const auto &entry : properties) {
        auto property = entry.second;
        if (property.getVisibility() != "private" ||
            property.getAlias().value_or("") != "true")
          continue;
        auto target = property.getInitial().getDefiningOp<ReadOp>();
        if (target && target.getOwner().value_or("root") == "root" &&
            properties.contains(target.getValue()))
          aliases.try_emplace(property.getName(), target.getValueAttr());
      }
      if (aliases.empty())
        continue;

      llvm::StringMap<StringAttr> resolved;
      for (const auto &entry : aliases) {
        llvm::SmallVector<llvm::StringRef, 4> path;
        llvm::DenseSet<llvm::StringRef> visiting;
        auto current = entry.getKey();
        StringAttr terminal;
        while (true) {
          if (auto known = resolved.find(current); known != resolved.end()) {
            terminal = known->second;
            break;
          }
          auto alias = aliases.find(current);
          if (alias == aliases.end()) {
            if (auto property = properties.lookup(current);
                property && property.getAlias().value_or("") != "true")
              terminal = StringAttr::get(&getContext(), current);
            break;
          }
          if (!visiting.insert(current).second)
            break;
          path.push_back(current);
          current = alias->second.getValue();
        }
        for (auto name : path)
          resolved.try_emplace(name, terminal);
      }

      // Public aliases and links retain their declaration; only the local
      // property operand changes to name the same backing storage directly.
      component.walk([&](ReadOp read) {
        if (read.getOwner().value_or("root") == "root")
          if (auto terminal = resolved.lookup(read.getValue()))
            read.setValueAttr(terminal);
      });
      component.walk([&](AssignOp assign) {
        if (assign.getOwner().value_or("root") == "root")
          if (auto terminal = resolved.lookup(assign.getName()))
            assign.setNameAttr(terminal);
      });
      for (const auto &entry : resolved)
        if (entry.second)
          properties.lookup(entry.getKey()).erase();
    }
  }
};

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
      auto number = parseFloatLiteral(literal.getValue());
      if (!number)
        return failure();
      llvm::APFloat value(*number);
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
    if (op.getValue() != "-")
      return failure();
    auto negated = -llvm::APInt(64, static_cast<uint64_t>(value));
    replaceLiteral(rewriter, op, std::to_string(negated.getSExtValue()));
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
  if (spelling == "+")
    value = lhs + rhs;
  else if (spelling == "-")
    value = lhs - rhs;
  else if (spelling == "*")
    value = lhs * rhs;
  else if ((spelling == "/" || spelling == "%") && right != 0 &&
           !(left == INT64_MIN && right == -1))
    value = spelling == "/" ? lhs.sdiv(rhs) : lhs.srem(rhs);
  else
    return failure();
  replaceLiteral(rewriter, op, std::to_string(value.getSExtValue()));
  return success();
}

LogicalResult foldFloat(BinaryOp op, PatternRewriter &rewriter, LiteralOp lhs,
                        LiteralOp rhs) {
  auto left = parseFloatLiteral(lhs.getValue());
  auto right = parseFloatLiteral(rhs.getValue());
  if (!left || !right)
    return failure();
  llvm::APFloat value(*left), operand(*right);
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

struct FoldMathFunction : OpRewritePattern<MathFunctionOp> {
  using OpRewritePattern::OpRewritePattern;

  LogicalResult matchAndRewrite(MathFunctionOp op,
                                PatternRewriter &rewriter) const override {
    Type input = op.getInputs().front().getType();
    auto operation = op.getValue();
    if (operation != "min" && operation != "max" && operation != "clamp" &&
        operation != "abs")
      return failure();
    if (isa<IntType>(input)) {
      llvm::SmallVector<int64_t, 3> values;
      for (Value operand : op.getInputs()) {
        auto literal = operand.getDefiningOp<LiteralOp>();
        int64_t value;
        if (!literal || literal.getValue().getAsInteger(10, value))
          return failure();
        values.push_back(value);
      }
      int64_t a = values[0];
      int64_t result;
      if (operation == "abs") {
        result = a == std::numeric_limits<int64_t>::min()
                     ? std::numeric_limits<int64_t>::max()
                     : (a < 0 ? -a : a);
      } else if (operation == "clamp") {
        auto [min, max] = std::minmax(values[1], values[2]);
        result = std::clamp(a, min, max);
      } else {
        result = operation == "min" ? std::min(a, values[1])
                                    : std::max(a, values[1]);
      }
      replaceLiteral(rewriter, op, std::to_string(result));
      return success();
    }

    // Percent literals are stored in display units, while runtime percent
    // operands hold ratios. Keep these operations at runtime to avoid changing
    // precision by introducing an extra conversion.
    if (isa<PercentType>(input))
      return failure();
    llvm::SmallVector<double, 3> values;
    for (Value operand : op.getInputs()) {
      auto literal = operand.getDefiningOp<LiteralOp>();
      auto value = literal ? parseFloatLiteral(literal.getValue()) : std::nullopt;
      if (!value)
        return failure();
      values.push_back(*value);
    }
    double a = values[0];
    double result;
    // Match the runtime helpers when numerically equal zeros have different signs.
    auto minimum = [](double left, double right) {
      if (left == 0.0 && right == 0.0)
        return std::signbit(left) ? left : right;
      return std::min(left, right);
    };
    auto maximum = [](double left, double right) {
      if (left == 0.0 && right == 0.0)
        return std::signbit(left) ? right : left;
      return std::max(left, right);
    };
    if (operation == "abs") {
      result = std::abs(a);
    } else if (operation == "clamp") {
      result = std::clamp(a, minimum(values[1], values[2]),
                          maximum(values[1], values[2]));
    } else {
      result = operation == "min" ? minimum(a, values[1]) : maximum(a, values[1]);
    }
    return replaceFloatLiteral(rewriter, op, llvm::APFloat(result));
  }
};

struct FoldRound : OpRewritePattern<RoundOp> {
  using OpRewritePattern::OpRewritePattern;

  LogicalResult matchAndRewrite(RoundOp op,
                                PatternRewriter &rewriter) const override {
    auto literal = op.getInput().getDefiningOp<LiteralOp>();
    auto value = literal ? parseFloatLiteral(literal.getValue()) : std::nullopt;
    if (!value)
      return failure();
    double rounded = std::round(*value);
    constexpr double upper = 0x1p63;
    int64_t result;
    if (rounded >= upper)
      result = std::numeric_limits<int64_t>::max();
    else if (rounded <= -upper)
      result = std::numeric_limits<int64_t>::min();
    else
      result = static_cast<int64_t>(rounded);
    replaceLiteral(rewriter, op, std::to_string(result));
    return success();
  }
};

struct FoldToRatio : OpRewritePattern<ToRatioOp> {
  using OpRewritePattern::OpRewritePattern;

  LogicalResult matchAndRewrite(ToRatioOp op,
                                PatternRewriter &rewriter) const override {
    auto literal = op.getInput().getDefiningOp<LiteralOp>();
    auto number = literal ? parseFloatLiteral(literal.getValue()) : std::nullopt;
    if (!number)
      return failure();
    llvm::APFloat value(*number);
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

template <typename Field, typename Make>
struct FoldBuiltinField : OpRewritePattern<Field> {
  FoldBuiltinField(MLIRContext *context, const PropertyAnalysis &properties)
      : OpRewritePattern<Field>(context), properties(properties) {}

  LogicalResult matchAndRewrite(Field field,
                                PatternRewriter &rewriter) const override {
    Value input = field.getInput();
    if (auto read = input.getDefiningOp<ReadOp>()) {
      auto property = properties.resolve(read);
      if (!properties.isImmutablePrivate(property))
        return failure();
      input = property.getInitial();
    }
    auto make = input.getDefiningOp<Make>();
    if (!make || !llvm::all_of(make->getOperands(), [](Value operand) {
          return static_cast<bool>(operand.getDefiningOp<LiteralOp>());
        }))
      return failure();

    auto name = field.getValue();
    auto index = [&]() -> std::optional<unsigned> {
      if constexpr (std::is_same_v<Field, DateFieldOp>)
        return llvm::StringSwitch<std::optional<unsigned>>(name)
            .Case("year", 0).Case("month", 1).Case("day", 2).Default(std::nullopt);
      if constexpr (std::is_same_v<Field, TimeFieldOp>)
        return llvm::StringSwitch<std::optional<unsigned>>(name)
            .Case("hour", 0).Case("minute", 1).Case("second", 2).Default(std::nullopt);
      return llvm::StringSwitch<std::optional<unsigned>>(name)
          .Case("family", 0).Case("size", 1).Case("weight", 2).Case("italic", 3)
          .Default(std::nullopt);
    }();
    if (!index || *index >= make->getNumOperands())
      return failure();
    auto literal = make->getOperand(*index).template getDefiningOp<LiteralOp>();
    if (literal.getResult().getType() != field.getResult().getType())
      return failure();
    if constexpr (std::is_same_v<Field, TimeFieldOp>) {
      // Match Time::normalized() used by both execution backends.
      int64_t value;
      if (literal.getValue().getAsInteger(10, value))
        return failure();
      replaceLiteral(rewriter, field, std::to_string(
          std::clamp<int64_t>(value, 0, *index == 0 ? 23 : 59)));
    } else {
      replaceLiteral(rewriter, field, literal.getValue());
    }
    return success();
  }

private:
  const PropertyAnalysis &properties;
};

LogicalResult rewriteExpressions(ModuleOp module, RewritePatternSet patterns) {
  GreedyRewriteConfig config;
  config.setRegionSimplificationLevel(GreedySimplifyRegionLevel::Disabled);
  return applyPatternsGreedily(module, std::move(patterns), config);
}

LogicalResult foldConstants(ModuleOp module, const PropertyAnalysis &properties) {
  auto *context = module.getContext();
  for (;;) {
    RewritePatternSet patterns(context);
    patterns.add<FoldUnary, FoldBinary, FoldCast, FoldMathFunction, FoldRound, FoldToRatio, FoldStringify>(context);
    patterns.add<FoldBuiltinField<DateFieldOp, MakeDateOp>,
                 FoldBuiltinField<TimeFieldOp, MakeTimeOp>,
                 FoldBuiltinField<FontFieldOp, MakeFontOp>>(context, properties);
    if (failed(rewriteExpressions(module, std::move(patterns))))
      return failure();

    // Named reads are not SSA uses of their property initializer. Revisit
    // them after folding to propagate through chains of properties.
    llvm::SmallVector<std::pair<ReadOp, StringAttr>> constants;
    module.walk([&](ReadOp read) {
      auto property = properties.resolve(read);
      if (properties.isImmutablePrivateScalar(property))
        if (auto literal = property.getInitial().getDefiningOp<LiteralOp>())
          constants.emplace_back(read, literal.getValueAttr());
    });
    if (constants.empty())
      return success();
    IRRewriter rewriter(context);
    for (auto [read, spelling] : constants) {
      rewriter.setInsertionPoint(read);
      rewriter.replaceOpWithNewOp<LiteralOp>(read, read.getResult().getType(),
                                             spelling);
    }
  }
}

struct FoldConstants : impl::XrFoldConstantsBase<FoldConstants> {
  void runOnOperation() override {
    if (failed(foldConstants(getOperation(), getAnalysis<PropertyAnalysis>())))
      signalPassFailure();
  }
};

// A record constructor eagerly evaluates every field. Literal leaves and
// nested literal records are safe to discard, including the unselected fields.
class ConstantRecordValues {
public:
  bool contains(Value value) {
    if (auto found = values.find(value); found != values.end())
      return found->second;
    auto *definition = value.getDefiningOp();
    bool constant = isa_and_nonnull<LiteralOp>(definition) ||
                    (isa_and_nonnull<MakeRecordOp, MakeDateOp, MakeTimeOp, MakeFontOp>(definition) &&
                     llvm::all_of(definition->getOperands(), [&](Value field) {
                       return contains(field);
                     }));
    return values.try_emplace(value, constant).first->second;
  }

private:
  llvm::DenseMap<Value, bool> values;
};

class RecordLayouts {
public:
  explicit RecordLayouts(ModuleOp module) {
    for (auto declaration : module.getOps<RecordDeclOp>()) {
      auto parsed = llvm::json::parse(declaration.getFields());
      if (!parsed) {
        llvm::consumeError(parsed.takeError());
        continue;
      }
      const auto *fields = parsed->getAsArray();
      if (!fields)
        continue;
      auto &indices = layouts[declaration.getSymName()];
      for (auto [index, field] : llvm::enumerate(*fields))
        if (auto *entry = field.getAsObject())
          if (auto name = entry->getString("name"))
            indices.try_emplace(*name, index);
    }
  }

  Value field(Value record, llvm::StringRef fieldName) const {
    auto make = record.getDefiningOp<MakeRecordOp>();
    if (!make)
      return {};
    auto layout = layouts.find(cast<RecordType>(record.getType()).getName());
    if (layout == layouts.end())
      return {};
    auto field = layout->second.find(fieldName);
    if (field == layout->second.end() || field->second >= make.getFields().size())
      return {};
    return make.getFields()[field->second];
  }

private:
  llvm::StringMap<llvm::StringMap<size_t>> layouts;
};

LiteralOp projectedLiteral(GetFieldOp field, const RecordLayouts &layouts,
                           const PropertyAnalysis &properties,
                           ConstantRecordValues &constants) {
  llvm::SmallVector<llvm::StringRef, 4> path;
  Value record;
  do {
    path.push_back(field.getValue());
    record = field.getInput();
    field = record.getDefiningOp<GetFieldOp>();
  } while (field);

  if (auto read = record.getDefiningOp<ReadOp>()) {
    auto property = properties.resolve(read);
    if (!properties.isImmutablePrivate(property))
      return {};
    record = property.getInitial();
  }
  if (!constants.contains(record))
    return {};
  for (auto name : llvm::reverse(path)) {
    record = layouts.field(record, name);
    if (!record)
      return {};
  }
  return record.getDefiningOp<LiteralOp>();
}

struct FoldRecordProjections
    : impl::XrFoldRecordProjectionsBase<FoldRecordProjections> {
  void runOnOperation() override {
    auto module = getOperation();
    const PropertyAnalysis *properties = nullptr;
    std::optional<RecordLayouts> layouts;
    for (;;) {
      ConstantRecordValues constants;
      llvm::SmallVector<std::pair<GetFieldOp, StringAttr>> projections;
      module.walk([&](GetFieldOp field) {
        if (!layouts) {
          layouts.emplace(module);
          properties = &getAnalysis<PropertyAnalysis>();
        }
        if (auto literal = projectedLiteral(field, *layouts, *properties, constants);
            literal && literal.getResult().getType() == field.getResult().getType())
          projections.emplace_back(field, literal.getValueAttr());
      });
      if (projections.empty())
        return;
      IRRewriter rewriter(&getContext());
      for (auto [field, spelling] : projections) {
        rewriter.setInsertionPoint(field);
        rewriter.replaceOpWithNewOp<LiteralOp>(field, field.getResult().getType(),
                                               spelling);
      }
      if (failed(foldConstants(module, *properties))) {
        signalPassFailure();
        return;
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
    if (kept) {
      rewriter.replaceOp(op, kept);
      return success();
    }
    auto lhs = op.getLhs(), rhs = op.getRhs();
    if (!isa<BoolType>(lhs.getType()) || lhs.getType() != rhs.getType())
      return failure();
    bool equal = op.getValue() == "==";
    if (!equal && op.getValue() != "!=")
      return failure();
    // x == false and x != true (in either operand order) evaluate x once.
    if (isBoolLiteral(lhs, !equal))
      kept = rhs;
    else if (isBoolLiteral(rhs, !equal))
      kept = lhs;
    else
      return failure();
    rewriter.replaceOpWithNewOp<UnaryOp>(op, kept.getType(), kept, "!");
    return success();
  }
};

struct SimplifyBooleanSelect : OpRewritePattern<SelectOp> {
  using OpRewritePattern::OpRewritePattern;

  LogicalResult matchAndRewrite(SelectOp op,
                                PatternRewriter &rewriter) const override {
    auto type = op.getResult().getType();
    if (!isa<BoolType>(type))
      return failure();
    auto whenTrue = op.getOnTrue(), whenFalse = op.getOnFalse();
    bool trueArmLiteral = !!whenTrue.getDefiningOp<LiteralOp>();
    bool falseArmLiteral = !!whenFalse.getDefiningOp<LiteralOp>();
    if (!trueArmLiteral && !falseArmLiteral)
      return failure();
    Value condition = op.getCondition();
    if (trueArmLiteral && falseArmLiteral) {
      bool onTrue = isBoolLiteral(whenTrue, true);
      if (isBoolLiteral(whenFalse, onTrue))
        return failure(); // The condition must still be evaluated.
      if (onTrue)
        rewriter.replaceOp(op, condition);
      else
        rewriter.replaceOpWithNewOp<UnaryOp>(op, type, condition, "!");
      return success();
    }

    // Keep the original condition first and evaluate the other arm only
    // where the original select would have taken it.
    bool skipValue = isBoolLiteral(trueArmLiteral ? whenTrue : whenFalse, true);
    if (trueArmLiteral != skipValue)
      condition = UnaryOp::create(rewriter, op.getLoc(), type, condition, "!")
                      .getResult();
    rewriter.replaceOpWithNewOp<BinaryOp>(
        op, type, condition, trueArmLiteral ? whenFalse : whenTrue,
        skipValue ? "||" : "&&");
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
    patterns.add<SimplifyUnary, SimplifyBinary, SimplifyBooleanSelect>(&getContext());
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

struct SimplifyStateTransitions
    : impl::XrSimplifyStateTransitionsBase<SimplifyStateTransitions> {
  void runOnOperation() override {
    getOperation()->walk([&](StateTimingOp timing) {
      auto enabled = timing.getEnabled();
      // Both backends evaluate enabled before duration. A disabled timing
      // contributes no entry or exit animation to the selected state.
      if (enabled && isBoolLiteral(enabled, false))
        timing.erase();
      else if (enabled && isBoolLiteral(enabled, true))
        timing.getEnabledMutable().clear();
    });
  }
};

Value joinStateConditions(ValueRange conditions, IRRewriter &rewriter,
                          Location loc) {
  if (conditions.size() == 1)
    return conditions.front();
  // Balance large groups while retaining left-to-right short-circuit order.
  auto midpoint = conditions.size() / 2;
  auto left = joinStateConditions(conditions.take_front(midpoint), rewriter, loc);
  auto right = joinStateConditions(conditions.drop_front(midpoint), rewriter, loc);
  return BinaryOp::create(rewriter, loc, left.getType(), left, right, "||")
      .getResult();
}

struct MergeStateCases : impl::XrMergeStateCasesBase<MergeStateCases> {
  void runOnOperation() override {
    IRRewriter rewriter(&getContext());
    getOperation()->walk([&](StateSelectOp selector) {
      using Target = std::tuple<Operation *, OperationName, StringAttr>;
      struct Case {
        llvm::DenseMap<Target, Value> overrides;
        llvm::SmallVector<Operation *, 2> users;
      };
      auto conditions = selector.getConditions();
      if (conditions.size() < 2)
        return;
      llvm::SmallVector<Case> cases(conditions.size());
      for (auto &use : selector.getResult().getUses()) {
        auto *user = use.getOwner();
        // Transition tracking and ordinary SSA consumers observe the case
        // index itself. A group containing either must keep its cases apart.
        if (use.getOperandNumber() != 0 || !isa<StateSetOp, InstanceStateOp>(user))
          return;
        auto index = user->getAttrOfType<IntegerAttr>("case_index").getInt();
        auto name = user->getAttrOfType<StringAttr>("name");
        auto &entry = cases[index];
        entry.overrides.try_emplace(Target(user->getParentOp(), user->getName(), name),
                                    user->getOperand(1));
        entry.users.push_back(user);
      }

      llvm::SmallVector<Value> mergedConditions;
      llvm::SmallVector<Attribute> mergedNames;
      rewriter.setInsertionPoint(selector);
      for (size_t start = 0; start < cases.size();) {
        size_t end = start + 1;
        while (end < cases.size() && cases[end].overrides == cases[start].overrides)
          ++end;
        if (mergedConditions.size() != start) {
          auto index = rewriter.getI64IntegerAttr(mergedConditions.size());
          for (auto *user : cases[start].users)
            user->setAttr("case_index", index);
        }
        for (size_t duplicate = start + 1; duplicate < end; ++duplicate)
          for (auto *user : cases[duplicate].users)
            rewriter.eraseOp(user);
        mergedConditions.push_back(joinStateConditions(
            conditions.slice(start, end - start), rewriter, selector.getLoc()));
        mergedNames.push_back(selector.getNames()[start]);
        start = end;
      }
      if (mergedConditions.size() != cases.size()) {
        selector.getConditionsMutable().assign(mergedConditions);
        selector.setNamesAttr(rewriter.getArrayAttr(mergedNames));
      }
    });
  }
};

bool canMaterializeState(StateSetOp state) {
  if (!state.getInput().getDefiningOp<LiteralOp>())
    return false;
  auto *owner = state->getParentOp();
  // Keep properties whose storage or effective value can also change via
  // a control, component input or layout feedback on their normal path.
  auto name = state.getName();
  if (llvm::StringSwitch<bool>(name)
          .Cases({"x", "y", "width", "height", "min-width", "max-width",
                  "min-height", "max-height", "preferred-width",
                  "preferred-height", "horizontal-stretch", "vertical-stretch",
                  "row", "col", "rowspan", "colspan", "z", "opacity"}, true)
          .Default(false))
    return isa<ElementOp, InstanceOp>(owner);

  auto element = dyn_cast<ElementOp>(owner);
  if (!element)
    return false;
  auto kind = element.getKind();
  return (kind == "Rectangle" && name == "background") ||
         ((kind == "Text" || kind == "Label") && (name == "text" || name == "color"));
}

template <typename Binding>
void materializeState(IRRewriter &rewriter, StateSetOp state) {
  auto &body = state->getParentOp()->getRegion(0).front();
  for (auto binding : llvm::make_early_inc_range(body.getOps<Binding>()))
    if (binding.getName() == state.getName())
      rewriter.eraseOp(binding);
  rewriter.setInsertionPoint(state);
  rewriter.replaceOpWithNewOp<Binding>(state, state.getInput(), state.getNameAttr());
}

struct MaterializeStaticStates
    : impl::XrMaterializeStaticStatesBase<MaterializeStaticStates> {
  void runOnOperation() override {
    IRRewriter rewriter(&getContext());
    llvm::SmallVector<StateSelectOp> selectors;
    getOperation()->walk(
        [&](StateSelectOp selector) { selectors.push_back(selector); });
    for (auto selector : selectors) {
      auto conditions = selector.getConditions();
      if (conditions.size() != 1 || !isBoolLiteral(conditions.front(), true))
        continue;
      llvm::SmallVector<StateSetOp> overrides;
      bool supported = true;
      for (auto &use : selector.getResult().getUses()) {
        auto state = dyn_cast<StateSetOp>(use.getOwner());
        if (use.getOperandNumber() != 0 || !state || !canMaterializeState(state)) {
          supported = false;
          break;
        }
        overrides.push_back(state);
      }
      if (!supported)
        continue;
      for (auto state : overrides) {
        if (isa<InstanceOp>(state->getParentOp()))
          materializeState<LayoutBindOp>(rewriter, state);
        else
          materializeState<BindOp>(rewriter, state);
      }
      rewriter.eraseOp(selector);
    }
  }
};

bool unusedExpression(Operation *op) {
  return op && op->use_empty() && (isExpression(*op) || isa<StateSelectOp>(op));
}

struct RemoveUnusedProperties
    : impl::XrRemoveUnusedPropertiesBase<RemoveUnusedProperties> {
  void runOnOperation() override {
    const auto &properties = getAnalysis<PropertyAnalysis>();
    llvm::DenseMap<Operation *, size_t> readCounts;
    getOperation()->walk([&](ReadOp read) {
      if (auto property = properties.resolve(read))
        ++readCounts[property];
    });

    // Property initializers are lazy in both backends. Once the last read of
    // a private binding is gone, its initializer cannot run. Follow released
    // SSA uses and named property reads together to remove whole chains.
    llvm::SmallSetVector<Operation *, 16> pending;
    auto scheduleProperty = [&](PropertyOp property) {
      if (properties.isImmutablePrivate(property) && !readCounts.lookup(property))
        pending.insert(property);
    };
    getOperation()->walk([&](PropertyOp property) { scheduleProperty(property); });
    while (!pending.empty()) {
      auto *op = pending.pop_back_val();
      if (auto read = dyn_cast<ReadOp>(op)) {
        if (auto property = properties.resolve(read)) {
          --readCounts[property];
          scheduleProperty(property);
        }
      }
      llvm::SmallVector<Value> operands(op->getOperands());
      op->erase();
      for (auto operand : operands)
        if (auto *definition = operand.getDefiningOp(); unusedExpression(definition))
          pending.insert(definition);
    }
  }
};

struct PruneExpressions : impl::XrPruneExpressionsBase<PruneExpressions> {
  void runOnOperation() override {
    // XR scopes have one block. Visit users before definitions, including uses
    // in nested scopes, so dead expression trees disappear in a single walk.
    getOperation()->walk<WalkOrder::PostOrder, ReverseIterator>(
        [](Operation *op) {
          // These expressions are lowered at their use sites. An unused xr.call
          // never executes; standalone calls are represented by xr.invoke.
          if (unusedExpression(op))
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
  passes.addPass(std::make_unique<CollapsePrivateAliases>());
  passes.addPass(std::make_unique<FoldConstants>());
  passes.addPass(std::make_unique<SimplifyExpressions>());
  passes.addPass(std::make_unique<FoldRecordProjections>());
  passes.addPass(std::make_unique<SimplifyVisuals>());
  passes.addPass(createCanonicalizerPass());
  passes.addPass(createCSEPass());
  passes.addPass(std::make_unique<PruneConstantStates>());
  passes.addPass(std::make_unique<SimplifyStateTransitions>());
  passes.addPass(std::make_unique<MergeStateCases>());
  passes.addPass(std::make_unique<MaterializeStaticStates>());
  passes.addPass(std::make_unique<PruneExpressions>());
  passes.addPass(std::make_unique<RemoveUnusedProperties>());
  passes.addPass(std::make_unique<SpecializeHiddenVisuals>());
  addScenePlanPasses(passes);
  addVisualCachePasses(passes);
}
} // namespace xrack::dialect
