#ifndef XRACK_MLIR_LSP_DOCUMENT_H
#define XRACK_MLIR_LSP_DOCUMENT_H

#include "llvm/ADT/DenseMap.h"
#include "llvm/ADT/SmallVector.h"
#include "llvm/Support/SMLoc.h"

#include <cstddef>
#include <vector>

namespace llvm {
class SourceMgr;
}

namespace mlir {
class AsmParserState;
class Block;
class Operation;
} // namespace mlir

namespace xrack::lsp {

class Document {
public:
  struct Symbol {
    mlir::Operation *op;
    llvm::SMRange definition;
    llvm::SmallVector<llvm::SMRange, 2> uses;
  };

  void index(mlir::Block &block, const mlir::AsmParserState &state,
             llvm::SourceMgr &source);
  const Symbol *lookup(llvm::SMLoc point) const;
  const Symbol *declaration(mlir::Operation *op) const;

private:
  struct Site {
    llvm::SMRange range;
    size_t symbol;
  };
  std::vector<Symbol> symbols;
  std::vector<Site> sites;
  llvm::DenseMap<mlir::Operation *, size_t> declarations;
};

} // namespace xrack::lsp

#endif
