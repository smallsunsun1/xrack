#ifndef XRACK_MLIR_XRACK_ANALYSIS_H
#define XRACK_MLIR_XRACK_ANALYSIS_H

#include "mlir/xrack_dialect.h"
#include "llvm/ADT/DenseMap.h"
#include "llvm/ADT/DenseSet.h"
#include "llvm/ADT/StringMap.h"

namespace xrack::dialect {

/// Property references in verified XR IR are named within a component. Keep
/// reads and writable endpoints together so a pass cannot fold through a link.
class PropertyAnalysis {
public:
  explicit PropertyAnalysis(mlir::Operation *module);

  PropertyOp resolve(ReadOp read) const;
  bool isImmutablePrivateScalar(PropertyOp property) const;
  bool hasRead(PropertyOp property) const;

private:
  using Owners = llvm::StringMap<llvm::StringMap<PropertyOp>>;
  llvm::DenseMap<mlir::Operation *, Owners> components;
  llvm::DenseSet<mlir::Operation *> reads, writes;

  PropertyOp resolve(mlir::Operation *op, llvm::StringRef owner,
                     llvm::StringRef name) const;
};

} // namespace xrack::dialect

#endif
