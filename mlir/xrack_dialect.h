#ifndef XRACK_MLIR_XRACK_DIALECT_H
#define XRACK_MLIR_XRACK_DIALECT_H

#include "mlir/IR/BuiltinOps.h"
#include "mlir/IR/Dialect.h"
#include "mlir/IR/OpDefinition.h"
#include "mlir/IR/SymbolTable.h"
#include "mlir/Interfaces/SideEffectInterfaces.h"
#include "mlir/XrackDialect.h.inc"
#include "llvm/ADT/StringRef.h"
#include <optional>

#define GET_TYPEDEF_CLASSES
#include "mlir/XrackTypes.h.inc"

#define GET_OP_CLASSES
#include "mlir/XrackOps.h.inc"

namespace xrack::dialect {
/// Key used to reference an element or instance within its component.
llvm::StringRef scopedOwnerKey(::mlir::Operation *op);

/// XR expressions evaluate at their use sites. Standalone actions are separate operations.
bool isExpression(::mlir::Operation &op);

/// Decode a finite XR float, length or percentage literal.
std::optional<double> parseFloatLiteral(llvm::StringRef spelling);

/// Check the XR compiler's top-level contract, beyond the per-operation ODS
/// checks.
::mlir::LogicalResult verifyModule(::mlir::ModuleOp module);
} // namespace xrack::dialect

#endif
