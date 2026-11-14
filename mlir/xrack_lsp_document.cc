#include "mlir/xrack_lsp_document.h"

#include "mlir/xrack_dialect.h"

#include "mlir/AsmParser/AsmParserState.h"
#include "mlir/IR/Diagnostics.h"
#include "llvm/ADT/STLExtras.h"
#include "llvm/ADT/StringMap.h"
#include "llvm/Support/MemoryBuffer.h"
#include "llvm/Support/SourceMgr.h"

using namespace mlir;
using namespace xrack::dialect;

namespace xrack::lsp {

void readStringAttributes(llvm::SourceMgr &source, Operation *op, SMLoc start,
                          llvm::StringMap<SMRange> &ranges);

namespace {
using Declarations = llvm::StringMap<Operation *>;

// An empty value marks an ambiguous name while a document is being edited.
void declare(Declarations &declarations, StringRef name, Operation *op) {
  if (name.empty())
    return;
  auto [entry, inserted] = declarations.try_emplace(name, op);
  if (!inserted)
    entry->second = nullptr;
}

StringRef stringAttr(Operation *op, StringRef name) {
  if (auto attr = op->getAttrOfType<StringAttr>(name))
    return attr.getValue();
  return {};
}
} // namespace

void Document::index(Block &block, const AsmParserState &state,
                     llvm::SourceMgr &source) {
  Operation *lastOperation = nullptr;
  llvm::StringMap<SMRange> attributes;
  llvm::DenseMap<Operation *, Declarations> owners;
  llvm::DenseMap<Operation *, Declarations> properties;
  llvm::DenseMap<Operation *, Declarations> callbacks;
  llvm::DenseMap<Operation *, Declarations> components;

  auto attributeRange = [&](Operation *op, StringRef name) -> SMRange {
    if (lastOperation != op) {
      lastOperation = op;
      attributes.clear();
      readStringAttributes(source, op, state.getOpDef(op)->loc.Start,
                           attributes);
    }
    return attributes.lookup(name);
  };

  auto addDeclaration = [&](Operation *op, StringRef attribute) {
    SMRange range = attributeRange(op, attribute);
    if (!range.Start.isValid() || range.Start == range.End)
      range = state.getOpDef(op)->loc;
    size_t index = symbols.size();
    declarations.try_emplace(op, index);
    symbols.push_back({op, range, {}});
    sites.push_back({range, index});
  };

  auto addUse = [&](Operation *op, StringRef attribute, Operation *target) {
    auto found = declarations.find(target);
    if (found == declarations.end())
      return;
    SMRange range = attributeRange(op, attribute);
    if (!range.Start.isValid() || range.Start == range.End)
      return;
    symbols[found->second].uses.push_back(range);
    sites.push_back({range, found->second});
  };

  for (const auto &def : state.getOpDefs()) {
    auto *op = def.op;
    if (auto component = dyn_cast<ComponentOp>(op)) {
      declare(components[op->getParentOp()], component.getSymName(), op);
      declare(owners[op], "root", op);
      addDeclaration(op, "sym_name");
      for (auto range : def.symbolUses) {
        auto index = declarations.lookup(op);
        symbols[index].uses.push_back(range);
        sites.push_back({range, index});
      }
      continue;
    }
    if (isa<RecordDeclOp>(op)) {
      addDeclaration(op, "sym_name");
      continue;
    }
    auto component = op->getParentOfType<ComponentOp>();
    if (!component)
      continue;

    StringRef key = scopedOwnerKey(op);
    if (!key.empty())
      declare(owners[component], key, op);
    if (isa<ElementOp, InstanceOp>(op)) {
      if (!stringAttr(op, "id").empty())
        addDeclaration(op, "id");
    } else if (auto property = dyn_cast<PropertyOp>(op)) {
      declare(properties[op->getParentOp()], property.getName(), op);
      addDeclaration(op, "name");
    } else if (auto callback = dyn_cast<CallbackOp>(op);
               callback && !isa<InstanceOp>(op->getParentOp())) {
      declare(callbacks[op->getParentOp()], callback.getName(), op);
      addDeclaration(op, "name");
    }
  }

  auto lookupName = [](const auto &scopes, Operation *scope,
                       StringRef name) -> Operation * {
    auto entry = scopes.find(scope);
    return entry == scopes.end() ? nullptr : entry->second.lookup(name);
  };
  auto memberScope = [&](Operation *scope, ComponentOp component) {
    if (auto instance = dyn_cast_or_null<InstanceOp>(scope))
      return lookupName(components, component->getParentOp(),
                        instance.getComponent());
    return scope;
  };

  for (const auto &def : state.getOpDefs()) {
    auto *op = def.op;
    auto component = op->getParentOfType<ComponentOp>();
    if (!component || op->getName().getDialectNamespace() != "xr")
      continue;
    auto owner = [&](StringRef name) {
      return lookupName(owners, component, name);
    };

    // Owner keys live in a component-wide namespace. Names of properties and
    // callbacks are scoped to that owner or to the component an instance uses.
    auto *target = owner(stringAttr(op, "owner"));
    if (target != component.getOperation())
      addUse(op, "owner", target);
    if (isa<FocusTargetOp, FocusCommandOp, ScrollTargetOp>(op))
      addUse(op, "target", owner(stringAttr(op, "target")));
    if (auto fill = dyn_cast<SplitFillOp>(op)) {
      auto &panes = cast<ElementOp>(op->getParentOp()).getBody().front();
      auto pane = llvm::find_if(panes, [&](Operation &candidate) {
        return isa<ElementOp, InstanceOp>(candidate) &&
               stringAttr(&candidate, "id") == fill.getTarget();
      });
      if (pane != panes.end())
        addUse(op, "target", &*pane);
    }
    if (isa<ActionGroupRefOp, RadioGroupRefOp, CheckGroupRefOp>(op))
      addUse(op, "group", owner(stringAttr(op, "group")));

    if (isa<ReadOp, AssignOp, CallOp, InvokeOp>(op)) {
      if (!op->getAttr("owner"))
        target = component;
      bool callback = isa<CallOp, InvokeOp>(op);
      StringRef field = isa<ReadOp, CallOp>(op) ? "value" : "name";
      addUse(op, field,
             lookupName(callback ? callbacks : properties,
                        memberScope(target, component), stringAttr(op, field)));
    } else if (auto instance = dyn_cast<InstanceOp>(op->getParentOp())) {
      auto *scope = memberScope(instance, component);
      if (isa<CallbackOp>(op))
        addUse(op, "name",
               lookupName(callbacks, scope, stringAttr(op, "name")));
      else if (isa<BindOp, InstanceLinkOp, InstanceStateOp, ClipboardPasteOp>(
                   op)) {
        StringRef field = isa<ClipboardPasteOp>(op) ? "target" : "name";
        addUse(op, field, lookupName(properties, scope, stringAttr(op, field)));
      }
    } else if (auto callback = dyn_cast<CallbackOp>(op)) {
      auto *alias = owner(callback.getAliasOwner().value_or(""));
      if (alias != component.getOperation())
        addUse(op, "alias_owner", alias);
      addUse(op, "alias_name",
             lookupName(callbacks, memberScope(alias, component),
                        callback.getAliasName().value_or("")));
    }
  }

  llvm::sort(sites, [](const Site &lhs, const Site &rhs) {
    return lhs.range.Start.getPointer() < rhs.range.Start.getPointer();
  });

  // Keep operation locations pointing at the .xr source. For verification
  // only, add the textual MLIR location so errors underline the offending op.
  auto verify = [&](ModuleOp module) {
    if (llvm::none_of(module.getBody()->getOperations(), [](Operation &op) {
          return op.getName().getDialectNamespace() == "xr";
        }))
      return;
    llvm::SmallVector<std::pair<Operation *, Location>> locations;
    llvm::DenseMap<Location, Location> originals;
    auto filename =
        source.getMemoryBuffer(source.getMainFileID())->getBufferIdentifier();
    module.walk([&](Operation *op) {
      if (auto *def = state.getOpDef(op)) {
        auto [line, column] = source.getLineAndColumn(def->loc.Start);
        Location original = op->getLoc();
        auto textLoc =
            FileLineColLoc::get(op->getContext(), filename, line, column);
        auto combined = FusedLoc::get(op->getContext(), {textLoc, original});
        locations.emplace_back(op, original);
        if (combined != textLoc)
          originals.try_emplace(combined, original);
        op->setLoc(combined);
      }
    });
    bool emitted = false;
    ScopedDiagnosticHandler observe(module.getContext(), [&](Diagnostic &diag) {
      emitted = true;
      auto original = originals.find(diag.getLocation());
      if (original != originals.end())
        diag.attachNote(original->second) << "original XRack source";
      return failure(); // Forward to the language server's diagnostic handler.
    });
    if (failed(verifyModule(module)) && !emitted)
      module.emitError("XRack module verification failed");
    for (auto &[op, location] : locations)
      op->setLoc(location);
  };

  for (auto &op : block)
    if (auto module = dyn_cast<ModuleOp>(op))
      verify(module);
}

const Document::Symbol *Document::lookup(SMLoc point) const {
  if (!point.isValid())
    return nullptr;
  auto it = llvm::upper_bound(sites, point.getPointer(),
                              [](const char *offset, const Site &site) {
                                return offset < site.range.Start.getPointer();
                              });
  if (it == sites.begin())
    return nullptr;
  --it;
  return point.getPointer() < it->range.End.getPointer() ? &symbols[it->symbol]
                                                         : nullptr;
}

const Document::Symbol *Document::declaration(Operation *op) const {
  auto it = declarations.find(op);
  return it == declarations.end() ? nullptr : &symbols[it->second];
}

} // namespace xrack::lsp
