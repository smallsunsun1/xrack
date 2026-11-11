include "mlir/Pass/PassBase.td"

def XrFoldConstants : Pass<"xr-fold-constants", "::mlir::ModuleOp"> {
  let summary = "Fold XR expressions and propagate immutable private scalar properties";
}

def XrSimplifyExpressions : Pass<"xr-simplify-expressions", "::mlir::ModuleOp"> {
  let summary = "Eliminate identities that preserve XR operand evaluation";
}

def XrSimplifyVisuals : Pass<"xr-simplify-visuals", "::mlir::ModuleOp"> {
  let summary = "Remove redundant visual guards and animations while preserving node identity";
}

def XrPruneConstantStates : Pass<"xr-prune-constant-states", "::mlir::ModuleOp"> {
  let summary = "Discard unreachable state cases and remap their users";
}

def XrRemoveUnusedProperties : Pass<"xr-remove-unused-properties", "::mlir::ModuleOp"> {
  let summary = "Remove unreferenced constant private scalar storage";
}

def XrPruneExpressions : Pass<"xr-prune-expressions", "::mlir::ModuleOp"> {
  let summary = "Remove unused XR expression graphs after rewrites";
}

def XrShareVisualBindings : Pass<"xr-share-visual-bindings", "::mlir::ModuleOp"> {
  let summary = "Assign shared cache slots to equivalent XR visual bindings";
}

def XrHoistRepeatInvariants : Pass<"xr-hoist-repeat-invariants", "::mlir::ModuleOp"> {
  let summary = "Cache row-independent visual bindings outside the repeat";
}

def XrHoistRepeatSubexpressions : Pass<"xr-hoist-repeat-subexpressions", "::mlir::ModuleOp"> {
  let summary = "Cache row-independent parts of a row-dependent binding";
}

def XrSpecializeHiddenVisuals : Pass<"xr-specialize-hidden-visuals", "::mlir::ModuleOp"> {
  let summary = "Lower constant-hidden visuals to stable scene placeholders";
}

def XrPlanUpdateEffects : Pass<"xr-plan-update-effects", "::mlir::ModuleOp"> {
  let summary = "Mark verified visual bindings that affect only painting";
}

def XrPlanStaticZOrder : Pass<"xr-plan-static-z-order", "::mlir::ModuleOp"> {
  let summary = "Precompute the paint order of fixed sibling lists";
}

def XrPlanRepeatRows : Pass<"xr-plan-repeat-rows", "::mlir::ModuleOp"> {
  let summary = "Mark component delegates eligible for per-row scene reuse";
}

def XrCacheStaticSubtrees : Pass<"xr-cache-static-subtrees", "::mlir::ModuleOp"> {
  let summary = "Identify passive scene subtrees that can share snapshots";
}

def XrPrecompileStaticPaths : Pass<"xr-precompile-static-paths", "::mlir::ModuleOp"> {
  let summary = "Identify constant Path commands for compile-time normalization";
}

def XrPlanStaticGrid : Pass<"xr-plan-static-grid", "::mlir::ModuleOp"> {
  let summary = "Identify grids with a fixed cell placement plan";
}

def XrCacheStaticGradients : Pass<"xr-cache-static-gradients", "::mlir::ModuleOp"> {
  let summary = "Reuse immutable gradient stops for constant brushes";
}
