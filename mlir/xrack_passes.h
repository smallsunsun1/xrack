#ifndef XRACK_MLIR_XRACK_PASSES_H
#define XRACK_MLIR_XRACK_PASSES_H

namespace mlir {
class OpPassManager;
}

namespace xrack::dialect {
void buildPipeline(mlir::OpPassManager &passes);
}

#endif
