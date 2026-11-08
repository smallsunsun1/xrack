include "mlir/Pass/PassBase.td"

def XrFoldConstants : Pass<"xr-fold-constants", "::mlir::ModuleOp"> {
  let summary = "Fold XR scalar arithmetic, conversions and string expressions";
}

def XrSimplifyExpressions : Pass<"xr-simplify-expressions", "::mlir::ModuleOp"> {
  let summary = "Eliminate identities that preserve XR operand evaluation";
}

def XrSimplifyVisuals : Pass<"xr-simplify-visuals", "::mlir::ModuleOp"> {
  let summary = "Remove unconditional visual guards while preserving node identity";
}

def XrPruneExpressions : Pass<"xr-prune-expressions", "::mlir::ModuleOp"> {
  let summary = "Remove unused XR expression graphs after rewrites";
}

def XrShareVisualBindings : Pass<"xr-share-visual-bindings", "::mlir::ModuleOp"> {
  let summary = "Assign shared cache slots to equivalent XR visual bindings";
}
