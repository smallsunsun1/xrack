include "mlir/Pass/PassBase.td"

def XrCollapsePrivateAliases : Pass<"xr-collapse-private-aliases", "::mlir::ModuleOp"> {
  let summary = "Forward local private aliases to their component property storage";
}

def XrFoldConstants : Pass<"xr-fold-constants", "::mlir::ModuleOp"> {
  let summary = "Fold XR expressions and propagate immutable private scalar properties";
}

def XrFoldRecordProjections : Pass<"xr-fold-record-projections", "::mlir::ModuleOp"> {
  let summary = "Replace reads of fields in constant XR records with literals";
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

def XrSimplifyStateTransitions : Pass<"xr-simplify-state-transitions", "::mlir::ModuleOp"> {
  let summary = "Discard disabled state transitions and remove constant enable checks";
}

def XrMergeStateCases : Pass<"xr-merge-state-cases", "::mlir::ModuleOp"> {
  let summary = "Combine adjacent state cases with identical overrides and no transitions";
}

def XrMaterializeStaticStates : Pass<"xr-materialize-static-states", "::mlir::ModuleOp"> {
  let summary = "Turn unconditional literal visual state overrides into ordinary bindings";
}

def XrRemoveUnusedProperties : Pass<"xr-remove-unused-properties", "::mlir::ModuleOp"> {
  let summary = "Remove unread, unwritten private bindings and their unused dependencies";
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

def XrSpecializeEmptyRepeats : Pass<"xr-specialize-empty-repeats", "::mlir::ModuleOp"> {
  let summary = "Use a stable placeholder for ordinary passive repeats with no rows";
}

def XrPlanUpdateEffects : Pass<"xr-plan-update-effects", "::mlir::ModuleOp"> {
  let summary = "Mark verified visual bindings that affect only painting";
}

def XrPlanFixedTextUpdates : Pass<"xr-plan-fixed-text-updates", "::mlir::ModuleOp"> {
  let summary = "Keep layout valid when text changes inside a fixed-size box";
}

def XrPlanFixedImageUpdates : Pass<"xr-plan-fixed-image-updates", "::mlir::ModuleOp"> {
  let summary = "Keep layout valid when a fixed-size image changes source";
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

def XrSpecializeStaticRepeats : Pass<"xr-specialize-static-repeats", "::mlir::ModuleOp"> {
  let summary = "Build fixed passive repeats as immutable scene snapshots";
}

def XrShareStaticSubtrees : Pass<"xr-share-static-subtrees", "::mlir::ModuleOp"> {
  let summary = "Assign shared snapshots to identical anonymous static subtrees";
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
