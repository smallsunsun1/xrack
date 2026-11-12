#include "mlir/c_api.h"

#include "mlir/CAPI/IR.h"
#include "mlir/CAPI/Pass.h"
#include "mlir/CAPI/Registration.h"
#include "mlir/CAPI/Support.h"
#include "mlir/IR/Builders.h"
#include "mlir/xrack_dialect.h"
#include "mlir/xrack_passes.h"
#include "llvm/ADT/SmallVector.h"
#include "llvm/ADT/StringMap.h"
#include <type_traits>

MLIR_DEFINE_CAPI_DIALECT_REGISTRATION(Xrack, xr, xrack::dialect::XrackDialect)

namespace {
template <typename Type>
mlir::Type buildType(mlir::MLIRContext *context, llvm::StringRef recordName) {
  using namespace xrack::dialect;
  if constexpr (std::is_same_v<Type, RecordType> ||
                std::is_same_v<Type, RecordListType>)
    return Type::get(context, recordName);
  else
    return Type::get(context);
}

template <typename... Types>
mlir::Type getType(mlir::MLIRContext *context, llvm::StringRef mnemonic,
                   llvm::StringRef recordName) {
  using Factory = mlir::Type (*)(mlir::MLIRContext *, llvm::StringRef);
  static const llvm::StringMap<Factory> factories = {
      {Types::getMnemonic(), &buildType<Types>}...};
  auto factory = factories.lookup(mnemonic);
  return factory ? factory(context, recordName) : mlir::Type();
}

using OpFactory = mlir::Operation *(*)(mlir::OpBuilder &, mlir::Location,
                                       mlir::TypeRange, mlir::ValueRange,
                                       llvm::ArrayRef<mlir::NamedAttribute>);

template <typename Op>
mlir::Operation *buildOp(mlir::OpBuilder &builder, mlir::Location location,
                         mlir::TypeRange results, mlir::ValueRange operands,
                         llvm::ArrayRef<mlir::NamedAttribute> attributes) {
  return Op::create(builder, location, results, operands, attributes)
      .getOperation();
}

template <typename... Ops> OpFactory getOpFactory(llvm::StringRef mnemonic) {
  constexpr llvm::StringLiteral prefix("xr.");
  static const llvm::StringMap<OpFactory> factories = {
      {Ops::getOperationName().drop_front(prefix.size()), &buildOp<Ops>}...};
  return factories.lookup(mnemonic);
}
} // namespace

extern "C" MlirType xrackMlirGetType(MlirContext context,
                                     MlirStringRef mnemonic,
                                     MlirStringRef recordName) {
  return wrap(getType<
#define GET_TYPEDEF_LIST
#include "mlir/XrackTypes.cpp.inc"
              >(unwrap(context), unwrap(mnemonic), unwrap(recordName)));
}

extern "C" MlirOperation
xrackMlirBuildOp(MlirBlock block, MlirLocation location, MlirStringRef mnemonic,
                 intptr_t numResults, const MlirType *results,
                 intptr_t numOperands, const MlirValue *operands,
                 intptr_t numAttributes, const MlirNamedAttribute *attributes) {
  auto factory = getOpFactory<
#define GET_OP_LIST
#include "mlir/XrackOps.cpp.inc"
      >(unwrap(mnemonic));
  if (!factory)
    return {nullptr};

  llvm::SmallVector<mlir::Type, 1> resultTypes;
  llvm::SmallVector<mlir::Value, 4> inputs;
  llvm::SmallVector<mlir::NamedAttribute, 4> attrs;
  for (intptr_t i = 0; i < numResults; ++i)
    resultTypes.push_back(unwrap(results[i]));
  for (intptr_t i = 0; i < numOperands; ++i)
    inputs.push_back(unwrap(operands[i]));
  for (intptr_t i = 0; i < numAttributes; ++i)
    attrs.emplace_back(unwrap(attributes[i].name),
                       unwrap(attributes[i].attribute));
  auto builder = mlir::OpBuilder::atBlockEnd(unwrap(block));
  return wrap(factory(builder, unwrap(location), resultTypes, inputs, attrs));
}

extern "C" bool xrackMlirVerifyModule(MlirModule module) {
  return mlir::succeeded(xrack::dialect::verifyModule(unwrap(module)));
}

extern "C" bool xrackMlirParseFloatLiteral(MlirStringRef spelling,
                                            double *result) {
  auto value = xrack::dialect::parseFloatLiteral(unwrap(spelling));
  if (!value || !result)
    return false;
  *result = *value;
  return true;
}

extern "C" void xrackMlirAddPipeline(MlirPassManager manager) {
  xrack::dialect::buildPipeline(*unwrap(manager));
}
