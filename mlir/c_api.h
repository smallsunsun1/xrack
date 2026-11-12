#ifndef XRACK_MLIR_C_API_H
#define XRACK_MLIR_C_API_H

#include "mlir-c/BuiltinAttributes.h"
#include "mlir-c/BuiltinTypes.h"
#include "mlir-c/Diagnostics.h"
#include "mlir-c/IR.h"
#include "mlir-c/Pass.h"

#ifdef __cplusplus
extern "C" {
#endif

MLIR_DECLARE_CAPI_DIALECT_REGISTRATION(Xrack, xr);
MlirType xrackMlirGetType(MlirContext context, MlirStringRef mnemonic,
                          MlirStringRef recordName);
MlirOperation xrackMlirBuildOp(MlirBlock block, MlirLocation location,
                               MlirStringRef mnemonic, intptr_t numResults,
                               const MlirType *results, intptr_t numOperands,
                               const MlirValue *operands,
                               intptr_t numAttributes,
                               const MlirNamedAttribute *attributes);
bool xrackMlirVerifyModule(MlirModule module);
bool xrackMlirParseFloatLiteral(MlirStringRef spelling, double *result);
void xrackMlirAddPipeline(MlirPassManager manager);

#ifdef __cplusplus
}
#endif

#endif
