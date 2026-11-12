#include "mlir/xrack_analysis.h"

namespace xrack::dialect {

using namespace mlir;

PropertyAnalysis::PropertyAnalysis(Operation *module) {
  module->walk([&](PropertyOp property) {
    auto *parent = property->getParentOp();
    auto component = property->getParentOfType<ComponentOp>();
    if (auto element = dyn_cast<ElementOp>(parent)) {
      if (isa<ComponentOp>(parent->getParentOp()))
        return;
      auto key = element.getKey().value_or(element.getId());
      if (!key.empty())
        components[component][key].try_emplace(property.getName(), property);
    } else if (isa<ComponentOp>(parent)) {
      components[component]["root"].try_emplace(property.getName(), property);
    }
  });

  module->walk([&](ReadOp read) {
    auto property = resolve(read);
    if (!property)
      return;
    for (auto *user : read.getResult().getUsers()) {
      auto alias = dyn_cast<PropertyOp>(user);
      if (isa<ControlLinkOp, InstanceLinkOp, RowLinkOp>(user) ||
          (alias && alias.getAlias().value_or("") == "true"))
        writes.insert(property);
    }
  });
  module->walk([&](AssignOp assign) {
    if (auto property = resolve(assign, assign.getOwner().value_or("root"),
                                assign.getName()))
      writes.insert(property);
  });
}

PropertyOp PropertyAnalysis::resolve(Operation *op, llvm::StringRef owner,
                                     llvm::StringRef name) const {
  auto component = components.find(op->getParentOfType<ComponentOp>());
  if (component == components.end())
    return {};
  auto scope = component->second.find(owner);
  return scope == component->second.end() ? PropertyOp{}
                                          : scope->second.lookup(name);
}

PropertyOp PropertyAnalysis::resolve(ReadOp read) const {
  return resolve(read, read.getOwner().value_or("root"), read.getValue());
}

bool PropertyAnalysis::isImmutablePrivate(PropertyOp property) const {
  return property && property.getVisibility() == "private" &&
         property.getAlias().value_or("") != "true" &&
         !writes.contains(property);
}

bool PropertyAnalysis::isImmutablePrivateScalar(PropertyOp property) const {
  return isImmutablePrivate(property) &&
         isa<IntType, FloatType, BoolType, StringType, LengthType, PercentType,
             ColorType>(property.getInitial().getType());
}

} // namespace xrack::dialect
