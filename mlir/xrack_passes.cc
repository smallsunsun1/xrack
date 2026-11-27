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
#include "llvm/ADT/StringExtras.h"
#include "llvm/ADT/StringMap.h"
#include "llvm/ADT/StringSwitch.h"
#include "llvm/Support/JSON.h"
#include "llvm/Support/raw_ostream.h"
#include <algorithm>
#include <cmath>
#include <limits>
#include <numbers>
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
#define GEN_PASS_DEF_XRSPECIALIZEROTATIONTRACKS
#define GEN_PASS_DEF_XRSIMPLIFYEXPRESSIONS
#define GEN_PASS_DEF_XRFUSESTRINGCONCATS
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

using GeometryNumbers = llvm::SmallVector<double, 6>;
constexpr double identity2D[] = {1.0, 0.0, 0.0, 1.0, 0.0, 0.0};
constexpr double identityQuaternion[] = {0.0, 0.0, 0.0, 1.0};

std::optional<GeometryNumbers> geometryNumbers(Value value) {
  while (auto local = value.getDefiningOp<LocalOp>())
    value = local.getInput();
  auto literal = value.getDefiningOp<LiteralOp>();
  if (!literal)
    return std::nullopt;
  if (isa<LengthType, FloatType>(value.getType())) {
    if (auto number = parseFloatLiteral(literal.getValue()))
      return GeometryNumbers{*number};
    return std::nullopt;
  }
  return parseGeometryLiteral(value.getType(), literal.getValue());
}

bool isIdentity2D(llvm::ArrayRef<double> values) {
  return values == llvm::ArrayRef<double>(identity2D);
}

// Evaluate the same separate multiply/add steps as the Rust matrix operations.
double geometryDot(double a, double b, double c, double d,
                   std::optional<double> bias = std::nullopt) {
  constexpr auto rounding = llvm::APFloat::rmNearestTiesToEven;
  llvm::APFloat first(a), second(c);
  first.multiply(llvm::APFloat(b), rounding);
  second.multiply(llvm::APFloat(d), rounding);
  first.add(second, rounding);
  if (bias)
    first.add(llvm::APFloat(*bias), rounding);
  return first.convertToDouble();
}

GeometryNumbers compose2D(const GeometryNumbers &a, const GeometryNumbers &b) {
  if (isIdentity2D(a))
    return b;
  if (isIdentity2D(b))
    return a;
  return {geometryDot(b[0], a[0], b[2], a[1]),
          geometryDot(b[1], a[0], b[3], a[1]),
          geometryDot(b[0], a[2], b[2], a[3]),
          geometryDot(b[1], a[2], b[3], a[3]),
          geometryDot(b[0], a[4], b[2], a[5], b[4]),
          geometryDot(b[1], a[4], b[3], a[5], b[5])};
}

LogicalResult replaceGeometryLiteral(PatternRewriter &rewriter, Operation *op,
                                     llvm::ArrayRef<double> values) {
  llvm::SmallString<128> spelling;
  for (double value : values) {
    if (!std::isfinite(value))
      return failure();
    llvm::SmallString<32> coordinate;
    llvm::APFloat(value).toString(coordinate);
    if (!spelling.empty())
      spelling += ' ';
    spelling += coordinate;
  }
  replaceLiteral(rewriter, op, spelling);
  return success();
}

// Dropping an unevaluated scene expression could drop a callback or a trap.
static bool staticGeometryArgument(Value value) {
  if (value.getDefiningOp<LiteralOp>())
    return true;
  auto *op = value.getDefiningOp();
  return op && isa<RecordArrayOp, MakeRecordOp, IntArrayOp>(op) &&
         llvm::all_of(op->getOperands(), staticGeometryArgument);
}

struct FoldGeometryFunction : OpRewritePattern<GeometryFunctionOp> {
  using OpRewritePattern::OpRewritePattern;

  LogicalResult matchAndRewrite(GeometryFunctionOp op, PatternRewriter &rewriter) const override {
    auto operation = op.getValue();
    auto inputs = op.getInputs();
    const bool volume = operation == "volume-rgba8";
    if ((operation == "texture-rgba8" || volume) && inputs.size() == (volume ? 4 : 3)) {
      auto pixels = inputs.back().getDefiningOp<IntArrayOp>();
      auto integer = [](Value value) -> std::optional<int64_t> {
        auto literal = value.getDefiningOp<LiteralOp>();
        int64_t number;
        if (!literal || literal.getValue().getAsInteger(10, number))
          return std::nullopt;
        return number;
      };
      if (!pixels || !inputs.back().hasOneUse() || !staticGeometryArgument(inputs.back()))
        return failure();
      const auto limit = volume ? MaxVolumeDataDimension : MaxTextureDataDimension;
      bool valid = true;
      size_t byteCount = 4;
      std::string packed;
      for (Value input : inputs.drop_back()) {
        const auto side = integer(input);
        if (!side)
          return failure();
        if (*side < 1 || *side > limit)
          valid = false;
        else
          byteCount *= *side;
        if (!packed.empty())
          packed += 'x';
        packed += std::to_string(*side);
      }
      if (!valid || byteCount > MaxProceduralTextureBytes || byteCount != pixels.getItems().size()) {
        replaceLiteral(rewriter, op, "");
        return success();
      }
      packed += ':';
      packed.reserve(packed.size() + pixels.getItems().size() * 2);
      for (Value byte : pixels.getItems()) {
        const auto value = integer(byte);
        if (!value || *value < 0 || *value > 255) {
          replaceLiteral(rewriter, op, "");
          return success();
        }
        packed.push_back(llvm::hexdigit(*value >> 4));
        packed.push_back(llvm::hexdigit(*value & 15));
      }
      replaceLiteral(rewriter, op, packed);
      return success();
    }
    if (operation == "identity")
      return replaceGeometryLiteral(rewriter, op, identity2D);
    if (operation == "quaternion-identity")
      return replaceGeometryLiteral(rewriter, op, identityQuaternion);
    if (operation == "combine-3d" && inputs.size() == 2) {
      for (unsigned slot : {0u, 1u}) {
        auto empty = inputs[slot].getDefiningOp<LiteralOp>();
        if (empty && empty.getValue().empty()) {
          rewriter.replaceOp(op, inputs[1 - slot]);
          return success();
        }
      }
    }
    if (inputs.empty())
      return failure();
    if (operation == "quaternion-look-rotation" || operation == "quaternion-look-at") {
      bool endpoints = operation == "quaternion-look-at";
      auto first = geometryNumbers(inputs.front());
      auto target = endpoints ? geometryNumbers(inputs[1]) : std::nullopt;
      if (!first || (endpoints && !target))
        return failure();
      auto upIndex = endpoints ? 2u : 1u;
      bool worldUp = true;
      if (inputs.size() > upIndex) {
        auto up = geometryNumbers(inputs[upIndex]);
        if (!up)
          return failure();
        worldUp = (*up)[0] == 0.0 && (*up)[1] >= 0.0 && (*up)[2] == 0.0;
      }
      bool zeroDirection = endpoints
          ? *first == *target
          : llvm::all_of(*first, [](double lane) { return lane == 0.0; });
      bool negativeZ = endpoints
          ? (*first)[0] == (*target)[0] && (*first)[1] == (*target)[1] && (*target)[2] < (*first)[2]
          : (*first)[0] == 0.0 && (*first)[1] == 0.0 && (*first)[2] < 0.0;
      if (zeroDirection || (worldUp && negativeZ))
        return replaceGeometryLiteral(rewriter, op, identityQuaternion);
      return failure();
    }
    if (operation == "quaternion-track") {
      auto keys = inputs.front().getDefiningOp<RecordArrayOp>();
      if (!keys || !keys.getItems().empty() || !inputs.front().hasOneUse())
        return failure();
      replaceLiteral(rewriter, op, "");
      return success();
    }
    if (operation == "quaternion-track-keys") {
      RotationTrackKeys packed;
      packed.reserve(inputs.size() / 2 * 5);
      for (Value input : inputs) {
        auto numbers = geometryNumbers(input);
        if (!numbers)
          return failure();
        llvm::append_range(packed, *numbers);
      }
      return replaceGeometryLiteral(rewriter, op, packed);
    }
    if (operation == "quaternion-sample") {
      auto track = inputs.front().getDefiningOp<LiteralOp>();
      auto time = geometryNumbers(inputs[1]);
      if (!track || !time)
        return failure();
      auto keys = parseRotationTrackLiteral(track.getValue());
      if (!keys)
        return failure();
      if (keys->empty())
        return replaceGeometryLiteral(rewriter, op, identityQuaternion);
      llvm::ArrayRef<double> packed(*keys);
      if (time->front() <= packed.front() || packed.size() == 5)
        return replaceGeometryLiteral(rewriter, op, packed.slice(1, 4));
      if (time->front() >= packed[packed.size() - 5])
        return replaceGeometryLiteral(rewriter, op, packed.take_back(4));
      for (size_t key = 5; key < packed.size(); key += 5)
        if (time->front() == packed[key])
          return replaceGeometryLiteral(rewriter, op, packed.slice(key + 1, 4));
      return failure();
    }
    if (operation == "quaternion-new") {
      GeometryNumbers components;
      for (Value input : inputs) {
        auto value = geometryNumbers(input);
        if (!value || value->size() != 1)
          return failure();
        components.push_back(value->front());
      }
      return replaceGeometryLiteral(rewriter, op, components);
    }
    if (operation == "quaternion-compose" || operation == "quaternion-inverse") {
      for (auto [index, input] : llvm::enumerate(inputs)) {
        auto value = geometryNumbers(input);
        if (!value || llvm::ArrayRef<double>(*value) != llvm::ArrayRef<double>(identityQuaternion))
          continue;
        if (operation == "quaternion-inverse")
          return replaceGeometryLiteral(rewriter, op, identityQuaternion);
        rewriter.replaceOp(op, inputs[1 - index]);
        return success();
      }
      return failure();
    }
    if (operation == "table-instance-3d") {
      if (inputs.size() != 2 || !staticGeometryArgument(inputs[1]))
        return failure();
      if (inputs.front().getDefiningOp<LiteralOp>()) {
        replaceLiteral(rewriter, op, "");
        return success();
      }
      auto prepared = inputs.front().getDefiningOp<GeometryFunctionOp>();
      if (!prepared || prepared.getValue() != "prepare-instance-table-3d" ||
          prepared.getInputs().size() != 1)
        return failure();
      auto model = prepared.getInputs().front();
      auto rows = model.getDefiningOp<RecordArrayOp>();
      auto literal = inputs[1].getDefiningOp<LiteralOp>();
      int64_t index;
      if (!rows || !model.hasOneUse() || !staticGeometryArgument(model) ||
          !literal || literal.getValue().getAsInteger(10, index) ||
          (index >= 0 && uint64_t(index) < rows.getItems().size()))
        return failure();
      replaceLiteral(rewriter, op, "");
      return success();
    }
    if (operation == "hit-all-3d" || operation == "hit-all-camera-3d" ||
        operation == "ray-hit-all-3d" || operation == "sphere-overlap-3d") {
      auto model = inputs.front().getDefiningOp<RecordArrayOp>();
      bool emptyModel = model && model.getItems().empty() && inputs.front().hasOneUse();
      bool emptyLayers = false;
      const unsigned withMask = operation == "hit-all-3d" ? 7u
                                : operation == "sphere-overlap-3d" ? 4u : 5u;
      if (inputs.size() == withMask)
        if (auto mask = inputs.back().getDefiningOp<LiteralOp>()) {
          int64_t bits;
          emptyLayers = !mask.getValue().getAsInteger(10, bits) && bits == 0;
        }
      if (!emptyModel && !emptyLayers)
        return failure();
      rewriter.replaceOpWithNewOp<Hit3DArrayOp>(
          op, op.getResult().getType(), ValueRange{}, "");
      return success();
    }
    if (operation == "hit-row-3d" || operation == "hit-row-camera-3d" || operation == "ray-hit-row-3d" ||
        operation == "hit-subset-3d" || operation == "hit-subset-camera-3d" || operation == "ray-hit-subset-3d" ||
        operation == "hit-instance-3d" || operation == "hit-instance-camera-3d" || operation == "ray-hit-instance-3d") {
      bool subset = operation.contains("-subset-");
      const unsigned selectorIndex = operation.starts_with("ray-") || operation.ends_with("-camera-3d") ? 4u : 6u;
      const unsigned count = operation.contains("-instance-") ? 3u : 1u;
      if (inputs.size() < selectorIndex + count)
        return failure();
      bool empty = false;
      if (subset) {
        auto rows = inputs[selectorIndex].getDefiningOp<IntArrayOp>();
        empty = rows && rows.getItems().empty();
      } else {
        for (auto [offset, value] : llvm::enumerate(inputs.slice(selectorIndex, count))) {
          auto literal = value.getDefiningOp<LiteralOp>();
          int64_t index;
          int64_t minimum = count == 3 && offset == 1 ? -1 : 0;
          empty |= literal && !literal.getValue().getAsInteger(10, index) &&
                   (index < minimum || index > std::numeric_limits<uint32_t>::max());
        }
      }
      if (inputs.size() == selectorIndex + count + 1)
        if (auto mask = inputs.back().getDefiningOp<LiteralOp>()) {
          int64_t bits;
          empty |= !mask.getValue().getAsInteger(10, bits) && bits == 0;
        }
      if (!empty || !llvm::all_of(inputs, staticGeometryArgument))
        return failure();
      if (subset)
        rewriter.replaceOpWithNewOp<Hit3DArrayOp>(op, op.getResult().getType(), ValueRange{}, "");
      else
        replaceLiteral(rewriter, op, "");
      return success();
    }
    if (operation == "pick-in-rect-3d" || operation == "pick-in-rect-camera-3d") {
      const unsigned endIndex = operation == "pick-in-rect-3d" ? 6u : 4u;
      if (inputs.size() <= endIndex)
        return failure();
      auto model = inputs.front().getDefiningOp<RecordArrayOp>();
      auto start = geometryNumbers(inputs[endIndex - 1]);
      auto end = geometryNumbers(inputs[endIndex]);
      bool empty = (model && model.getItems().empty()) ||
                   (start && end && ((*start)[0] == (*end)[0] ||
                                     (*start)[1] == (*end)[1]));
      if (inputs.size() == endIndex + 2)
        if (auto mask = inputs.back().getDefiningOp<LiteralOp>()) {
          int64_t bits;
          empty |= !mask.getValue().getAsInteger(10, bits) && bits == 0;
        }
      if (!empty || !llvm::all_of(inputs, staticGeometryArgument))
        return failure();
      rewriter.replaceOpWithNewOp<IntArrayOp>(
          op, op.getResult().getType(), ValueRange{}, "");
      return success();
    }
    if (operation == "closest-point-3d" || operation == "closest-point-on-3d") {
      const unsigned radiusIndex = operation == "closest-point-3d" ? 2u : 3u;
      if (inputs.size() <= radiusIndex)
        return failure();
      auto model = inputs.front().getDefiningOp<RecordArrayOp>();
      auto radius = geometryNumbers(inputs[radiusIndex]);
      bool miss = (model && model.getItems().empty()) ||
                  (radius && radius->front() < 0.0);
      if (inputs.size() == radiusIndex + 2)
        if (auto mask = inputs.back().getDefiningOp<LiteralOp>()) {
          int64_t bits;
          miss |= !mask.getValue().getAsInteger(10, bits) && bits == 0;
        }
      if (!miss || !llvm::all_of(inputs, staticGeometryArgument))
        return failure();
      replaceLiteral(rewriter, op, "");
      return success();
    }
    if ((operation == "capsule-trigger-crossings-3d" || operation == "capsule-walk-result-3d" ||
         operation == "capsule-walk-with-contacts-3d") &&
        inputs.size() >= 5 && llvm::all_of(inputs, staticGeometryArgument)) {
      auto motion = geometryNumbers(inputs[2]);
      if (!motion)
        return failure();
      auto hitList = Hit3DListType::get(op.getContext());
      if (operation == "capsule-trigger-crossings-3d") {
        auto from = geometryNumbers(inputs[1]);
        if (from && *from == *motion) {
          rewriter.replaceOpWithNewOp<Hit3DArrayOp>(op, hitList, ValueRange{}, "");
          return success();
        }
      } else if (llvm::all_of(*motion, [](double value) { return value == 0.0; })) {
        // An idle capsule can still be grounded. Fold only when its collision
        // mask or its statically built scene excludes every surface.
        auto model = inputs.front().getDefiningOp<RecordArrayOp>();
        bool noSurfaces = model && model.getItems().empty();
        if (inputs.size() == 9)
          if (auto mask = inputs.back().getDefiningOp<LiteralOp>()) {
            int64_t bits;
            noSurfaces |= !mask.getValue().getAsInteger(10, bits) && bits == 0;
          }
        if (noSurfaces) {
          auto empty = rewriter.create<Hit3DArrayOp>(op.getLoc(), hitList, ValueRange{}, "");
          rewriter.replaceOpWithNewOp<WalkResult3DOp>(
              op, op.getResult().getType(), inputs[1], empty.getResult(), "");
          return success();
        }
      }
    }
    if (operation == "capsule-cast-3d" || operation == "capsule-overlap-3d" ||
        operation == "capsule-slide-3d" || operation == "capsule-walk-3d") {
      unsigned segmentIndex = operation == "capsule-cast-3d" ? 5 :
                              operation == "capsule-overlap-3d" ? 3 : 4;
      if (inputs.size() > segmentIndex) {
        auto segment = geometryNumbers(inputs[segmentIndex]);
        if (segment && segment->size() == 1 && segment->front() == 0.0) {
          SmallVector<Value> sphereInputs(inputs.begin(), inputs.end());
          sphereInputs.erase(sphereInputs.begin() + segmentIndex);
          auto sphere = (llvm::Twine("sphere-") + operation.drop_front(8)).str();
          rewriter.replaceOpWithNewOp<GeometryFunctionOp>(op, op.getResult().getType(),
                                                         sphereInputs, sphere);
          return success();
        }
      }
    }
    if ((operation == "sphere-slide-3d" || operation == "sphere-walk-3d" ||
         operation == "capsule-slide-3d" || operation == "capsule-walk-3d") && inputs.size() >= 4) {
      auto displacement = geometryNumbers(inputs[2]);
      if (displacement && displacement->size() == 3 &&
          llvm::all_of(*displacement, [](double value) { return value == 0.0; })) {
        rewriter.replaceOp(op, inputs[1]);
        return success();
      }
      return failure();
    }
    if (operation == "scene-instance-3d" || operation == "scene-instance-camera-3d" ||
        operation == "scene-instance-orbit-3d") {
      if (inputs.size() < 4 || !llvm::all_of(inputs, staticGeometryArgument))
        return failure();
      auto model = inputs.front().getDefiningOp<RecordArrayOp>();
      bool missing = model && model.getItems().empty() && inputs.front().hasOneUse();
      auto address = inputs.take_back(3);
      for (auto [offset, value] : llvm::enumerate(address)) {
        auto literal = value.getDefiningOp<LiteralOp>();
        int64_t index;
        missing |= literal && !literal.getValue().getAsInteger(10, index) &&
                   (index < (offset == 0 ? 0 : -1) ||
                    index > std::numeric_limits<uint32_t>::max());
      }
      if (!missing)
        return failure();
      replaceLiteral(rewriter, op, "");
      return success();
    }
    if (operation == "map-position-3d" || operation == "map-direction-3d") {
      if (inputs.size() != 5 || !llvm::all_of(inputs, staticGeometryArgument))
        return failure();
      auto from = stringLiteral(inputs[1].getDefiningOp<LiteralOp>());
      auto to = stringLiteral(inputs[2].getDefiningOp<LiteralOp>());
      if (!from || !to)
        return failure();
      if (from->empty() && to->empty()) {
        if (!geometryNumbers(inputs[3]))
          return failure();
        rewriter.replaceOp(op, inputs[3]);
        return success();
      }
      if (auto model = inputs.front().getDefiningOp<RecordArrayOp>();
          model && model.getItems().empty()) {
        rewriter.replaceOp(op, inputs[4]);
        return success();
      }
      return failure();
    }
    if (operation == "point-3d" || operation == "vector-3d") {
      GeometryNumbers values;
      for (auto input : inputs) {
        auto number = geometryNumbers(input);
        if (!number)
          return failure();
        values.push_back(number->front());
      }
      return replaceGeometryLiteral(rewriter, op, values);
    }
    auto a = geometryNumbers(inputs[0]);
    auto b = inputs.size() == 2 ? geometryNumbers(inputs[1]) : std::nullopt;
    if (operation == "compose") {
      if (a && isIdentity2D(*a)) {
        rewriter.replaceOp(op, inputs[1]);
        return success();
      }
      if (b && isIdentity2D(*b)) {
        rewriter.replaceOp(op, inputs[0]);
        return success();
      }
    }
    if (operation == "map-point" && a && isIdentity2D(*a)) {
      rewriter.replaceOp(op, inputs[1]);
      return success();
    }
    if (operation == "map-vector" && a &&
        llvm::ArrayRef<double>(*a).take_front(4) ==
            llvm::ArrayRef<double>(identity2D).take_front(4)) {
      rewriter.replaceOp(op, inputs[1]);
      return success();
    }
    if (!a || (inputs.size() == 2 && !b))
      return failure();

    GeometryNumbers result;
    if (operation == "point" || operation == "vector") {
      result = {(*a)[0], (*b)[0]};
    } else if (operation == "translation") {
      result = {1.0, 0.0, 0.0, 1.0, (*a)[0], (*b)[0]};
    } else if (operation == "scale") {
      result = {(*a)[0], 0.0, 0.0, (*b)[0], 0.0, 0.0};
    } else if (operation == "rotation") {
      double degrees = std::fmod((*a)[0], 360.0);
      if (degrees < 0.0)
        degrees += 360.0;
      double radians = degrees * (std::numbers::pi / 180.0);
      auto snap = [](double value) { return std::abs(value) < 1e-12 ? 0.0 : value; };
      double sin = snap(std::sin(radians)), cos = snap(std::cos(radians));
      result = {cos, sin, -sin, cos, 0.0, 0.0};
    } else if (operation == "compose") {
      result = compose2D(*a, *b);
    } else if (operation == "around") {
      if (isIdentity2D(*a))
        return replaceGeometryLiteral(rewriter, op, identity2D);
      auto [x, y] = std::pair{(*b)[0], (*b)[1]};
      result = compose2D(compose2D({1.0, 0.0, 0.0, 1.0, -x, -y}, *a),
                         {1.0, 0.0, 0.0, 1.0, x, y});
    } else if (operation == "map-point" || operation == "map-vector") {
      auto [x, y] = std::pair{(*b)[0], (*b)[1]};
      auto &m = *a;
      auto bias = [&](size_t axis) -> std::optional<double> {
        return operation == "map-point" ? std::optional(m[axis + 4]) : std::nullopt;
      };
      result = {geometryDot(m[0], x, m[2], y, bias(0)),
                geometryDot(m[1], x, m[3], y, bias(1))};
    } else {
      return failure();
    }
    return replaceGeometryLiteral(rewriter, op, result);
  }
};

template <typename Field>
struct FoldGeometryField : OpRewritePattern<Field> {
  using OpRewritePattern<Field>::OpRewritePattern;

  LogicalResult matchAndRewrite(Field op, PatternRewriter &rewriter) const override {
    if (auto coordinates = geometryNumbers(op.getInput()))
      return replaceFloatLiteral(rewriter, op, llvm::APFloat((*coordinates)[op.getValue() == "z" ? 2 : op.getValue() == "y" ? 1 : 0]));
    return failure();
  }
};

struct FoldRotationTrackField : OpRewritePattern<RotationTrack3DFieldOp> {
  using OpRewritePattern::OpRewritePattern;

  LogicalResult matchAndRewrite(RotationTrack3DFieldOp op,
                               PatternRewriter &rewriter) const override {
    auto literal = op.getInput().getDefiningOp<LiteralOp>();
    if (!literal)
      return failure();
    auto keys = parseRotationTrackLiteral(literal.getValue());
    if (!keys)
      return failure();
    if (op.getValue() == "count")
      replaceLiteral(rewriter, op, std::to_string(keys->size() / 5));
    else if (op.getValue() == "valid")
      replaceLiteral(rewriter, op, keys->empty() ? "false" : "true");
    else
      return replaceFloatLiteral(rewriter, op,
          llvm::APFloat(keys->empty() ? 0.0 : (*keys)[keys->size() - 5]));
    return success();
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

struct FoldWalkResultField : OpRewritePattern<WalkResult3DFieldOp> {
  using OpRewritePattern::OpRewritePattern;

  LogicalResult matchAndRewrite(WalkResult3DFieldOp op,
                                PatternRewriter &rewriter) const override {
    bool position = op.getValue() == "position";
    if (auto result = op.getInput().getDefiningOp<WalkResult3DOp>()) {
      if (op.getValue() == "crossings") {
        if (!staticGeometryArgument(result.getPosition()))
          return failure();
        rewriter.replaceOp(op, result.getCrossings());
        return success();
      }
      auto crossings = result.getCrossings().getDefiningOp<Hit3DArrayOp>();
      if (!crossings || !crossings.getItems().empty())
        return failure();
      if (position)
        rewriter.replaceOp(op, result.getPosition());
      else if (!staticGeometryArgument(result.getPosition()))
        return failure();
      else if (op.getValue() == "contacts")
        rewriter.replaceOp(op, result.getCrossings());
      else if (op.getValue() == "support")
        replaceLiteral(rewriter, op, "");
      else
        replaceLiteral(rewriter, op, "false");
      return success();
    }
    if (!position || !op.getInput().hasOneUse())
      return failure();
    auto local = op.getInput().getDefiningOp<LocalOp>();
    Value input = local ? local.getInput() : op.getInput();
    auto query = input.getDefiningOp<GeometryFunctionOp>();
    if (!query ||
        (query.getValue() != "capsule-walk-result-3d" &&
         query.getValue() != "capsule-walk-with-contacts-3d") ||
        !input.hasOneUse())
      return failure();
    // Keep the original model read placement and any explicit local evaluation.
    OpBuilder::InsertionGuard guard(rewriter);
    auto type = op.getResult().getType();
    rewriter.setInsertionPoint(query);
    Value endpoint = rewriter.create<GeometryFunctionOp>(
        query.getLoc(), type, query.getInputs(), "capsule-walk-3d").getResult();
    if (local) {
      rewriter.setInsertionPoint(local);
      endpoint = rewriter.create<LocalOp>(
          local.getLoc(), type, endpoint, local.getNameAttr()).getResult();
    }
    rewriter.replaceOp(op, endpoint);
    if (local)
      rewriter.eraseOp(local);
    rewriter.eraseOp(query);
    return success();
  }
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
    patterns.add<FoldUnary, FoldBinary, FoldCast, FoldMathFunction, FoldGeometryFunction,
                 FoldRotationTrackField,
                 FoldWalkResultField,
                 FoldGeometryField<Point2DFieldOp>, FoldGeometryField<Vector2DFieldOp>,
                 FoldGeometryField<Point3DFieldOp>, FoldGeometryField<Vector3DFieldOp>,
                 FoldRound, FoldToRatio, FoldStringify>(context);
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

struct SpecializeRotationTracks
    : impl::XrSpecializeRotationTracksBase<SpecializeRotationTracks> {
  void runOnOperation() override {
    auto module = getOperation();
    llvm::SmallVector<GeometryFunctionOp> tracks;
    module.walk([&](GeometryFunctionOp op) {
      if (op.getValue() == "quaternion-track")
        tracks.push_back(op);
    });
    if (tracks.empty())
      return;
    const auto &layouts = getAnalysis<RecordLayouts>();
    IRRewriter rewriter(&getContext());
    bool changed = false;
    for (auto track : tracks) {
      auto value = track.getInputs().front();
      auto array = value.getDefiningOp<RecordArrayOp>();
      if (!array || !value.hasOneUse() || array.getItems().empty())
        continue;
      ConstantRecordValues constants;
      llvm::SmallVector<Value> inputs;
      inputs.reserve(array.getItems().size() * 2);
      double previous = -1.0;
      for (Value row : array.getItems()) {
        auto time = layouts.field(row, "time");
        auto orientation = layouts.field(row, "orientation");
        auto literal = time ? time.getDefiningOp<LiteralOp>() : LiteralOp();
        auto seconds = literal ? parseFloatLiteral(literal.getValue()) : std::nullopt;
        if (!seconds || *seconds < 0.0 || *seconds <= previous || !orientation)
          break;
        // The model and record shells can go away. Keep the rotation reads in
        // row order and only discard auxiliary fields that are safe literals.
        auto record = row.getDefiningOp<MakeRecordOp>();
        if (llvm::any_of(record.getFields(), [&](Value field) {
              return field != time && field != orientation && !constants.contains(field);
            }))
          break;
        previous = *seconds;
        inputs.append({time, orientation});
      }
      if (inputs.size() != array.getItems().size() * 2)
        continue;
      rewriter.setInsertionPoint(track);
      rewriter.replaceOpWithNewOp<GeometryFunctionOp>(
          track, track.getResult().getType(), inputs, "quaternion-track-keys");
      changed = true;
    }
    if (changed && failed(foldConstants(module, getAnalysis<PropertyAnalysis>())))
      signalPassFailure();
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

// Leave explicitly planned invariant operands intact. Consuming them as a
// single operand keeps their original cache and its lazy evaluation scope.
struct FuseStringConcats : impl::XrFuseStringConcatsBase<FuseStringConcats> {
  void runOnOperation() override {
    IRRewriter rewriter(&getContext());
    getOperation()->walk([&](BinaryOp add) {
      if (add.getValue() != "+" || !isa<StringType>(add.getResult().getType()))
        return;
      llvm::SmallVector<Value> parts;
      llvm::SmallVector<Value> pending{add.getRhs(), add.getLhs()};
      while (!pending.empty()) {
        Value value = pending.pop_back_val();
        if (auto *def = value.getDefiningOp();
            def && !def->hasAttr("xr.repeat_subexpression_group")) {
          if (auto binary = dyn_cast<BinaryOp>(def); binary && binary.getValue() == "+" &&
              isa<StringType>(binary.getResult().getType())) {
            pending.push_back(binary.getRhs());
            pending.push_back(binary.getLhs());
            continue;
          }
          if (auto concat = dyn_cast<ConcatOp>(def)) {
            llvm::append_range(pending, llvm::reverse(concat.getParts()));
            continue;
          }
        }
        parts.push_back(value);
      }
      if (parts.size() <= 2)
        return;
      rewriter.setInsertionPoint(add);
      auto concat = rewriter.create<ConcatOp>(add.getLoc(), add.getResult().getType(),
                                              parts, rewriter.getStringAttr(""));
      for (auto attr : add->getDiscardableAttrs())
        concat->setDiscardableAttr(attr.getName(), attr.getValue());
      rewriter.replaceOp(add, concat.getResult());
    });
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
  rewriter.replaceOpWithNewOp<Binding>(state, state.getInput(), state.getNameAttr(), StringAttr{});
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
  passes.addPass(std::make_unique<SpecializeRotationTracks>());
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
  passes.addPass(std::make_unique<FuseStringConcats>());
  passes.addPass(std::make_unique<PruneExpressions>());
}
} // namespace xrack::dialect
