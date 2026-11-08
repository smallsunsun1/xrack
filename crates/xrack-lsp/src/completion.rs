mod imports;

use crate::{
    analysis::Source,
    catalog,
    editor_tokens::{self, Token, name},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use xrack_ir::semantic::{builtin_event, builtin_property, is_builtin, layout_item_property};
use xrack_parser::ast::Type;

pub(crate) use imports::Workspace;

const CLASS: u32 = 7;
const FIELD: u32 = 5;
const FUNCTION: u32 = 3;
const KEYWORD: u32 = 14;
const PROPERTY: u32 = 10;
const VARIABLE: u32 = 6;
const ENUM_MEMBER: u32 = 20;
const STRUCT: u32 = 22;

#[derive(Clone, Copy)]
enum Role<'a> {
    File,
    Component {
        name: &'a str,
        base: &'a str,
        exported: bool,
    },
    Element {
        kind: &'a str,
        id: Option<&'a str>,
    },
    Record(&'a str),
    RecordValue(&'a str),
    Handler,
    Animation,
    Import,
    Block,
}

#[derive(Clone)]
struct Member {
    name: String,
    ty: Option<Type>,
    visibility: &'static str,
    kind: u32,
}

struct Scope<'a> {
    role: Role<'a>,
    parent: usize,
    body: usize,
    close: usize,
    statement: usize,
    members: Vec<Member>,
}

struct Index<'a> {
    scopes: Vec<Scope<'a>>,
    imports: Vec<&'a str>,
    imported_members: BTreeMap<String, Vec<Member>>,
}

impl<'a> Index<'a> {
    fn new(tokens: &[&Token<'a>], end: usize) -> Self {
        let mut scopes = vec![Scope {
            role: Role::File,
            parent: 0,
            body: 0,
            close: end,
            statement: 0,
            members: vec![],
        }];
        let mut imports = Vec::new();
        let mut current = 0;
        for (i, token) in tokens.iter().enumerate() {
            match token.text {
                "{" => {
                    let head = &tokens[scopes[current].statement..i];
                    let role = role(head, current == 0);
                    let members = locals(head, role);
                    scopes.push(Scope {
                        role,
                        parent: current,
                        body: token.range.end,
                        close: end,
                        statement: i + 1,
                        members,
                    });
                    current = scopes.len() - 1;
                }
                "}" if current != 0 => {
                    let scope = &mut scopes[current];
                    scope.close = token.range.start;
                    if matches!(scope.role, Role::Import) {
                        imports.extend(
                            tokens[scope.statement..i]
                                .split(|t| t.text == ",")
                                .filter_map(|names| {
                                    names.last().filter(|t| name(t.text)).map(|t| t.text)
                                }),
                        );
                    }
                    current = scope.parent;
                    scopes[current].statement = i + 1;
                }
                ";" => scopes[current].statement = i + 1,
                "property"
                    if matches!(
                        scopes[current].role,
                        Role::Component { .. } | Role::Element { .. }
                    ) =>
                {
                    let declaration = &tokens[i + 1..];
                    if let Some((ty, consumed)) = property_type(declaration)
                        && let Some(member) = declaration.get(consumed).filter(|t| name(t.text))
                    {
                        let visibility = match i.checked_sub(1).map(|i| tokens[i].text) {
                            Some("in") => "in",
                            Some("out") => "out",
                            Some("in-out") => "in-out",
                            _ => "private",
                        };
                        scopes[current].members.push(Member {
                            name: member.text.into(),
                            ty: Some(ty),
                            visibility,
                            kind: PROPERTY,
                        });
                    }
                }
                "callback" if matches!(scopes[current].role, Role::Component { .. }) => {
                    if let Some(member) = tokens.get(i + 1).filter(|t| name(t.text)) {
                        scopes[current].members.push(Member {
                            name: member.text.into(),
                            ty: None,
                            visibility: "out",
                            kind: FUNCTION,
                        });
                    }
                }
                _ if matches!(scopes[current].role, Role::Record(_))
                    && name(token.text)
                    && tokens.get(i + 1).is_some_and(|t| t.text == ":") =>
                {
                    if let Some(ty) = read_type(&tokens[i + 2..]) {
                        scopes[current].members.push(Member {
                            name: token.text.into(),
                            ty: Some(ty),
                            visibility: "in-out",
                            kind: FIELD,
                        });
                    }
                }
                _ => {}
            }
        }
        Self {
            scopes,
            imports,
            imported_members: BTreeMap::new(),
        }
    }

    fn scope_at(&self, cursor: usize) -> usize {
        self.scopes
            .iter()
            .rposition(|s| s.body <= cursor && cursor <= s.close)
            .unwrap_or(0)
    }

    fn ancestors(&self, scope: usize) -> impl Iterator<Item = usize> {
        std::iter::successors(Some(scope), |&i| (i != 0).then_some(self.scopes[i].parent))
    }

    fn component(&self, scope: usize) -> Option<usize> {
        self.ancestors(scope)
            .find(|&i| matches!(self.scopes[i].role, Role::Component { .. }))
    }

    fn element(&self, scope: usize) -> Option<usize> {
        self.ancestors(scope).find(|&i| {
            matches!(
                self.scopes[i].role,
                Role::Element { .. } | Role::Component { .. }
            )
        })
    }

    fn named_element(&self, scope: usize, name: &str) -> Option<usize> {
        let component = self.component(scope)?;
        self.scopes.iter().enumerate().find_map(|(i, s)| {
            (matches!(s.role, Role::Element { id: Some(id), .. } if id == name)
                && self.component(i) == Some(component))
            .then_some(i)
        })
    }

    fn declared_component(&self, name: &str) -> Option<usize> {
        self.scopes
            .iter()
            .position(|s| matches!(s.role, Role::Component { name: n, .. } if n == name))
    }

    fn record(&self, name: &str) -> &[Member] {
        self.scopes
            .iter()
            .find(|s| matches!(s.role, Role::Record(n) if n == name))
            .map_or(&[], |s| &s.members)
    }

    fn members(&self, scope: usize, public: bool) -> Vec<&Member> {
        let mut members = Vec::new();
        let mut scope = Some((scope, public));
        let mut seen = BTreeSet::new();
        while let Some((i, public)) = scope.filter(|(i, _)| seen.insert(*i)) {
            let current = &self.scopes[i];
            members.extend(
                current
                    .members
                    .iter()
                    .filter(|member| !public || member.visibility != "private"),
            );
            let base = match current.role {
                Role::Component { base, .. } | Role::Element { kind: base, .. } => base,
                _ => break,
            };
            scope = self.declared_component(base).map(|i| (i, true));
            if scope.is_none()
                && let Some(imported) = self.imported_members.get(base)
            {
                members.extend(imported);
            }
        }
        members
    }
}

fn role<'a>(head: &[&Token<'a>], top_level: bool) -> Role<'a> {
    let exported = head.first().is_some_and(|t| t.text == "export");
    let head = match head {
        [first, rest @ ..] if first.text == "export" => rest,
        _ => head,
    };
    let first = head.first().map_or("", |t| t.text);
    let last = head.last().map_or("", |t| t.text);
    if top_level {
        return match first {
            "import" => Role::Import,
            "struct" => Role::Record(head.get(1).map_or("", |t| t.text)),
            "component" => Role::Component {
                name: head.get(1).map_or("", |t| t.text),
                base: last,
                exported,
            },
            _ => Role::Block,
        };
    }
    if last == "=>" {
        return Role::Handler;
    }
    if first == "animate" {
        return Role::Animation;
    }
    let target = match first {
        "if" | "for" => head
            .iter()
            .rposition(|t| t.text == ":")
            .map(|i| &head[i + 1..]),
        _ => Some(head),
    };
    match target {
        Some([kind]) if name(kind.text) => Role::Element {
            kind: kind.text,
            id: None,
        },
        Some([id, assign, kind]) if assign.text == ":=" && name(kind.text) => Role::Element {
            kind: kind.text,
            id: Some(id.text),
        },
        _ if name(last) => Role::RecordValue(last),
        _ => Role::Block,
    }
}

fn locals(head: &[&Token<'_>], role: Role<'_>) -> Vec<Member> {
    let names: &[&Token<'_>] = match role {
        Role::Handler => head
            .iter()
            .position(|t| t.text == "(")
            .map_or(&[], |start| &head[start + 1..]),
        Role::Element { .. } if head.first().is_some_and(|t| t.text == "for") => head
            .iter()
            .position(|t| t.text == "in")
            .map_or(&[], |end| &head[1..end]),
        _ => &[],
    };
    names
        .iter()
        .filter(|t| name(t.text))
        .map(|t| Member {
            name: t.text.into(),
            ty: None,
            visibility: "private",
            kind: VARIABLE,
        })
        .collect()
}

fn read_type(tokens: &[&Token<'_>]) -> Option<Type> {
    match tokens {
        [open, ty, close, ..] if open.text == "[" && name(ty.text) && close.text == "]" => {
            Some(Type::from_source(&format!("[{}]", ty.text)))
        }
        [ty, ..] if name(ty.text) => Some(Type::from_source(ty.text)),
        _ => None,
    }
}

fn property_type(tokens: &[&Token<'_>]) -> Option<(Type, usize)> {
    let end = match tokens {
        [open, _, close, ..] if open.text == "<" && close.text == ">" => 2,
        [open, bracket, _, _, close, ..]
            if open.text == "<" && bracket.text == "[" && close.text == ">" =>
        {
            4
        }
        _ => return None,
    };
    Some((read_type(&tokens[1..end])?, end + 1))
}

struct Items<'a> {
    prefix: &'a str,
    range: Value,
    seen: BTreeSet<String>,
    values: Vec<Value>,
}

impl<'a> Items<'a> {
    fn add(&mut self, label: &str, kind: u32, detail: &str) {
        if label.starts_with(self.prefix) && self.seen.insert(label.into()) {
            self.values.push(json!({
                "label": label, "kind": kind, "detail": detail,
                "textEdit": { "range": self.range, "newText": label },
            }));
        }
    }

    fn member(&mut self, member: &Member) {
        let detail = member.ty.as_ref().map_or_else(
            || {
                if member.kind == FUNCTION {
                    "callback".into()
                } else {
                    "local binding".into()
                }
            },
            Type::name,
        );
        self.add(&member.name, member.kind, &detail);
    }

    fn words(&mut self, words: &'a str, kind: u32, detail: &str) {
        for word in words.split_ascii_whitespace() {
            self.add(word, kind, detail);
        }
    }

    fn elements(&mut self, index: &Index<'a>) {
        for kind in catalog::ELEMENTS
            .split_ascii_whitespace()
            .filter(|kind| is_builtin(kind))
        {
            self.add(kind, CLASS, "built-in element");
        }
        for scope in &index.scopes {
            if let Role::Component { name, .. } = scope.role {
                self.add(name, CLASS, "component");
            }
        }
        for &name in &index.imports {
            self.add(name, CLASS, "import");
        }
    }

    fn types(&mut self, index: &Index<'a>) {
        self.words(catalog::TYPES, CLASS, "XRack type");
        for scope in &index.scopes {
            if let Role::Record(name) = scope.role {
                self.add(name, STRUCT, "record");
            }
        }
        for &name in &index.imports {
            self.add(name, CLASS, "import");
        }
    }
}

pub(crate) fn complete(
    text: &str,
    line: usize,
    character: usize,
    workspace: Option<&Workspace<'_>>,
) -> Vec<Value> {
    let source = Source::new(text);
    let Some(cursor) = source.offset_at(line, character) else {
        return vec![];
    };
    let tokens = editor_tokens::scan(text);
    if tokens.iter().any(|t| t.blocks_completion(cursor)) {
        return vec![];
    }
    let code: Vec<_> = tokens.iter().filter(|t| t.code()).collect();
    let prefix = code.iter().find(|t| {
        (name(t.text) || t.text.starts_with('@')) && t.range.start < cursor && cursor <= t.range.end
    });
    let start = prefix.map_or(cursor, |t| t.range.start);
    let end = prefix.map_or(cursor, |t| t.range.end);
    let prior = &code[..code.partition_point(|t| t.range.end <= start)];
    let mut index = Index::new(&code, text.len());
    if let Some(workspace) = workspace
        && !index.imports.is_empty()
    {
        index.imported_members = workspace.imported_members(&tokens);
    }
    let scope = index.scope_at(cursor);
    let head = &prior[prior
        .iter()
        .rposition(|t| matches!(t.text, "{" | "}" | ";"))
        .map_or(0, |i| i + 1)..];
    let mut items = Items {
        prefix: &text[start..cursor],
        range: source.range(start, end),
        seen: BTreeSet::new(),
        values: vec![],
    };
    suggest(&index, scope, head, &mut items);
    items.values
}

fn suggest<'a>(index: &Index<'a>, scope: usize, head: &[&Token<'a>], items: &mut Items<'a>) {
    if items.prefix.starts_with('@') {
        items.words(catalog::FUNCTIONS, FUNCTION, "built-in function");
        return;
    }
    let role = index.scopes[scope].role;
    let head = if matches!(role, Role::Record(_) | Role::RecordValue(_)) {
        &head[head
            .iter()
            .rposition(|t| t.text == ",")
            .map_or(0, |i| i + 1)..]
    } else {
        head
    };
    let first = head.first().map_or("", |t| t.text);
    let last = head.last().map_or("", |t| t.text);
    if last == "." {
        qualified(index, scope, head, items);
        return;
    }
    if last == ":=" || last == "inherits" || (matches!(first, "if" | "for") && last == ":") {
        items.elements(index);
        return;
    }
    if last == "->"
        || matches!(role, Role::Record(_)) && last == ":"
        || head.iter().any(|t| t.text == "property") && matches!(last, "<" | "[")
        || head.iter().any(|t| t.text == "callback") && last == ":"
    {
        items.types(index);
        return;
    }
    if scope == 0 {
        items.words(
            if first == "export" {
                "component struct"
            } else {
                "import export component struct"
            },
            KEYWORD,
            "XRack declaration",
        );
        return;
    }
    let kind = match role {
        Role::Component { base, .. } => base,
        Role::Element { kind, .. } => kind,
        Role::Animation => {
            items.add("duration", PROPERTY, "animation duration");
            return;
        }
        _ => "",
    };
    let expression = matches!(role, Role::Handler)
        || matches!(first, "if" | "for")
        || head
            .iter()
            .any(|t| matches!(t.text, ":" | "<=>" | "=" | "+=" | "-="));
    if expression {
        if let Some(ty) = expected_type(index, scope, head, kind) {
            for &variant in ty.enum_variants() {
                items.add(variant, ENUM_MEMBER, &ty.name());
            }
        }
        for ancestor in index.ancestors(scope) {
            for member in index.members(ancestor, false) {
                items.member(member);
            }
        }
        if let Some(component) = index.component(scope) {
            for (i, candidate) in index.scopes.iter().enumerate() {
                if let Role::Element { id: Some(id), .. } = candidate.role
                    && index.component(i) == Some(component)
                {
                    items.add(id, VARIABLE, "element id");
                }
            }
        }
        items.words("root self true false", KEYWORD, "XRack expression");
        items.words(catalog::FUNCTIONS, FUNCTION, "built-in function");
        return;
    }
    if let Role::RecordValue(record) = role {
        for member in index.record(record) {
            items.member(member);
        }
        return;
    }
    if !kind.is_empty() {
        let animation = first == "animate";
        for property in catalog::PROPERTIES.split_ascii_whitespace() {
            if (kind == "StackView" && matches!(property, "current-page" | "depth" | "can-go-back"))
                || (kind == "StackPage" && property == "visible")
            {
                continue;
            }
            let ty = if is_builtin(kind) {
                builtin_property(kind, property)
            } else {
                layout_item_property(property)
            };
            if let Some(ty) = ty
                && (!animation
                    || matches!(
                        ty,
                        Type::Int | Type::Float | Type::Length | Type::Color | Type::Brush
                    ))
            {
                items.add(property, PROPERTY, &ty.name());
            }
        }
        if !animation {
            let instance = matches!(role, Role::Element { .. }) && !is_builtin(kind);
            if matches!(kind, "Button" | "ToolButton" | "MenuItem") {
                items.add("action", PROPERTY, "Action id");
            }
            for member in index.members(scope, false) {
                if (!instance && member.kind == FUNCTION)
                    || matches!(member.visibility, "in" | "in-out")
                {
                    items.member(member);
                }
            }
            if instance {
                return;
            }
            for event in catalog::EVENTS
                .split_ascii_whitespace()
                .filter(|event| builtin_event(kind, event))
            {
                items.add(event, FUNCTION, "event handler");
            }
            items.elements(index);
            items.words(
                "property callback pure in out in-out private if for animate",
                KEYWORD,
                "XRack member",
            );
        }
    }
}

fn expected_type(index: &Index<'_>, scope: usize, head: &[&Token<'_>], kind: &str) -> Option<Type> {
    if let Some(start) = head.iter().position(|t| t.text == "property") {
        return property_type(&head[start + 1..]).map(|(ty, _)| ty);
    }
    let name = head.first()?.text;
    if !head.get(1).is_some_and(|t| t.text == ":") {
        return None;
    }
    let members = match index.scopes[scope].role {
        Role::RecordValue(record) => index.record(record).iter().collect(),
        _ => index.members(scope, false),
    };
    builtin_property(kind, name)
        .or_else(|| members.into_iter().find(|m| m.name == name)?.ty.clone())
}

fn qualified<'a>(index: &Index<'a>, scope: usize, head: &[&Token<'a>], items: &mut Items<'a>) {
    let mut receiver = Vec::new();
    for pair in head.rchunks_exact(2) {
        if pair[1].text != "." || !name(pair[0].text) {
            break;
        }
        receiver.push(pair[0].text);
    }
    let Some(first) = receiver.pop() else {
        return;
    };
    if receiver.is_empty()
        && let Some(ty) = Type::parse(first)
    {
        for &variant in ty.enum_variants() {
            items.add(variant, ENUM_MEMBER, first);
        }
    }
    let owner = match first {
        "root" => index.component(scope),
        "self" => index.element(scope),
        id => index.named_element(scope, id),
    };
    if receiver.is_empty()
        && let Some(owner) = owner
        && matches!(
            index.scopes[owner].role,
            Role::Element {
                kind: "StackView",
                ..
            } | Role::Component {
                base: "StackView",
                ..
            }
        )
    {
        for property in ["current-page", "depth", "can-go-back"] {
            if let Some(ty) = builtin_property("StackView", property) {
                items.add(property, PROPERTY, &ty.name());
            }
        }
        if matches!(index.scopes[scope].role, Role::Handler) {
            items.words(
                "push pop replace pop-to reset push-pages reset-to",
                FUNCTION,
                "StackView navigation",
            );
        }
    }
    let mut members = if let Some(owner) = owner {
        index.members(owner, false)
    } else {
        let Some(Type::Record(record)) = index.ancestors(scope).find_map(|i| {
            index
                .members(i, false)
                .into_iter()
                .find(|m| m.name == first)?
                .ty
                .clone()
        }) else {
            return;
        };
        index.record(&record).iter().collect()
    };
    for field in receiver.into_iter().rev() {
        let Some(Type::Record(record)) = members
            .iter()
            .find(|m| m.name == field)
            .and_then(|m| m.ty.as_ref())
        else {
            return;
        };
        members = index.record(record).iter().collect();
    }
    for member in members {
        items.member(member);
    }
}
