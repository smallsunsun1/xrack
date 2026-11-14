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
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    path::{Path, PathBuf},
};
use xrack_ir::semantic::{builtin_event, builtin_property, is_builtin, layout_item_property};
use xrack_parser::{ast::Type, builtins};

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
pub(super) struct Site {
    pub(super) range: Range<usize>,
    pub(super) file: Option<PathBuf>,
}

#[derive(Clone)]
struct Symbol {
    kind: Kind,
    site: Option<Site>,
    alias_site: Option<Site>,
    ty: Option<Type>,
    type_file: Option<PathBuf>,
    modifiers: u32,
}

impl Symbol {
    fn builtin(kind: Kind, ty: Option<Type>) -> Self {
        Self {
            kind,
            site: None,
            alias_site: None,
            ty,
            type_file: None,
            modifiers: BUILTIN,
        }
    }

    fn declared(kind: Kind, range: Range<usize>, file: Option<PathBuf>) -> Self {
        Self {
            kind,
            site: Some(Site { range, file }),
            alias_site: None,
            ty: None,
            type_file: None,
            modifiers: 0,
        }
    }

    fn refactor_site(&self, token: &Token<'_>) -> Option<&Site> {
        // `root` and `self` can navigate to a declaration but are keywords.
        if matches!(token.text, "root" | "self") {
            return None;
        }
        self.alias_site.as_ref().or(self.site.as_ref())
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

pub(super) fn active_calls<'a>(
    tokens: impl Iterator<Item = &'a Token<'a>>,
) -> impl Iterator<Item = (usize, usize)> {
    let mut frames = Vec::new();
    for (index, token) in tokens.enumerate() {
        match token.text {
            "(" | "[" | "{" => frames.push((index, token.text, 0)),
            ")" | "]" | "}" => {
                let matching = match token.text {
                    ")" => "(",
                    "]" => "[",
                    _ => "{",
                };
                if let Some(open) = frames
                    .iter()
                    .rposition(|(_, delimiter, _)| *delimiter == matching)
                {
                    frames.truncate(open);
                }
            }
            "," => {
                if let Some((_, "(", active)) = frames.last_mut() {
                    *active += 1;
                }
            }
            _ => {}
        }
    }
    frames
        .into_iter()
        .rev()
        .filter_map(|(open, delimiter, active)| {
            (delimiter == "(")
                .then(|| open.checked_sub(1).map(|callee| (callee, active)))
                .flatten()
        })
}

pub(crate) fn signature_help(
    text: &str,
    position: (usize, usize),
    workspace: Option<&Workspace<'_>>,
) -> Value {
    let source = Source::new(text);
    let Some(cursor) = source.offset_at(position.0, position.1) else {
        return Value::Null;
    };
    let tokens = editor_tokens::scan(text);
    if tokens.iter().any(|token| token.blocks_completion(cursor)) {
        return Value::Null;
    }
    let symbols = Symbols::new(&tokens, text.len(), workspace);
    for (callee, active) in active_calls(
        symbols
            .code
            .iter()
            .copied()
            .take_while(|token| token.range.end <= cursor),
    ) {
        let open = callee + 1;
        // Parentheses in declarations and handler heads are not calls.
        if symbols.text(callee.saturating_sub(1)) == "callback"
            || symbols
                .index
                .scopes
                .iter()
                .any(|scope| matches!(scope.role, Role::Handler) && scope.header.contains(&open))
        {
            return Value::Null;
        }
        let name = symbols.text(callee);
        if let Some((signature, parameters, documentation)) = symbols.builtin_signature(callee) {
            return json!({
                "signatures": [{
                    "label": signature,
                    "documentation": {"kind": "markdown", "value": documentation},
                    "parameters": parameters.iter().map(|label| json!({"label": label})).collect::<Vec<_>>()
                }],
                "activeSignature": 0,
                "activeParameter": active.min(parameters.len().saturating_sub(1)),
            });
        }
        let Some(symbol) = symbols
            .at(callee)
            .filter(|symbol| matches!(symbol.kind, Kind::Function))
        else {
            continue;
        };
        let Some(member) = symbols.callback_declaration(symbol.site.as_ref()) else {
            continue;
        };
        let mut label = format!("{name}({})", member.parameter_labels.join(", "));
        if let Some(result) = &member.return_label {
            label.push_str(" -> ");
            label.push_str(result);
        }
        let parameters: Vec<_> = member
            .parameter_labels
            .iter()
            .map(|label| json!({"label": label}))
            .collect();
        let mut result = json!({
            "signatures": [{"label": label, "parameters": parameters}],
            "activeSignature": 0,
        });
        if !parameters.is_empty() {
            result["activeParameter"] = json!(active.min(parameters.len() - 1));
        }
        return result;
    }
    Value::Null
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
    let Some((token, symbol)) = symbols.hit(cursor) else {
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
            let i = symbols
                .code
                .partition_point(|other| other.range.start < token.range.start);
            let description = if token.text == "@children" {
                Some(catalog::CHILDREN_DOC.to_owned())
            } else if let Some((signature, _, docs)) = symbols.builtin_signature(i) {
                Some(format!("```xrack\n{signature}\n```\n\n{docs}"))
            } else if let Some(constant) = symbols.builtin_constant(i) {
                Some(format!(
                    "```xrack\n{}.{}: float = {}\n```\n\n{}",
                    constant.namespace, constant.member, constant.value, constant.documentation
                ))
            } else if matches!(symbol.kind, Kind::EnumMember) && symbol.ty == Some(Type::LogLevel) {
                Some(format!(
                    "`LogLevel.{}` selects the {} level for `Log.write`.",
                    token.text, token.text
                ))
            } else {
                None
            };
            if let Some(description) = description {
                return json!({
                    "contents": { "kind": "markdown", "value": description },
                    "range": source.range(token.range.start, token.range.end),
                });
            }
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
            let i = symbols
                .code
                .partition_point(|other| other.range.start < token.range.start);
            if let Some((signature, _, _)) = symbols.builtin_signature(i) {
                return json!(
                    symbols
                        .code
                        .iter()
                        .enumerate()
                        .filter(|(index, occurrence)| occurrence.text == token.text
                            && symbols
                                .builtin_signature(*index)
                                .is_some_and(|(label, _, _)| label == signature))
                        .map(|(_, occurrence)| json!({
                            "range": source.range(occurrence.range.start, occurrence.range.end),
                            "kind": 1,
                        }))
                        .collect::<Vec<_>>()
                );
            }
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

pub(super) struct Occurrence {
    pub(super) range: Range<usize>,
    pub(super) site: Site,
    pub(super) declaration: bool,
    pub(super) definition: Site,
    pub(super) alias: bool,
}

pub(super) fn occurrences(
    tokens: &[Token<'_>],
    end: usize,
    workspace: Option<&Workspace<'_>>,
) -> Vec<Occurrence> {
    let symbols = Symbols::new(tokens, end, workspace);
    symbols
        .code
        .iter()
        .enumerate()
        .filter_map(|(index, token)| {
            let symbol = symbols.at(index)?;
            Some(Occurrence {
                range: token.range.clone(),
                site: symbol.refactor_site(token)?.clone(),
                declaration: symbol.modifiers & DECLARATION != 0,
                definition: symbol.site.clone()?,
                alias: symbol.alias_site.is_some(),
            })
        })
        .collect()
}

/// Detect duplicate declarations even when the surrounding document is incomplete.
pub(super) fn duplicate_declaration(
    tokens: &[Token<'_>],
    end: usize,
    edits: &[(Range<usize>, Range<usize>)],
    name: &str,
) -> bool {
    let edited = |offset| {
        edits
            .binary_search_by_key(&offset, |(_, new)| new.start)
            .is_ok()
    };
    let code: Vec<_> = tokens.iter().filter(|token| token.code()).collect();
    let index = Index::new(&code, end);
    let key = xrack_ir::semantic::rust_name(name);
    let matches = |token: &Token<'_>| xrack_ir::semantic::rust_name(token.text) == key;
    let mut types = Vec::new();
    let mut elements = Vec::new();

    for (scope_id, scope) in index.scopes.iter().enumerate() {
        let header = &code[scope.header.clone()];
        match scope.role {
            Role::Component { .. } | Role::Record { .. } => {
                let exported =
                    usize::from(header.first().is_some_and(|token| token.text == "export"));
                if let Some(token) = header.get(exported + 1).filter(|token| matches(token)) {
                    types.push(token.range.start);
                }
            }
            Role::Element { id: Some(_), .. } => {
                if let Some(token) = header
                    .iter()
                    .rev()
                    .nth(2)
                    .filter(|token| token.text == name)
                {
                    elements.push((index.component(scope_id), token.range.start));
                }
            }
            _ => {}
        }
        for local in [true, false] {
            let mut matching = scope
                .members
                .iter()
                .filter(|member| {
                    (member.kind == VARIABLE) == local
                        && xrack_ir::semantic::rust_name(&member.name) == key
                })
                .peekable();
            if let Some(first) = matching.next()
                && matching.peek().is_some()
                && (edited(first.range.start) || matching.any(|member| edited(member.range.start)))
            {
                return true;
            }
        }
        // The callback declaration's parameter names are distinct from the
        // names chosen by its handler.
        for member in scope
            .members
            .iter()
            .filter(|member| member.kind == FUNCTION)
        {
            let start = code.partition_point(|token| token.range.start <= member.range.start);
            let Some([open, parameters @ ..]) = code.get(start..) else {
                continue;
            };
            if open.text != "(" {
                continue;
            }
            let parameters = parameters
                .iter()
                .take_while(|token| !matches!(token.text, ")" | "{" | "}" | ";"))
                .copied()
                .collect::<Vec<_>>();
            let names: Vec<_> = parameters
                .split(|token| token.text == ",")
                .filter_map(|parameter| match parameter {
                    [token, colon, ..] if colon.text == ":" && matches(token) => {
                        Some(token.range.start)
                    }
                    _ => None,
                })
                .collect();
            if names.len() > 1 && names.iter().copied().any(edited) {
                return true;
            }
        }
    }
    let imports: Vec<_> = tokens
        .iter()
        .filter(|token| token.code() || token.quoted())
        .collect();
    for clause in named_imports(&imports) {
        for binding in clause.names.split(|token| token.text == ",") {
            if let Some(token) = binding.last().filter(|token| matches(token)) {
                types.push(token.range.start);
            }
        }
    }
    if (types.len() > 1 || is_builtin(name) || Type::parse(name).is_some())
        && types.iter().copied().any(edited)
    {
        return true;
    }
    elements.iter().any(|(component, offset)| {
        edited(*offset)
            && elements
                .iter()
                .any(|(other, start)| other == component && start != offset)
    })
}

struct Symbols<'a, 'w> {
    code: Vec<&'a Token<'a>>,
    index: Index<'a>,
    workspace: Option<&'a Workspace<'w>>,
    imports: BTreeMap<String, Target>,
    aliases: BTreeMap<String, Site>,
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
            aliases: BTreeMap::new(),
            modules: RefCell::new(modules),
            known: BTreeMap::new(),
            elements,
        };
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
                        input: file.clone(),
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
                        let site = Site {
                            range: alias.range.clone(),
                            file: None,
                        };
                        alias_symbol.alias_site = Some(site.clone());
                        result.aliases.insert(alias.text.into(), site);
                        result.known.insert(alias.range.start, alias_symbol);
                    }
                }
            }
        }
        result.mark_declarations();
        result
    }

    fn callback_declaration(&self, site: Option<&Site>) -> Option<Member> {
        let mut site = site?.clone();
        let mut visited = BTreeSet::new();
        loop {
            if !visited.insert((site.file.clone(), site.range.start)) {
                return None;
            }
            let (member, target) = if let Some(path) = &site.file {
                let workspace = self.workspace?.for_entry(path);
                let text = workspace.load(path)?;
                let tokens = editor_tokens::scan(&text);
                let symbols = Symbols::new(&tokens, text.len(), Some(&workspace));
                let (member, mut target) = symbols.callback_at(&site.range)?;
                if let Some(target) = &mut target {
                    target.file.get_or_insert_with(|| path.clone());
                }
                (member, target)
            } else {
                self.callback_at(&site.range)?
            };
            if let Some(target) = target {
                site = target;
            } else {
                return Some(member);
            }
        }
    }

    fn callback_at(&self, range: &Range<usize>) -> Option<(Member, Option<Site>)> {
        let (owner, member) = self
            .index
            .scopes
            .iter()
            .enumerate()
            .find_map(|(owner, scope)| {
                scope
                    .members
                    .iter()
                    .find(|member| member.kind == FUNCTION && member.range == *range)
                    .map(|member| (owner, member))
            })?;
        let Some(alias) = &member.callback_alias else {
            return Some((member.clone(), None));
        };
        let symbol = if let Some((receiver, name)) = alias.split_once('.') {
            let owner = self.owner(owner, receiver)?;
            self.owner_member(owner, name, false, true)?
        } else {
            self.index
                .ancestors(owner)
                .find_map(|owner| self.owner_member(owner, alias, false, true))?
        };
        Some((member.clone(), Some(symbol.site?)))
    }

    fn hit(&self, cursor: usize) -> Option<(&Token<'_>, Symbol)> {
        let i = self
            .code
            .partition_point(|token| token.range.start <= cursor)
            .checked_sub(1)?;
        let hit = |i: usize| {
            let token = self.code.get(i).filter(|token| cursor <= token.range.end)?;
            Some((*token, self.at(i)?))
        };
        hit(i).or_else(|| hit(i.checked_sub(1)?))
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
        if let Some(mut symbol) = self
            .imports
            .get(name)
            .and_then(|target| self.external_type(target))
        {
            symbol.alias_site = self.aliases.get(name).cloned();
            return symbol;
        }
        Symbol {
            kind: fallback,
            site: None,
            alias_site: None,
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
                .index
                .component(scope)
                .and_then(|component| self.elements.get(&(component, id)).copied())
                .or_else(|| {
                    self.index.declared_component(id).filter(|owner| {
                        matches!(
                            self.index.scopes[*owner].role,
                            Role::Component { base: "global", .. }
                        )
                    })
                }),
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
        if kind == "MediaPlayer" {
            if call
                && catalog::MEDIA_METHODS
                    .iter()
                    .any(|method| method.name == name)
            {
                return Some(Symbol::builtin(Kind::Function, None));
            }
            if let Some(ty) = xrack_ir::semantic::media_read_type(name) {
                let mut symbol = Symbol::builtin(Kind::Property, Some(ty));
                symbol.modifiers |= READONLY;
                return Some(symbol);
            }
        }
        if call && builtin_event(kind, name) {
            return Some(Symbol::builtin(Kind::Event, None));
        }
        builtin_property(kind, name)
            .or_else(|| layout_item_property(name))
            .map(|ty| Symbol::builtin(Kind::Property, Some(ty)))
    }

    fn field(&self, parent: &Symbol, name: &str) -> Option<Symbol> {
        let ty = parent.ty.as_ref()?;
        if let Some(ty) = ty.input_field(name).or_else(|| {
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

    fn builtin_namespace(&self, i: usize) -> Option<&'static str> {
        let scope = self.index.scope_at(self.code.get(i)?.range.start);
        self.index.builtin_receiver(scope, &self.code, i)
    }

    fn builtin_signature(&self, i: usize) -> Option<(String, Vec<&'static str>, &'static str)> {
        if i >= 2 && self.text(i - 1) == "." {
            let scope = self.index.scope_at(self.code[i].range.start);
            if let Some(owner) = self.owner(scope, self.text(i - 2))
                && matches!(
                    self.index.scopes[owner].role,
                    Role::Element {
                        kind: "MediaPlayer",
                        ..
                    } | Role::Component {
                        base: "MediaPlayer",
                        ..
                    }
                )
                && let Some(method) = catalog::MEDIA_METHODS
                    .iter()
                    .find(|method| method.name == self.text(i))
            {
                return Some((
                    method.signature(),
                    method.parameters.to_vec(),
                    method.documentation,
                ));
            }
        }
        if let Some(form) = catalog::builtin_function(self.text(i)) {
            return Some((
                form.signature(),
                form.parameters.to_vec(),
                form.documentation,
            ));
        }
        let function = builtins::function(self.builtin_namespace(i)?, self.text(i))?;
        Some((
            function.signature(),
            function
                .parameters
                .iter()
                .map(|parameter| parameter.label)
                .collect(),
            function.documentation,
        ))
    }

    fn builtin_constant(&self, i: usize) -> Option<&'static builtins::Constant> {
        builtins::constant(self.builtin_namespace(i)?, self.text(i))
    }

    fn at(&self, i: usize) -> Option<Symbol> {
        let token = self.code[i];
        if let Some(symbol) = self.known.get(&token.range.start) {
            return Some(symbol.clone());
        }
        let word = token.text;
        if !name(word) {
            if word == "@children" {
                return Some(Symbol::builtin(Kind::Function, None));
            }
            return catalog::builtin_function(word).map(|_| Symbol::builtin(Kind::Function, None));
        }
        if self.builtin_namespace(i).is_some() {
            return if self.builtin_signature(i).is_some() {
                Some(Symbol::builtin(Kind::Function, None))
            } else {
                self.builtin_constant(i)
                    .map(|_| Symbol::builtin(Kind::Property, Some(Type::Float)))
            };
        }
        let scope = self.index.scope_at(token.range.start);
        let next = self.text(i + 1);
        if next == "." && self.code.get(i + 2).is_some() && self.builtin_namespace(i + 2).is_some()
        {
            return Some(Symbol::builtin(Kind::Type, None));
        }
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
