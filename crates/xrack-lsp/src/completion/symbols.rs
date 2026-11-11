use super::{
    FIELD, FUNCTION, Index, Member, Role, VARIABLE, Workspace,
    imports::{Module, Target, import_targets, module_path, named_imports, normalize},
};
use crate::{
    analysis::Source,
    catalog,
    editor_tokens::{self, Token, name},
};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    ops::Range,
    path::{Path, PathBuf},
};
use xrack_ir::semantic::{builtin_event, builtin_property, is_builtin, layout_item_property};
use xrack_parser::ast::Type;

#[repr(u32)]
#[derive(Clone, Copy)]
enum Kind {
    Type,
    Class,
    Struct,
    Enum,
    EnumMember,
    Property,
    Variable,
    Parameter,
    Function,
    Event,
}

const DECLARATION: u32 = 1;
const READONLY: u32 = 2;
const BUILTIN: u32 = 4;

pub(crate) fn legend() -> Value {
    json!({
        "tokenTypes": ["type", "class", "struct", "enum", "enumMember", "property",
                       "variable", "parameter", "function", "event"],
        "tokenModifiers": ["declaration", "readonly", "defaultLibrary"],
    })
}

#[derive(Clone, PartialEq, Eq)]
struct Site {
    range: Range<usize>,
    file: Option<PathBuf>,
}

#[derive(Clone)]
struct Symbol {
    kind: Kind,
    site: Option<Site>,
    ty: Option<Type>,
    type_file: Option<PathBuf>,
    modifiers: u32,
}

impl Symbol {
    fn builtin(kind: Kind, ty: Option<Type>) -> Self {
        Self {
            kind,
            site: None,
            ty,
            type_file: None,
            modifiers: BUILTIN,
        }
    }

    fn declared(kind: Kind, range: Range<usize>, file: Option<PathBuf>) -> Self {
        Self {
            kind,
            site: Some(Site { range, file }),
            ty: None,
            type_file: None,
            modifiers: 0,
        }
    }

    fn member(member: &Member, parameter: bool) -> Self {
        let kind = match member.kind {
            FUNCTION => Kind::Function,
            VARIABLE if parameter => Kind::Parameter,
            VARIABLE => Kind::Variable,
            _ => Kind::Property,
        };
        let mut symbol = Self::declared(kind, member.range.clone(), member.file.clone());
        symbol.ty = member.ty.clone();
        symbol.type_file = member.type_file.clone();
        symbol
    }
}

pub(crate) fn query(
    method: &str,
    uri: &str,
    text: &str,
    position: Option<(usize, usize)>,
    workspace: Option<&Workspace<'_>>,
) -> Value {
    let source = Source::new(text);
    let tokens = editor_tokens::scan(text);
    let symbols = Symbols::new(&tokens, text.len(), workspace);
    if method == "textDocument/semanticTokens/full" {
        let mut data = Vec::new();
        let mut previous = (0, 0);
        for (i, token) in symbols.code.iter().enumerate() {
            let Some(symbol) = symbols.at(i) else {
                continue;
            };
            let (line, column) = source.coordinates(token.range.start);
            let length = token.text.encode_utf16().count() as u32;
            let delta_line = line - previous.0;
            let delta_start = if delta_line == 0 {
                column - previous.1
            } else {
                column
            };
            data.extend([
                delta_line as u32,
                delta_start as u32,
                length,
                symbol.kind as u32,
                symbol.modifiers,
            ]);
            previous = (line, column);
        }
        return json!({ "data": data });
    }
    let Some(cursor) = position.and_then(|(line, column)| source.offset_at(line, column)) else {
        return Value::Null;
    };
    let Some(i) = symbols
        .code
        .partition_point(|token| token.range.start <= cursor)
        .checked_sub(1)
    else {
        return Value::Null;
    };
    let hit = |i: usize| {
        let token = symbols
            .code
            .get(i)
            .filter(|token| cursor <= token.range.end)?;
        Some((*token, symbols.at(i)?))
    };
    let Some((token, symbol)) = hit(i).or_else(|| hit(i.checked_sub(1)?)) else {
        return Value::Null;
    };
    match method {
        "textDocument/definition" => {
            let Some(site) = symbol.site else {
                return Value::Null;
            };
            if let Some(file) = site.file {
                let Some(workspace) = workspace else {
                    return Value::Null;
                };
                let Some((target, content)) = workspace.uri(&file).zip(workspace.load(&file))
                else {
                    return Value::Null;
                };
                if content.get(site.range.clone()).is_none() {
                    return Value::Null;
                }
                json!({ "uri": target, "range": Source::new(&content).range(site.range.start, site.range.end) })
            } else {
                json!({ "uri": uri, "range": source.range(site.range.start, site.range.end) })
            }
        }
        "textDocument/hover" => {
            let role = match symbol.kind {
                Kind::Type => "type",
                Kind::Class => "component",
                Kind::Struct => "struct",
                Kind::Enum => "enum",
                Kind::EnumMember => "enum member",
                Kind::Property => "property",
                Kind::Variable => "variable",
                Kind::Parameter => "parameter",
                Kind::Function => "callback",
                Kind::Event => "event",
            };
            let ty = symbol.ty.as_ref().map(|ty| match ty {
                Type::Record(name) => name.to_string(),
                Type::RecordList(name) => format!("[{name}]"),
                Type::StringList => "[string]".into(),
                _ => ty.name().into_owned(),
            });
            let description = ty.map_or_else(
                || format!("{role} {}", token.text),
                |ty| format!("{role} {}: {ty}", token.text),
            );
            json!({
                "contents": { "kind": "markdown", "value": format!("```xrack\n{description}\n```") },
                "range": source.range(token.range.start, token.range.end),
            })
        }
        "textDocument/documentHighlight" => {
            let Some(site) = symbol.site else {
                return Value::Null;
            };
            json!(
                symbols
                    .code
                    .iter()
                    .enumerate()
                    .filter_map(|(i, token)| {
                        let occurrence = symbols.at(i)?;
                        (occurrence.site.as_ref() == Some(&site)).then(|| {
                            json!({
                                "range": source.range(token.range.start, token.range.end),
                                "kind": if occurrence.modifiers & DECLARATION != 0 { 3 } else { 1 },
                            })
                        })
                    })
                    .collect::<Vec<_>>()
            )
        }
        _ => Value::Null,
    }
}

struct Symbols<'a, 'w> {
    code: Vec<&'a Token<'a>>,
    index: Index<'a>,
    workspace: Option<&'a Workspace<'w>>,
    imports: BTreeMap<String, Target>,
    modules: RefCell<BTreeMap<PathBuf, Module>>,
    known: BTreeMap<usize, Symbol>,
    elements: BTreeMap<(usize, &'a str), usize>,
}

impl<'a, 'w> Symbols<'a, 'w> {
    fn new(tokens: &'a [Token<'a>], end: usize, workspace: Option<&'a Workspace<'w>>) -> Self {
        let code: Vec<_> = tokens.iter().filter(|t| t.code()).collect();
        let mut index = Index::new(&code, end);
        let imports = workspace.map_or_else(BTreeMap::new, |w| import_targets(&w.entry, tokens));
        let mut modules = BTreeMap::new();
        if let Some(workspace) = workspace {
            index.imported_members = workspace.interfaces(&imports, &mut modules);
        }
        index.infer_repeat_items();
        index.infer_handler_parameters(&code);
        let elements = index
            .scopes
            .iter()
            .enumerate()
            .filter_map(|(i, scope)| {
                let Role::Element { id: Some(id), .. } = scope.role else {
                    return None;
                };
                Some(((index.component(i)?, id), i))
            })
            .collect();
        let mut result = Self {
            code,
            index,
            workspace,
            imports,
            modules: RefCell::new(modules),
            known: BTreeMap::new(),
            elements,
        };
        result.mark_declarations();
        if let Some(workspace) = workspace {
            let imports: Vec<_> = tokens.iter().filter(|t| t.code() || t.quoted()).collect();
            for clause in named_imports(&imports) {
                let Some(file) = module_path(&workspace.entry, clause.source.text) else {
                    continue;
                };
                for binding in clause.names.split(|t| t.text == ",") {
                    let [original, ..] = binding else { continue };
                    let target = Target {
                        file: file.clone(),
                        name: original.text.into(),
                    };
                    let Some(symbol) = result.external_type(&target) else {
                        continue;
                    };
                    result.known.insert(original.range.start, symbol.clone());
                    if let [_, as_token, alias] = binding
                        && as_token.text == "as"
                    {
                        let mut alias_symbol = symbol;
                        alias_symbol.modifiers |= DECLARATION;
                        result.known.insert(alias.range.start, alias_symbol);
                    }
                }
            }
        }
        result
    }

    fn mark_declarations(&mut self) {
        let mut types = Vec::new();
        for (i, scope) in self.index.scopes.iter().enumerate() {
            let head = &self.code[scope.header.clone()];
            let start = usize::from(head.first().is_some_and(|token| token.text == "export"));
            match scope.role {
                Role::Component { .. } | Role::Record { .. } => {
                    if let Some(token) = head.get(start + 1) {
                        let mut symbol = Symbol::declared(
                            if matches!(scope.role, Role::Component { .. }) {
                                Kind::Class
                            } else {
                                Kind::Struct
                            },
                            token.range.clone(),
                            None,
                        );
                        symbol.modifiers |= DECLARATION;
                        self.known.insert(token.range.start, symbol);
                    }
                    if matches!(scope.role, Role::Component { .. })
                        && let Some(base) = head.get(start + 3)
                    {
                        self.known
                            .insert(base.range.start, self.type_name(base.text, Kind::Class));
                    }
                }
                Role::Element { id, .. } => {
                    if let Some(kind) = head.last() {
                        self.known
                            .insert(kind.range.start, self.type_name(kind.text, Kind::Class));
                    }
                    if id.is_some()
                        && let Some(token) = head.iter().rev().nth(2)
                    {
                        let mut symbol =
                            Symbol::declared(Kind::Variable, token.range.clone(), None);
                        symbol.modifiers = DECLARATION | READONLY;
                        self.known.insert(token.range.start, symbol);
                    }
                }
                Role::RecordValue(_) if let Some(token) = head.last() => {
                    self.known
                        .insert(token.range.start, self.type_name(token.text, Kind::Struct));
                }
                Role::Handler if let Some(token) = head.first() => {
                    if let Some(owner) = self.index.element(i)
                        && let Some(symbol) = self.owner_member(owner, token.text, false, true)
                    {
                        self.known.insert(token.range.start, symbol);
                    }
                }
                _ => {}
            }
            for member in &scope.members {
                let mut symbol = Symbol::member(member, matches!(scope.role, Role::Handler));
                symbol.modifiers |= DECLARATION;
                self.known.insert(member.range.start, symbol);
                let j = self
                    .code
                    .partition_point(|token| token.range.start < member.range.start);
                if member.kind == FIELD {
                    types.push(j + 2);
                } else if member.kind == FUNCTION && self.text(j + 1) == "(" {
                    let mut start = j + 2;
                    for k in j + 2..self.code.len() {
                        match self.text(k) {
                            "," | ")" => {
                                if k > start {
                                    let (param, ty) = if self.text(start + 1) == ":" {
                                        (Some(self.code[start]), start + 2)
                                    } else {
                                        (None, start)
                                    };
                                    if let Some(param) = param {
                                        let mut symbol = Symbol::declared(
                                            Kind::Parameter,
                                            param.range.clone(),
                                            None,
                                        );
                                        symbol.modifiers = DECLARATION;
                                        self.known.insert(param.range.start, symbol);
                                    }
                                    types.push(ty);
                                }
                                if self.text(k) == ")" {
                                    if self.text(k + 1) == "->" {
                                        types.push(k + 2);
                                    }
                                    break;
                                }
                                start = k + 1;
                            }
                            ";" | "{" | "}" => break,
                            _ => {}
                        }
                    }
                }
            }
        }
        for i in 0..self.code.len() {
            if self.text(i) == "property" && self.text(i + 1) == "<" {
                types.push(i + 2);
            }
        }
        for i in types {
            self.mark_type(i);
        }
    }

    fn mark_type(&mut self, i: usize) {
        let i = i + usize::from(self.text(i) == "[");
        if let Some(token) = self.code.get(i).filter(|token| name(token.text)) {
            self.known
                .insert(token.range.start, self.type_name(token.text, Kind::Type));
        }
    }

    fn text(&self, i: usize) -> &str {
        self.code.get(i).map_or("", |token| token.text)
    }

    fn type_name(&self, name: &str, fallback: Kind) -> Symbol {
        if let Some(ty) = Type::parse(name) {
            let kind = if !ty.enum_variants().is_empty() {
                Kind::Enum
            } else if matches!(
                ty,
                Type::KeyEvent | Type::KeyModifiers | Type::Date | Type::Time
            ) {
                Kind::Struct
            } else {
                Kind::Type
            };
            return Symbol::builtin(kind, Some(ty));
        }
        if is_builtin(name) {
            return Symbol::builtin(Kind::Class, None);
        }
        for scope in &self.index.scopes {
            let kind = match scope.role {
                Role::Component {
                    name: candidate, ..
                } if name == candidate => Kind::Class,
                Role::Record {
                    name: candidate, ..
                } if name == candidate => Kind::Struct,
                _ => continue,
            };
            let head = &self.code[scope.header.clone()];
            let i = 1 + usize::from(head.first().is_some_and(|token| token.text == "export"));
            if let Some(declaration) = head.get(i) {
                return Symbol::declared(kind, declaration.range.clone(), None);
            }
        }
        if let Some(symbol) = self
            .imports
            .get(name)
            .and_then(|target| self.external_type(target))
        {
            return symbol;
        }
        Symbol {
            kind: fallback,
            site: None,
            ty: None,
            type_file: None,
            modifiers: 0,
        }
    }

    fn external_type(&self, target: &Target) -> Option<Symbol> {
        let mut modules = self.modules.borrow_mut();
        let module = self.workspace?.module(&target.file, &mut modules);
        let (kind, range) =
            if let Some(component) = module.components.get(&target.name).filter(|c| c.exported) {
                (Kind::Class, component.range.clone())
            } else {
                let record = module.records.get(&target.name).filter(|r| r.exported)?;
                (Kind::Struct, record.range.clone())
            };
        Some(Symbol::declared(kind, range, Some(normalize(&target.file))))
    }

    fn owner(&self, scope: usize, name: &str) -> Option<usize> {
        match name {
            "root" => self.index.component(scope),
            "self" => self.index.element(scope),
            id => self
                .elements
                .get(&(self.index.component(scope)?, id))
                .copied(),
        }
    }

    fn owner_member(&self, owner: usize, name: &str, public: bool, call: bool) -> Option<Symbol> {
        let scope = &self.index.scopes[owner];
        if let Some(member) = self
            .index
            .members(owner, public)
            .into_iter()
            .find(|member| member.name == name && (!call || member.kind == FUNCTION))
        {
            return Some(Symbol::member(member, matches!(scope.role, Role::Handler)));
        }
        let kind = match scope.role {
            Role::Component { base, .. } | Role::Element { kind: base, .. } => base,
            _ => return None,
        };
        if call && builtin_event(kind, name) {
            return Some(Symbol::builtin(Kind::Event, None));
        }
        builtin_property(kind, name)
            .or_else(|| layout_item_property(name))
            .map(|ty| Symbol::builtin(Kind::Property, Some(ty)))
    }

    fn field(&self, parent: &Symbol, name: &str) -> Option<Symbol> {
        let ty = parent.ty.as_ref()?;
        if let Some(ty) = ty.key_field(name).or_else(|| {
            (matches!(ty, Type::Image) && matches!(name, "width" | "height")).then_some(Type::Int)
        }) {
            let mut symbol = Symbol::builtin(Kind::Property, Some(ty));
            symbol.modifiers |= READONLY;
            return Some(symbol);
        }
        let Type::Record(record) = ty else {
            return None;
        };
        self.record_field(record, name, parent.type_file.as_deref())
    }

    fn record_field(&self, record: &str, field: &str, file: Option<&Path>) -> Option<Symbol> {
        if file.is_none()
            && let Some(member) = self
                .index
                .record(record)
                .iter()
                .find(|member| member.name == field)
        {
            return Some(Symbol::member(member, false));
        }
        let workspace = self.workspace?;
        let mut modules = self.modules.borrow_mut();
        let target = if let Some(file) = file {
            let module = workspace.module(file, &mut modules);
            if let Some(record) = module.records.get(record) {
                let member = record.fields.iter().find(|member| member.name == field)?;
                return Some(Symbol::member(member, false));
            }
            module.imports.get(record)?.clone()
        } else {
            self.imports.get(record)?.clone()
        };
        let module = workspace.module(&target.file, &mut modules);
        let record = module
            .records
            .get(&target.name)
            .filter(|record| record.exported)?;
        let member = record.fields.iter().find(|member| member.name == field)?;
        Some(Symbol::member(member, false))
    }

    fn at(&self, i: usize) -> Option<Symbol> {
        let token = self.code[i];
        if let Some(symbol) = self.known.get(&token.range.start) {
            return Some(symbol.clone());
        }
        let word = token.text;
        if !name(word) {
            return catalog::FUNCTIONS
                .split_ascii_whitespace()
                .any(|f| f == word)
                .then(|| Symbol::builtin(Kind::Function, None));
        }
        let scope = self.index.scope_at(token.range.start);
        let next = self.text(i + 1);
        let call = next == "(";
        let mut start = i;
        while start >= 2 && self.text(start - 1) == "." && name(self.text(start - 2)) {
            start -= 2;
        }
        if start != i {
            let receiver = self.text(start);
            if let Some(ty) = Type::parse(receiver)
                && start + 2 == i
                && ty.enum_variants().contains(&word)
            {
                return Some(Symbol::builtin(Kind::EnumMember, Some(ty)));
            }
            let mut symbol = if let Some(owner) = self.owner(scope, receiver) {
                let public = matches!(self.index.scopes[owner].role, Role::Element { kind, .. } if !is_builtin(kind));
                self.owner_member(owner, self.text(start + 2), public, call && start + 2 == i)?
            } else {
                let base = self.at(start)?;
                self.field(&base, self.text(start + 2))?
            };
            for field in (start + 4..=i).step_by(2) {
                symbol = self.field(&symbol, self.text(field))?;
            }
            return Some(symbol);
        }
        if matches!(next, ":" | "<=>")
            && i.checked_sub(1)
                .is_some_and(|previous| matches!(self.text(previous), "{" | "}" | ";" | ","))
        {
            if let Role::RecordValue(record) = self.index.scopes[scope].role {
                return self.record_field(record, word, None);
            }
            if let Some(owner) = self.index.element(scope)
                && let Some(symbol) = self.owner_member(owner, word, false, false)
            {
                return Some(symbol);
            }
        }
        if let Some(symbol) = self.index.ancestors(scope).find_map(|owner| {
            self.index
                .members(owner, false)
                .into_iter()
                .find(|member| member.name == word && (!call || member.kind == FUNCTION))
                .map(|member| {
                    Symbol::member(
                        member,
                        matches!(self.index.scopes[owner].role, Role::Handler),
                    )
                })
        }) {
            return Some(symbol);
        }
        if next == "." || !matches!(word, "root" | "self") {
            if let Some(owner) = self.owner(scope, word) {
                let role = self.index.scopes[owner].role;
                let head = &self.code[self.index.scopes[owner].header.clone()];
                let declaration = match role {
                    Role::Component { .. } => head.get(
                        1 + usize::from(head.first().is_some_and(|token| token.text == "export")),
                    ),
                    Role::Element { id: Some(_), .. } => head.iter().rev().nth(2),
                    _ => None,
                };
                return Some(declaration.map_or_else(
                    || Symbol::builtin(Kind::Variable, None),
                    |token| Symbol::declared(Kind::Variable, token.range.clone(), None),
                ));
            }
            if next == "." && Type::parse(word).is_some_and(|ty| !ty.enum_variants().is_empty()) {
                return Some(self.type_name(word, Kind::Enum));
            }
        }
        if let Some(owner) = self.index.element(scope)
            && let Some(symbol) = self.owner_member(owner, word, false, call)
        {
            return Some(symbol);
        }
        if i >= 2
            && self.text(i - 1) == ":"
            && let Some(ty) = self.at(i - 2).and_then(|symbol| symbol.ty)
            && ty.enum_variants().contains(&word)
        {
            return Some(Symbol::builtin(Kind::EnumMember, Some(ty)));
        }
        None
    }
}
