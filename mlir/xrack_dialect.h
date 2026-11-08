#ifndef XRACK_MLIR_XRACK_DIALECT_H
#define XRACK_MLIR_XRACK_DIALECT_H

#include "mlir/IR/BuiltinOps.h"
#include "mlir/IR/Dialect.h"
#include "mlir/IR/OpDefinition.h"
#include "mlir/Interfaces/SideEffectInterfaces.h"
#include "mlir/XrackDialect.h.inc"

#define GET_TYPEDEF_CLASSES
#include "mlir/XrackTypes.h.inc"

#define GET_OP_CLASSES
#include "mlir/XrackOps.h.inc"

namespace xrack::dialect {
/// Check the XR compiler's top-level contract, beyond the per-operation ODS
/// checks.
::mlir::LogicalResult verifyModule(::mlir::ModuleOp module);
} // namespace xrack::dialect

#endif
