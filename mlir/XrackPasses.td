include "mlir/Pass/PassBase.td"

def XrFoldConstants : Pass<"xr-fold-constants", "::mlir::ModuleOp"> {
  let summary = "Fold XR integer and boolean expressions with known operands";
}
