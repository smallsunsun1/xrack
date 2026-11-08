#include "native/bridge.h"
#include "antlr4-runtime.h"
#include "grammar/generated/XrackLexer.h"
#include "grammar/generated/XrackParser.h"
#include "llvm/Support/JSON.h"
#include "llvm/Support/raw_ostream.h"
#include <cstdlib>
#include <cstring>
#include <mutex>
#include <stdexcept>

namespace {
using llvm::json::Array;
using llvm::json::Object;
using llvm::json::Value;

struct Node {
  std::string rule, text;
  size_t line = 1, column = 1;
  std::vector<Node> children;
  const Node &one(const std::string &name) const {
    for (auto &child : children)
      if (child.rule == name)
        return child;
    throw std::runtime_error("missing parse-tree rule: " + name);
  }
  std::vector<const Node *> all(const std::string &name) const {
    std::vector<const Node *> out;
    for (auto &child : children)
      if (child.rule == name)
        out.push_back(&child);
    return out;
  }
  std::vector<std::string> tokens() const {
    std::vector<std::string> out;
    for (auto &child : children)
      if (child.rule.empty() || child.rule == "identifier")
        out.push_back(child.text);
    return out;
  }
  Object span() const {
    return Object{{"line", int64_t(line)}, {"column", int64_t(column)}};
  }
};

Node snapshot(antlr4::tree::ParseTree *tree,
              const std::vector<std::string> &rules) {
  Node out;
  out.text = tree->getText();
  if (auto ctx = dynamic_cast<antlr4::ParserRuleContext *>(tree)) {
    out.rule = rules.at(ctx->getRuleIndex());
    if (auto token = ctx->getStart()) {
      out.line = token->getLine();
      out.column = token->getCharPositionInLine() + 1;
    }
  }
  for (auto child : tree->children)
    out.children.push_back(snapshot(child, rules));
  return out;
}
Value tag(const char *name, Value value) {
  return Object{{name, std::move(value)}};
}

Value expression(const Node &n);

Object call(const Node &node) {
  Array arguments;
  for (auto arg : node.all("expression"))
    arguments.push_back(expression(*arg));
  return Object{{"callee", node.one("reference").text},
                {"arguments", std::move(arguments)},
                {"span", node.span()}};
}

Value expression(const Node &n) {
  const auto &c = n.children;
  Value kind = nullptr;
  if (n.rule == "reference")
    kind = tag("Reference", n.text);
  else if (n.rule == "call")
    kind = tag("Call", call(n));
  else if (n.rule == "imageUrl")
    kind = tag("ImageUrl", c.at(2).text);
  else if (n.rule == "recordLiteral") {
    Array fields;
    for (auto value : n.all("recordValue"))
      fields.push_back(Object{{"name", value->one("identifier").text},
                              {"value", expression(value->one("expression"))}});
    kind = tag("Record", Object{{"name", n.one("identifier").text},
                                {"fields", std::move(fields)}});
  } else if (n.rule == "arrayLiteral") {
    Array values;
    for (auto value : n.all("expression"))
      values.push_back(expression(*value));
    kind = tag("Array", std::move(values));
  } else if (n.rule == "gradient") {
    Array stops;
    for (auto stop : n.all("gradientStop")) {
      auto tokens = stop->tokens();
      stops.push_back(Object{
          {"color", expression(stop->one("expression"))},
          {"position", tokens.empty() ? Value(nullptr) : Value(tokens[0])},
          {"span", stop->span()}});
    }
    auto angle = n.all("gradientAngle");
    if (angle.empty()) {
      kind = tag("RadialGradient", Object{{"stops", std::move(stops)}});
    } else {
      auto degrees = angle[0]->text;
      degrees.resize(degrees.size() - 3); // Strip "deg".
      kind = tag("LinearGradient", Object{{"angle", std::move(degrees)},
                                          {"stops", std::move(stops)}});
    }
  } else if (n.rule == "primary") {
    if (c.at(0).text == "(")
      return expression(n.one("expression"));
    if (c.at(0).rule == "reference" || c.at(0).rule == "gradient" ||
        c.at(0).rule == "imageUrl" || c.at(0).rule == "arrayLiteral" ||
        c.at(0).rule == "recordLiteral" || c.at(0).rule == "call")
      return expression(c[0]);
    auto value = c[0].text;
    std::string ty = "int";
    if (value[0] == '"')
      ty = "string";
    else if (value[0] == '#')
      ty = "color";
    else if (value == "true" || value == "false")
      ty = "bool";
    else if (c.size() > 1)
      ty = c[1].text == "%" ? "percent" : "length";
    else if (value.find_first_of(".eE") != std::string::npos)
      ty = "float";
    kind = tag("Literal", Object{{"value", value}, {"ty", ty}});
  } else if (n.rule == "unary" && c.size() == 2) {
    kind = tag("Unary", Object{{"op", c[0].text}, {"value", expression(c[1])}});
  } else if (n.rule == "conditional" && c.size() > 1) {
    kind = tag("Conditional", Object{{"condition", expression(c[0])},
                                     {"yes", expression(c[2])},
                                     {"no", expression(c[4])}});
  } else if (c.size() == 1)
    return expression(c[0]);
  else {
    auto left = expression(c.at(0));
    for (size_t i = 1; i < c.size(); i += 2)
      left = Object{
          {"kind", tag("Binary", Object{{"op", c[i].text},
                                        {"left", std::move(left)},
                                        {"right", expression(c.at(i + 1))}})},
          {"span", n.span()}};
    return left;
  }
  return Object{{"kind", std::move(kind)}, {"span", n.span()}};
}

Array members(const Node &n);
Value member(const Node &wrapper) {
  const Node &n = wrapper.children.at(0);
  const auto t = n.tokens();
  Object out{{"span", n.span()}};
  if (n.rule == "animation") {
    out["property"] = t.at(1);
    out["duration_ms"] = t.at(5);
    return tag("Animation", std::move(out));
  }
  if (n.rule == "property") {
    out["name"] = t.at(3);
    out["ty"] = n.one("typeName").text;
    auto visibility = n.all("visibility");
    out["visibility"] = visibility.empty() ? "private" : visibility[0]->text;
    auto value = n.all("expression");
    out["value"] = value.empty() ? Value(nullptr) : expression(*value[0]);
    return tag("Property", std::move(out));
  }
  if (n.rule == "callback") {
    bool pure = t.at(0) == "pure";
    out["name"] = t.at(pure ? 2 : 1);
    if (pure)
      out["pure"] = true;
    Array parameters;
    for (auto p : n.all("parameter")) {
      auto names = p->all("identifier");
      parameters.push_back(Object{{"name", names.empty() ? "" : names[0]->text},
                                  {"ty", p->one("typeName").text}});
    }
    out["parameters"] = std::move(parameters);
    auto returns = n.all("typeName");
    if (!returns.empty())
      out["return_type"] = returns[0]->text;
    return tag("Callback", std::move(out));
  }
  if (n.rule == "binding") {
    out["name"] = t.at(0);
    if (t.at(1) == "<=>") {
      out["two_way"] = true;
      out["value"] = expression(n.one("reference"));
    } else {
      out["value"] = expression(n.one("expression"));
    }
    return tag("Binding", std::move(out));
  }
  if (n.rule == "handler") {
    out["name"] = t.at(0);
    Array parameters, statements;
    if (t.at(1) == "(")
      for (size_t i = 2; t.at(i) != ")"; ++i)
        if (t[i] != ",")
          parameters.push_back(t[i]);
    for (auto s : n.all("statement")) {
      if (auto calls = s->all("call"); !calls.empty()) {
        statements.push_back(call(*calls[0]));
      } else {
        statements.push_back(Object{{"target", s->one("reference").text},
                                    {"op", s->one("assignmentOp").text},
                                    {"value", expression(s->one("expression"))},
                                    {"span", s->span()}});
      }
    }
    out["parameters"] = std::move(parameters);
    out["statements"] = std::move(statements);
    auto result = n.all("expression");
    if (!result.empty())
      out["result"] = expression(*result[0]);
    return tag("Handler", std::move(out));
  }
  if (n.rule == "element") {
    bool named = t.size() > 1 && t[1] == ":=";
    out["id"] = named ? Value(t[0]) : Value(nullptr);
    out["ty"] = t[named ? 2 : 0];
    out["members"] = members(n);
    if (auto conditions = n.all("conditionClause"); !conditions.empty())
      out["condition"] = expression(conditions[0]->one("expression"));
    if (auto repeats = n.all("repeatClause"); !repeats.empty()) {
      const auto &repeat = *repeats[0];
      auto names = repeat.all("identifier");
      out["repeat"] = Object{
          {"item", names.at(0)->text},
          {"index", names.size() == 2 ? Value(names[1]->text) : Value(nullptr)},
          {"model", expression(repeat.one("expression"))}};
    }
    return tag("Element", std::move(out));
  }
  throw std::runtime_error("unrecognized member rule");
}
Array members(const Node &n) {
  Array out;
  for (auto m : n.all("member"))
    out.push_back(member(*m));
  return out;
}

class Errors : public antlr4::BaseErrorListener {
public:
  Array diagnostics;
  void syntaxError(antlr4::Recognizer *, antlr4::Token *, size_t line,
                   size_t column, const std::string &message,
                   std::exception_ptr) override {
    diagnostics.push_back(
        Object{{"span", Object{{"line", int64_t(line)},
                               {"column", int64_t(column + 1)}}},
               {"message", message}});
  }
};
XrackBuffer encode(Value value) {
  std::string text;
  llvm::raw_string_ostream stream(text);
  stream << value;
  auto data = static_cast<unsigned char *>(std::malloc(text.size()));
  if (!data)
    return {nullptr, 0};
  std::memcpy(data, text.data(), text.size());
  return {data, text.size()};
}
Value failure(const std::string &message) {
  return Object{
      {"document", nullptr},
      {"diagnostics", Array{Object{{"span", Object{{"line", 1}, {"column", 1}}},
                                   {"message", message}}}}};
}
} // namespace

extern "C" XrackBuffer xrack_parse(const unsigned char *source, size_t len) {
  try {
    static std::mutex mutex;
    std::lock_guard<std::mutex> lock(mutex);
    antlr4::ANTLRInputStream input(
        std::string(reinterpret_cast<const char *>(source), len));
    XrackLexer lexer(&input);
    Errors errors;
    lexer.removeErrorListeners();
    lexer.addErrorListener(&errors);
    antlr4::CommonTokenStream tokens(&lexer);
    XrackParser parser(&tokens);
    parser.removeErrorListeners();
    parser.addErrorListener(&errors);
    auto tree = parser.document();
    if (!errors.diagnostics.empty())
      return encode(Object{{"document", nullptr},
                           {"diagnostics", std::move(errors.diagnostics)}});
    auto root = snapshot(tree, parser.getRuleNames());
    Array imports;
    for (auto decl : root.all("importDecl")) {
      Array names;
      for (auto item : decl->all("importName")) {
        auto ids = item->all("identifier");
        names.push_back(Object{
            {"name", ids.at(0)->text},
            {"alias", ids.size() == 2 ? Value(ids[1]->text) : Value(nullptr)}});
      }
      auto tokens = decl->tokens();
      imports.push_back(Object{{"path", tokens.at(tokens.size() - 2)},
                               {"names", std::move(names)},
                               {"span", decl->span()}});
    }
    Array components;
    for (auto c : root.all("component")) {
      auto t = c->tokens();
      bool exported = t.at(0) == "export";
      size_t offset = exported ? 1 : 0;
      components.push_back(Object{{"name", t.at(offset + 1)},
                                  {"base", t.at(offset + 3)},
                                  {"exported", exported},
                                  {"members", members(*c)},
                                  {"span", c->span()}});
    }
    Array records;
    for (auto record : root.all("recordDecl")) {
      Array fields;
      for (auto field : record->all("recordField"))
        fields.push_back(Object{{"name", field->one("identifier").text},
                                {"ty", field->one("typeName").text},
                                {"span", field->span()}});
      records.push_back(Object{{"name", record->one("identifier").text},
                               {"exported", record->tokens().at(0) == "export"},
                               {"fields", std::move(fields)},
                               {"span", record->span()}});
    }
    return encode(
        Object{{"document", Object{{"components", std::move(components)},
                                   {"imports", std::move(imports)},
                                   {"records", std::move(records)}}},
               {"diagnostics", Array{}}});
  } catch (const std::exception &e) {
    return encode(failure(e.what()));
  } catch (...) {
    return encode(failure("unexpected native parser exception"));
  }
}
extern "C" void xrack_parse_free(XrackBuffer buffer) { std::free(buffer.data); }
