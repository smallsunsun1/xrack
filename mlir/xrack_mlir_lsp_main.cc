#include "mlir/xrack_dialect.h"

#include "mlir/IR/DialectRegistry.h"
#include "mlir/Tools/mlir-lsp-server/MlirLspServerMain.h"

int main(int argc, char **argv) {
  mlir::DialectRegistry registry;
  registry.insert<xrack::dialect::XrackDialect>();
  return mlir::failed(mlir::MlirLspServerMain(argc, argv, registry));
}
