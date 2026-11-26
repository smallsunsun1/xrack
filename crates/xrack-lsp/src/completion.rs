mod imports;
mod index_cache;
mod symbols;
mod workspace_index;

use crate::{
    analysis::Source,
    catalog,
    editor_tokens::{self, Token, name},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    path::PathBuf,
};
use xrack_ir::semantic::{
    builtin_event, builtin_property, content_viewport, control_alias_type, focus_property,
    geometry_owner, geometry_property, hover_property, is_builtin, layout_item_property,
    nonvisual_element, scale_rotate_property, state_property, swipe_property,
    table_history_property, table_selection_property, table_sort_property, text_command_property,
    text_selection_property, touch_area_property, view_count_property, viewport_kind,
    viewport_offset_property, window_status_property,
};
use xrack_parser::{ast::Type, builtins};

pub(crate) use imports::{Workspace, document_links, file_path, import_definition};
pub(crate) use symbols::{legend, query, signature_help};
pub(crate) use workspace_index::{BackgroundIndex, Index as WorkspaceIndex};

const CLASS: u32 = 7;
const FIELD: u32 = 5;
const FUNCTION: u32 = 3;
const KEYWORD: u32 = 14;
const PROPERTY: u32 = 10;
const VARIABLE: u32 = 6;
const ENUM: u32 = 13;
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
    Record {
        name: &'a str,
        exported: bool,
    },
    RecordValue(&'a str),
    Handler,
    StatementBlock,
    Animation,
    States,
    State,
    StateTransition,
    Import,
    Block,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Member {
    name: String,
    ty: Option<Type>,
    visibility: String,
    kind: u32,
    range: Range<usize>,
    file: Option<PathBuf>,
    type_file: Option<PathBuf>,
    parameters: Vec<Type>,
    parameter_labels: Vec<String>,
    return_label: Option<String>,
    callback_alias: Option<String>,
}

struct Scope<'a> {
    role: Role<'a>,
    parent: usize,
    header: Range<usize>,
    body: usize,
    close: usize,
    statement: usize,
    brackets: usize,
    members: Vec<Member>,
    repeat_model: Vec<&'a str>,
}

struct Index<'a> {
    scopes: Vec<Scope<'a>>,
    imports: Vec<&'a str>,
    combo_models: BTreeMap<usize, ComboModelHint<'a>>,
    value_navigation: BTreeSet<usize>,
    choice_roles: BTreeMap<(usize, &'a str), String>,
    imported_members: BTreeMap<String, Vec<Member>>,
    imported_globals: BTreeSet<String>,
}

enum ComboModelHint<'a> {
    Reference(Option<&'a str>, &'a str),
    RecordLiteral(&'a str),
    Strings,
    Unresolved,
}

impl<'a> Index<'a> {
    fn new(tokens: &[&Token<'a>], end: usize) -> Self {
        let mut scopes = vec![Scope {
            role: Role::File,
            parent: 0,
            header: 0..0,
            body: 0,
            close: end,
            statement: 0,
            brackets: 0,
            members: vec![],
            repeat_model: vec![],
        }];
        let mut imports = Vec::new();
        let mut combo_models = BTreeMap::new();
        let mut value_navigation = BTreeSet::new();
        let mut current = 0;
        for (i, token) in tokens.iter().enumerate() {
            match token.text {
                "[" if matches!(&tokens[scopes[current].statement..i], [name] if name.text == "states") =>
                {
                    scopes.push(Scope {
                        role: Role::States,
                        parent: current,
                        header: i..i,
                        body: token.range.end,
                        close: end,
                        statement: i + 1,
                        brackets: 1,
                        members: vec![],
                        repeat_model: vec![],
                    });
                    current = scopes.len() - 1;
                }
                "[" if matches!(scopes[current].role, Role::States) => {
                    scopes[current].brackets += 1
                }
                "]" if matches!(scopes[current].role, Role::States) => {
                    let scope = &mut scopes[current];
                    scope.brackets -= 1;
                    if scope.brackets == 0 {
                        scope.close = token.range.start;
                        current = scope.parent;
                        scopes[current].statement = i + 1;
                    }
                }
                "{" => {
                    let head = &tokens[scopes[current].statement..i];
                    let role = match scopes[current].role {
                        Role::States
                            if scopes[current].brackets == 1
                                && head.last().is_some_and(|token| token.text == ":") =>
                        {
                            Role::State
                        }
                        Role::State if matches!(head, [name] if matches!(name.text, "in" | "out" | "in-out")) => {
                            Role::StateTransition
                        }
                        Role::Handler | Role::StatementBlock if control_block(head) => {
                            Role::StatementBlock
                        }
                        _ => role(head, current == 0),
                    };
                    let members = locals(head, role);
                    let header = scopes[current].statement..i;
                    scopes.push(Scope {
                        role,
                        parent: current,
                        header,
                        body: token.range.end,
                        close: end,
                        statement: i + 1,
                        brackets: 0,
                        members,
                        repeat_model: repeat_model(head, role),
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
                    let record_value = matches!(scope.role, Role::RecordValue(_));
                    current = scope.parent;
                    let parent = &mut scopes[current];
                    let control_header =
                        matches!(parent.role, Role::Handler | Role::StatementBlock)
                            && tokens
                                .get(parent.statement)
                                .is_some_and(|token| matches!(token.text, "if" | "else" | "for"));
                    if !record_value || !control_header {
                        parent.statement = i + 1;
                    }
                }
                ";" => scopes[current].statement = i + 1,
                "current-value"
                    if scopes[current].statement == i
                        && tokens
                            .get(i + 1)
                            .is_some_and(|op| matches!(op.text, ":" | "<=>"))
                        && match scopes[current].role {
                            Role::Element { kind, .. } | Role::Component { base: kind, .. } => {
                                builtins::navigation_group(kind)
                            }
                            _ => false,
                        } =>
                {
                    value_navigation.insert(current);
                }
                "model"
                    if scopes[current].statement == i
                        && matches!(
                            scopes[current].role,
                            Role::Element {
                                kind: "ComboBox" | "Tumbler" | "PathView",
                                ..
                            } | Role::Component {
                                base: "ComboBox" | "Tumbler" | "PathView",
                                ..
                            }
                        )
                        && tokens.get(i + 1).is_some_and(|next| next.text == ":") =>
                {
                    let spelling =
                        |offset: usize| tokens.get(i + offset).map_or("", |token| token.text);
                    let hint = match (spelling(2), spelling(3), spelling(4)) {
                        ("[", record, "{") if name(record) => ComboModelHint::RecordLiteral(record),
                        ("[", _, _) => ComboModelHint::Strings,
                        (owner, ".", field) if name(owner) && name(field) => {
                            ComboModelHint::Reference(Some(owner), field)
                        }
                        (field, ";" | "}" | "", _) if name(field) => {
                            ComboModelHint::Reference(None, field)
                        }
                        _ => ComboModelHint::Unresolved,
                    };
                    combo_models.insert(current, hint);
                }
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
                            visibility: visibility.into(),
                            kind: PROPERTY,
                            range: member.range.clone(),
                            file: None,
                            type_file: None,
                            parameters: vec![],
                            parameter_labels: vec![],
                            return_label: None,
                            callback_alias: None,
                        });
                    }
                }
                "callback" | "function"
                    if matches!(
                        scopes[current].role,
                        Role::Component { .. } | Role::Element { .. }
                    ) =>
                {
                    if let Some(member) = tokens.get(i + 1).filter(|t| name(t.text)) {
                        let signature = callback_signature(&tokens[i + 2..]);
                        scopes[current].members.push(Member {
                            name: member.text.into(),
                            ty: None,
                            visibility: "out".into(),
                            kind: FUNCTION,
                            range: member.range.clone(),
                            file: None,
                            type_file: None,
                            parameters: signature.parameters,
                            parameter_labels: signature.labels,
                            return_label: signature.result,
                            callback_alias: signature.alias,
                        });
                    }
                }
                _ if matches!(scopes[current].role, Role::Record { .. })
                    && name(token.text)
                    && tokens.get(i + 1).is_some_and(|t| t.text == ":") =>
                {
                    if let Some(ty) = read_type(&tokens[i + 2..]) {
                        scopes[current].members.push(Member {
                            name: token.text.into(),
                            ty: Some(ty),
                            visibility: "in-out".into(),
                            kind: FIELD,
                            range: token.range.clone(),
                            file: None,
                            type_file: None,
                            parameters: vec![],
                            parameter_labels: vec![],
                            return_label: None,
                            callback_alias: None,
                        });
                    }
                }
                _ => {}
            }
        }
        Self {
            scopes,
            imports,
            combo_models,
            value_navigation,
            choice_roles: BTreeMap::new(),
            imported_members: BTreeMap::new(),
            imported_globals: BTreeSet::new(),
        }
    }

    fn scope_at(&self, cursor: usize) -> usize {
        let previous = self.scopes.partition_point(|scope| scope.body <= cursor) - 1;
        self.ancestors(previous)
            .find(|&scope| cursor <= self.scopes[scope].close)
            .unwrap_or(0)
    }

    fn ancestors(&self, scope: usize) -> impl Iterator<Item = usize> {
        std::iter::successors(Some(scope), |&i| (i != 0).then_some(self.scopes[i].parent))
    }

    fn template_inputs(&self, scope: usize) -> Option<Vec<(&'static str, Type)>> {
        self.ancestors(scope).find_map(|ancestor| {
            let Role::Element { kind, .. } = self.scopes[ancestor].role else {
                return None;
            };
            xrack_ir::semantic::visual_template_inputs(kind)
        })
    }

    fn combo_option_data(&self, scope: usize) -> Option<(Option<Type>, Option<PathBuf>)> {
        let template = self.ancestors(scope).find(|&id| {
            matches!(
                self.scopes[id].role,
                Role::Element {
                    kind: "ComboOption" | "TumblerOption" | "PathViewOption",
                    ..
                }
            )
        })?;
        let parent = self.scopes[template].parent;
        if matches!(
            self.scopes[template].role,
            Role::Element {
                kind: "TumblerOption" | "PathViewOption",
                ..
            }
        ) && !self.combo_models.contains_key(&parent)
        {
            return Some((Some(Type::Int), None));
        }
        Some(self.choice_model_item(parent))
    }

    fn choice_model_item(&self, combo: usize) -> (Option<Type>, Option<PathBuf>) {
        match self.combo_models.get(&combo) {
            Some(ComboModelHint::Reference(owner, member)) => {
                let typed = match owner {
                    Some("root") => self
                        .component(combo)
                        .and_then(|id| self.typed_member(id, member)),
                    Some("self") => self.typed_member(combo, member),
                    Some(owner) => self
                        .named_element(combo, owner)
                        .and_then(|id| self.typed_member(id, member)),
                    None => self
                        .ancestors(combo)
                        .find_map(|id| self.typed_member(id, member)),
                };
                typed.map_or((None, None), |(ty, file)| (ty.list_item(), file))
            }
            Some(ComboModelHint::RecordLiteral(record)) => {
                (Some(Type::Record((*record).into())), None)
            }
            Some(ComboModelHint::Unresolved) => (None, None),
            Some(ComboModelHint::Strings) | None => (Some(Type::String), None),
        }
    }

    fn infer_choice_roles(&mut self, tokens: &'a [Token<'a>]) {
        let tokens: Vec<_> = tokens
            .iter()
            .filter(|token| token.code() || token.quoted())
            .collect();
        for window in tokens.windows(3) {
            let [role, colon, field] = window else {
                continue;
            };
            if colon.text != ":"
                || !field.quoted()
                || !matches!(role.text, "text-role" | "value-role")
            {
                continue;
            }
            let owner = self.scope_at(role.range.start);
            if !self.combo_models.contains_key(&owner) {
                continue;
            }
            if let Ok(field) = serde_json::from_str(field.text) {
                self.choice_roles.insert((owner, role.text), field);
            }
        }
    }

    fn choice_value_type(&self, scope: usize) -> Option<Type> {
        let (Some(mut ty @ Type::Record(_)), _) = self.choice_model_item(scope) else {
            return None;
        };
        let field = self
            .choice_roles
            .get(&(scope, "value-role"))
            .or_else(|| self.choice_roles.get(&(scope, "text-role")))
            .map_or("text", String::as_str);
        for name in field.split('.') {
            let Type::Record(record) = ty else {
                return None;
            };
            let fields = self
                .imported_members
                .get(record.as_ref())
                .map_or_else(|| self.record(&record), Vec::as_slice);
            ty = fields
                .iter()
                .find(|member| member.name == name)?
                .ty
                .clone()?;
        }
        Some(ty)
    }

    fn button_methods(
        &self,
        owner: usize,
        kind: &str,
    ) -> impl Iterator<Item = &'static catalog::BuiltinFunction> {
        let in_row = self
            .ancestors(owner)
            .any(|scope| !self.scopes[scope].repeat_model.is_empty());
        catalog::button_methods(kind)
            .iter()
            .filter(move |method| !in_row || method.name == "toggle")
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

    fn builtin_namespace_visible(&self, scope: usize, namespace: &str) -> bool {
        self.named_element(scope, namespace).is_none()
            && !self.ancestors(scope).any(|id| {
                self.scopes[id]
                    .members
                    .iter()
                    .any(|member| member.kind == VARIABLE && member.name == namespace)
            })
    }

    fn builtin_receiver(
        &self,
        scope: usize,
        tokens: &[&Token<'_>],
        member: usize,
    ) -> Option<&'static str> {
        let receiver = member.checked_sub(2)?;
        let namespace = tokens.get(receiver)?.text;
        if tokens[member - 1].text != "."
            || (receiver > 0 && tokens[receiver - 1].text == ".")
            || !self.builtin_namespace_visible(scope, namespace)
        {
            return None;
        }
        builtins::NAMESPACES
            .iter()
            .copied()
            .find(|name| *name == namespace)
    }

    fn declared_component(&self, name: &str) -> Option<usize> {
        self.scopes
            .iter()
            .position(|s| matches!(s.role, Role::Component { name: n, .. } if n == name))
    }

    fn record(&self, name: &str) -> &[Member] {
        self.scopes
            .iter()
            .find(|s| matches!(s.role, Role::Record { name: n, .. } if n == name))
            .map_or(&[], |s| &s.members)
    }

    fn infer_repeat_items(&mut self, loops: bool) {
        for i in 1..self.scopes.len() {
            let scope = &self.scopes[i];
            if scope.repeat_model.is_empty() || matches!(scope.role, Role::StatementBlock) != loops
            {
                continue;
            }
            let parent = scope.parent;
            let ty = match scope.repeat_model.as_slice() {
                [owner, ".", field] => {
                    let owner = match *owner {
                        "root" => self.component(parent),
                        "self" => self.element(parent),
                        id => self.named_element(parent, id),
                    };
                    owner.and_then(|owner| self.typed_member(owner, field))
                }
                [count] if count.parse::<i64>().is_ok() => Some((Type::Int, None)),
                [name] => self
                    .ancestors(parent)
                    .find_map(|i| self.typed_member(i, name)),
                _ => None,
            };
            if let Some((ty, file)) = ty {
                let item_type = match ty {
                    Type::Int => Some(Type::Int),
                    ty => ty.list_item(),
                };
                if let Some(item_type) = item_type
                    && let Some(item) = self.scopes[i].members.first_mut()
                {
                    item.ty = Some(item_type);
                    item.type_file = file;
                }
            }
        }
    }

    fn typed_member(&self, scope: usize, name: &str) -> Option<(Type, Option<PathBuf>)> {
        let member = self
            .members(scope, false)
            .into_iter()
            .find(|member| member.name == name);
        if let Some(member) =
            member.and_then(|member| Some((member.ty.clone()?, member.type_file.clone())))
        {
            return Some(member);
        }
        let kind = match self.scopes[scope].role {
            Role::Element { kind, .. } | Role::Component { base: kind, .. } => kind,
            _ => return None,
        };
        builtin_property(kind, name).map(|ty| (ty, None))
    }

    fn infer_handler_parameters(&mut self, code: &[&Token<'_>]) {
        for i in 1..self.scopes.len() {
            let scope = &self.scopes[i];
            if !matches!(scope.role, Role::Handler) {
                continue;
            }
            let Some(name) = code.get(scope.header.start).map(|token| token.text) else {
                continue;
            };
            let signature = self.ancestors(scope.parent).find_map(|owner| {
                self.members(owner, false)
                    .into_iter()
                    .find(|member| member.kind == FUNCTION && member.name == name)
                    .map(|member| (member.parameters.clone(), member.file.clone()))
            });
            let signature = signature.or_else(|| {
                (name == "value-selected")
                    .then(|| self.choice_value_type(scope.parent))
                    .flatten()
                    .map(|ty| (vec![ty], None))
            });
            let signature = signature.or_else(|| {
                if name != "triggered"
                    || !matches!(
                        self.scopes[scope.parent].role,
                        Role::Element {
                            kind: "FrameAnimation",
                            ..
                        }
                    )
                {
                    return None;
                }
                let types = xrack_ir::semantic::event_parameters(
                    "FrameAnimation",
                    name,
                    None,
                    Default::default(),
                )
                .ok()?;
                Some((types.to_vec(), None))
            });
            if let Some((types, file)) = signature {
                for (parameter, ty) in self.scopes[i].members.iter_mut().zip(types) {
                    parameter.ty = Some(ty);
                    parameter.type_file = file.clone();
                }
            }
        }
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
            "struct" | "enum" => Role::Record {
                name: head.get(1).map_or("", |t| t.text),
                exported,
            },
            "component" | "global" => Role::Component {
                name: head.get(1).map_or("", |t| t.text),
                base: if first == "global" { "global" } else { last },
                exported,
            },
            _ => Role::Block,
        };
    }
    if last == "=>"
        || matches!(head, [pure, function, ..] if pure.text == "pure" && function.text == "function")
    {
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
    let repeat = matches!(role, Role::Element { .. } | Role::StatementBlock)
        && head.first().is_some_and(|t| t.text == "for");
    let names: &[&Token<'_>] = match role {
        Role::Handler => head
            .iter()
            .position(|t| t.text == "(")
            .map_or(&[], |start| &head[start + 1..]),
        _ if repeat => head
            .iter()
            .position(|t| t.text == "in")
            .map_or(&[], |end| &head[1..end]),
        _ => &[],
    };
    let key_handler = matches!(role, Role::Handler)
        && head.first().is_some_and(|token| {
            matches!(
                token.text,
                "capture-key-pressed" | "capture-key-released" | "key-pressed" | "key-released"
            )
        });
    let wheel_handler = matches!(role, Role::Handler)
        && head
            .first()
            .is_some_and(|token| matches!(token.text, "wheel" | "scroll-event"));
    let pointer_handler = matches!(role, Role::Handler)
        && head
            .first()
            .is_some_and(|token| token.text == "pointer-event");
    let signature =
        matches!(head, [pure, function, ..] if pure.text == "pure" && function.text == "function");
    let declarations: Vec<_> = if signature {
        let end = names
            .iter()
            .position(|token| token.text == ")")
            .unwrap_or(names.len());
        names[..end]
            .split(|token| token.text == ",")
            .filter_map(|parameter| {
                let [name, colon, ty @ ..] = parameter else {
                    return None;
                };
                (colon.text == ":").then(|| (*name, read_type(ty)))
            })
            .collect()
    } else {
        names
            .iter()
            .filter(|token| name(token.text))
            .map(|token| (*token, None))
            .collect()
    };
    declarations
        .into_iter()
        .enumerate()
        .map(|(i, (t, parameter_ty))| Member {
            name: t.text.into(),
            ty: if repeat && i == 1 {
                Some(Type::Int)
            } else if parameter_ty.is_some() {
                parameter_ty
            } else {
                wheel_handler
                    .then_some(Type::WheelEvent)
                    .or_else(|| pointer_handler.then_some(Type::PointerEvent))
                    .or_else(|| key_handler.then_some(Type::KeyEvent))
            },
            visibility: "private".into(),
            kind: VARIABLE,
            range: t.range.clone(),
            file: None,
            type_file: None,
            parameters: vec![],
            parameter_labels: vec![],
            return_label: None,
            callback_alias: None,
        })
        .collect()
}

fn control_block(head: &[&Token<'_>]) -> bool {
    if !head
        .first()
        .is_some_and(|token| matches!(token.text, "if" | "else" | "for"))
    {
        return false;
    }
    let depth = head.iter().fold(0, |depth, token| match token.text {
        "[" | "(" => depth + 1,
        "]" | ")" => depth - 1,
        _ => depth,
    });
    depth == 0
}

fn repeat_model<'a>(head: &[&Token<'a>], role: Role<'_>) -> Vec<&'a str> {
    if head.first().is_none_or(|token| token.text != "for") {
        return vec![];
    }
    let start = head.iter().position(|t| t.text == "in");
    let end = if matches!(role, Role::StatementBlock) {
        Some(head.len())
    } else {
        head.iter().rposition(|t| t.text == ":")
    };
    start
        .zip(end)
        .filter(|(start, end)| start < end)
        .map_or_else(Vec::new, |(start, end)| {
            head[start + 1..end]
                .iter()
                .map(|token| token.text)
                .collect()
        })
}

#[derive(Default)]
struct CallbackSignature {
    parameters: Vec<Type>,
    labels: Vec<String>,
    result: Option<String>,
    alias: Option<String>,
}

fn callback_signature(tokens: &[&Token<'_>]) -> CallbackSignature {
    let mut signature = CallbackSignature::default();
    let [open, tail @ ..] = tokens else {
        return signature;
    };
    if open.text == "<=>" {
        let target = tail
            .iter()
            .take_while(|token| token.text != ";")
            .take_while(|token| name(token.text) || token.text == ".")
            .map(|token| token.text)
            .collect::<String>();
        signature.alias = (!target.is_empty()).then_some(target);
        return signature;
    }
    if open.text != "(" {
        return signature;
    }
    let end = tail
        .iter()
        .position(|token| matches!(token.text, ")" | ";" | "{" | "}"))
        .unwrap_or(tail.len());
    for parameter in tail[..end].split(|token| token.text == ",") {
        let (name, ty) = match parameter {
            [name, colon, ty @ ..] if colon.text == ":" => (Some(name.text), ty),
            _ => (None, parameter),
        };
        if let Some(kind) = read_type(ty) {
            let label = type_spelling(ty);
            signature
                .labels
                .push(name.map_or_else(|| label.clone(), |name| format!("{name}: {label}")));
            signature.parameters.push(kind);
        }
    }
    if let [close, arrow, ty @ ..] = &tail[end..]
        && close.text == ")"
        && arrow.text == "->"
        && read_type(ty).is_some()
    {
        signature.result = Some(type_spelling(ty));
    }
    signature
}

fn type_spelling(tokens: &[&Token<'_>]) -> String {
    match tokens {
        [open, ty, close, ..] if open.text == "[" && close.text == "]" => format!("[{}]", ty.text),
        [ty, ..] => ty.text.into(),
        [] => String::new(),
    }
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
    import_kind: Option<u32>,
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

    fn special_forms(&mut self) {
        for function in catalog::FUNCTIONS {
            if function.name.starts_with(self.prefix) {
                self.add(function.name, FUNCTION, &function.signature());
            }
        }
    }

    fn elements(&mut self, index: &Index<'a>, allow_nonvisual: impl Fn(&str) -> bool) {
        let prefix = self.prefix;
        for kind in catalog::ELEMENTS.split_ascii_whitespace().filter(|kind| {
            kind.starts_with(prefix)
                && is_builtin(kind)
                && xrack_ir::semantic::control_part_parents(kind).is_empty()
                && (!nonvisual_element(kind) || allow_nonvisual(kind))
        }) {
            self.add(kind, CLASS, "built-in element");
        }
        self.components(index);
    }

    fn components(&mut self, index: &Index<'a>) {
        self.import_kind = Some(CLASS);
        for scope in &index.scopes {
            if let Role::Component { name, base, .. } = scope.role
                && base != "global"
            {
                self.add(name, CLASS, "component");
            }
        }
        for &name in &index.imports {
            self.add(name, CLASS, "import");
        }
    }

    fn types(&mut self, index: &Index<'a>) {
        self.import_kind = Some(STRUCT);
        self.words(catalog::TYPES, CLASS, "XRack type");
        let dynamic = index.scopes.iter().any(|scope| {
            matches!(
                scope.role,
                Role::Element {
                    kind: "DynamicContainer",
                    ..
                } | Role::Component {
                    base: "DynamicContainer",
                    ..
                }
            )
        });
        if dynamic {
            self.add(
                "ExternalComponentHandle",
                STRUCT,
                "external component handle",
            );
        }
        for scope in &index.scopes {
            if dynamic
                && let Role::Component { name, base, .. } = scope.role
                && base != "global"
            {
                self.add(&format!("{name}Handle"), STRUCT, "component handle");
            }
            if let Role::Record { name, .. } = scope.role {
                self.add(name, STRUCT, "record");
            }
        }
        for &name in &index.imports {
            self.add(name, CLASS, "import");
        }
    }
}

#[cfg(test)]
pub(crate) fn complete(
    text: &str,
    line: usize,
    character: usize,
    workspace: Option<&Workspace<'_>>,
) -> Vec<Value> {
    complete_with_imports(text, line, character, workspace, |_, _| Vec::new())
}

fn suggest_position_child_argument<'a>(
    index: &Index<'a>,
    scope: usize,
    prior: &[&Token<'a>],
    items: &mut Items<'a>,
) -> bool {
    let Some((callee, argument)) = symbols::active_calls(prior.iter().copied()).next() else {
        return false;
    };
    if callee < 2
        || prior[callee - 1].text != "."
        || prior[callee].text != catalog::POSITION_CHILD_METHOD.name
    {
        return false;
    }
    let receiver = match prior[callee - 2].text {
        "root" => index.component(scope),
        "self" => index.element(scope),
        name => index.named_element(scope, name),
    };
    let Some(receiver) = receiver else {
        return false;
    };
    let kind = match index.scopes[receiver].role {
        Role::Element { kind, .. } | Role::Component { base: kind, .. } => kind,
        _ => return false,
    };
    if !content_viewport(kind)
        || index.members(receiver, false).iter().any(|member| {
            member.name == catalog::POSITION_CHILD_METHOD.name && member.kind == FUNCTION
        })
    {
        return false;
    }
    if argument != 0 {
        items.add("ScrollAlignment", ENUM, "expected enum");
        return false;
    }
    for (candidate, data) in index.scopes.iter().enumerate() {
        let Role::Element { id: Some(id), .. } = data.role else {
            continue;
        };
        if candidate == receiver
            || !index
                .ancestors(candidate)
                .any(|ancestor| ancestor == receiver)
        {
            continue;
        }
        let drawable = index
            .ancestors(candidate)
            .take_while(|&ancestor| ancestor != receiver)
            .all(|ancestor| {
                let entry = &index.scopes[ancestor];
                entry.repeat_model.is_empty()
                    && match entry.role {
                        Role::Element { kind, .. } => {
                            xrack_ir::semantic::visual_state_target(kind)
                                && xrack_ir::semantic::control_part_parents(kind).is_empty()
                                && !matches!(
                                    kind,
                                    "Tooltip"
                                        | "Menu"
                                        | "PopupWindow"
                                        | "Dialog"
                                        | "Drawer"
                                        | "Shortcut"
                                )
                        }
                        _ => true,
                    }
            });
        if drawable {
            items.add(id, VARIABLE, "viewport descendant");
        }
    }
    true
}

fn shortcut_values(
    text: &str,
    source: &Source<'_>,
    tokens: &[Token<'_>],
    code: &[&Token<'_>],
    cursor: usize,
) -> Option<Vec<Value>> {
    let literal = tokens.iter().find(|token| token.blocks_completion(cursor));
    if literal.is_some_and(|token| !token.text.starts_with('"')) {
        return None;
    }
    let start = literal.map_or(cursor, |token| token.range.start);
    let before = &code[..code.partition_point(|token| token.range.end <= start)];
    let statement = before
        .iter()
        .rposition(|token| matches!(token.text, "{" | "}" | ";"))
        .map_or(before, |position| &before[position + 1..]);
    let name = match statement {
        [binding, separator]
            if separator.text == ":" && matches!(binding.text, "shortcut" | "sequence") =>
        {
            binding.text
        }
        [binding, separator, list, rest @ ..]
            if binding.text == "sequences"
                && separator.text == ":"
                && list.text == "["
                && rest.iter().all(|token| token.text == ",") =>
        {
            binding.text
        }
        _ => return None,
    };
    let index = Index::new(code, text.len());
    let owner = match index.scopes[index.scope_at(start)].role {
        Role::Element { kind, .. } | Role::Component { base: kind, .. } => kind,
        _ => return None,
    };
    if !matches!(
        (owner, name),
        ("Shortcut", "sequence" | "sequences") | ("MenuItem" | "Action", "shortcut")
    ) {
        return None;
    }
    let (edit_start, edit_end) = literal.map_or((cursor, cursor), |token| {
        (
            token.range.start + 1,
            token.range.end - usize::from(token.quoted()),
        )
    });
    let prefix = &text[edit_start..cursor];
    let range = source.range(edit_start, edit_end);
    Some(
        [
            ("Primary+[", "Modified left bracket"),
            ("Primary+]", "Modified right bracket"),
            ("Ctrl+Tab", "Switch tab with a modified Tab key"),
            (
                "Ctrl+Shift+Tab",
                "Switch tab backward with a modified Tab key",
            ),
            ("MouseBack", "Mouse back button"),
            ("MouseForward", "Mouse forward button"),
        ]
        .into_iter()
        .filter(|(value, _)| value.starts_with(prefix))
        .map(|(value, detail)| {
            let new_text = if literal.is_some() {
                value.to_owned()
            } else {
                format!("\"{value}\"")
            };
            json!({"label": value, "kind": ENUM_MEMBER, "detail": detail,
            "textEdit": {"range": range, "newText": new_text}})
        })
        .collect(),
    )
}

fn mesh_options(
    text: &str,
    source: &Source<'_>,
    tokens: &[Token<'_>],
    cursor: usize,
) -> Option<Vec<Value>> {
    for (index, window) in tokens.windows(4).enumerate().rev() {
        let [url, open, path, delimiter] = window else {
            continue;
        };
        if url.text != "@mesh-url"
            || open.text != "("
            || !path.quoted()
            || delimiter.text != ","
            || delimiter.range.end > cursor
        {
            continue;
        }
        let before = &tokens[index + 4..tokens.partition_point(|token| token.range.start < cursor)];
        let mut used = BTreeSet::new();
        let mut start = delimiter.range.end;
        let mut completed = true;
        let mut pairs = before.chunks_exact(2);
        for pair in &mut pairs {
            if !name(pair[0].text) || pair[1].text != "," {
                completed = false;
                break;
            }
            used.insert(pair[0].text);
            start = pair[1].range.end;
        }
        if !completed
            || pairs
                .remainder()
                .first()
                .is_some_and(|token| !name(token.text))
        {
            continue;
        }
        let prefix = text.get(start..cursor)?.trim_start();
        let start = cursor - prefix.len();
        let end = tokens
            .iter()
            .find(|token| token.range.start == start && name(token.text))
            .map_or(cursor, |token| token.range.end);
        return Some(
            builtins::MESH_URL_OPTIONS
                .iter()
                .filter(|(option, _)| option.starts_with(prefix) && !used.contains(option))
                .map(|(option, description)| {
                    json!({
                        "label": option, "kind": KEYWORD, "detail": description,
                        "textEdit": { "range": source.range(start, end), "newText": option }
                    })
                })
                .collect(),
        );
    }
    None
}

pub(crate) fn complete_with_imports(
    text: &str,
    line: usize,
    character: usize,
    workspace: Option<&Workspace<'_>>,
    exports: impl FnOnce(&str, u32) -> Vec<(String, String)>,
) -> Vec<Value> {
    let source = Source::new(text);
    let Some(cursor) = source.offset_at(line, character) else {
        return vec![];
    };
    let tokens = editor_tokens::scan(text);
    if let Some(items) = mesh_options(text, &source, &tokens, cursor) {
        return items;
    }
    if let Some(kinds) = imports::scene_string_options(&source, &tokens, cursor, text.len()) {
        return kinds;
    }
    if let Some(items) = imports::scene_effect_buffer_values(&source, &tokens, cursor) {
        return items;
    }
    if let Some(items) = imports::scene_item_texture_values(&source, &tokens, cursor, text.len()) {
        return items;
    }
    if let Some(scopes) = imports::scene_light_scope_values(&source, &tokens, cursor) {
        return scopes;
    }
    if let Some(references) = imports::scene_instance_reference_values(&source, &tokens, cursor) {
        return references;
    }
    if let Some(paths) = workspace
        .and_then(|workspace| imports::scene_asset_paths(&source, &tokens, cursor, workspace))
    {
        return paths;
    }
    if let Some(slots) = imports::scene_material_slot_values(&source, &tokens, cursor, workspace) {
        return slots;
    }
    if let Some(clips) = workspace.and_then(|workspace| {
        imports::scene_asset_selector_values(&source, &tokens, cursor, workspace)
    }) {
        return clips;
    }
    if let Some(cameras) = imports::scene_camera_values(&source, &tokens, cursor, workspace) {
        return cameras;
    }
    let code: Vec<_> = tokens.iter().filter(|t| t.code()).collect();
    if let Some(values) = shortcut_values(text, &source, &tokens, &code, cursor) {
        return values;
    }
    if tokens.iter().any(|t| t.blocks_completion(cursor)) {
        return vec![];
    }
    let prefix = code.iter().find(|t| {
        (name(t.text) || t.text.starts_with('@')) && t.range.start < cursor && cursor <= t.range.end
    });
    let start = prefix.map_or(cursor, |t| t.range.start);
    let end = prefix.map_or(cursor, |t| t.range.end);
    let mut items = Items {
        prefix: &text[start..cursor],
        range: source.range(start, end),
        seen: BTreeSet::new(),
        values: vec![],
        import_kind: None,
    };
    let mut index = Index::new(&code, text.len());
    let scope = index.scope_at(cursor);
    if matches!(index.scopes[scope].role, Role::Import)
        && let Some(symbols) =
            workspace.and_then(|workspace| workspace.import_symbols(&tokens, cursor))
    {
        for (symbol, kind) in symbols {
            let detail = if kind == STRUCT {
                "exported record"
            } else {
                "exported component"
            };
            items.add(&symbol, kind, detail);
        }
        return items.values;
    }
    let prior = &code[..code.partition_point(|t| t.range.end <= start)];
    if let [.., type_name, separator] = prior
        && separator.text == "::"
        && let Some(declaration) = code.windows(3).position(|tokens| {
            tokens[0].text == "enum" && tokens[1].text == type_name.text && tokens[2].text == "{"
        })
    {
        let mut depth = 0_usize;
        let mut next_variant = true;
        for token in &code[declaration + 3..] {
            if token.text == "}" && depth == 0 {
                break;
            }
            match token.text {
                "(" | "{" => depth += 1,
                ")" | "}" => depth = depth.saturating_sub(1),
                "," if depth == 0 => next_variant = true,
                variant if depth == 0 && next_variant && name(variant) => {
                    items.add(variant, ENUM_MEMBER, type_name.text);
                    next_variant = false;
                }
                _ => {}
            }
        }
        return items.values;
    }
    if matches!(
        index.scopes[scope].role,
        Role::Handler | Role::StatementBlock
    ) {
        let end = prior
            .iter()
            .rposition(|token| token.text == ";")
            .unwrap_or(0);
        for declaration in prior[..end].windows(3) {
            if declaration[0].text == "let"
                && declaration[2].text == "="
                && index
                    .ancestors(scope)
                    .any(|ancestor| ancestor == index.scope_at(declaration[0].range.start))
            {
                items.add(declaration[1].text, VARIABLE, "immutable local binding");
            }
        }
    }
    if scope != 0 {
        if let Some(workspace) = workspace
            && !index.imports.is_empty()
        {
            (index.imported_members, index.imported_globals) = workspace.imported_members(&tokens);
        }
        index.infer_repeat_items(false);
        index.infer_choice_roles(&tokens);
        index.infer_handler_parameters(&code);
        index.infer_repeat_items(true);
    }
    let head = &prior[prior
        .iter()
        .rposition(|t| matches!(t.text, "{" | "}" | ";"))
        .map_or(0, |i| i + 1)..];
    let role = index.scopes[scope].role;
    let head = member_head(text, &tokens, head, start, role);
    if matches!(head, [keyword] if keyword.text == "slot")
        && let Role::Element { kind, .. } = role
        && let Some(declaration) = index.declared_component(kind)
    {
        let declaration = &index.scopes[declaration];
        for marker in code.windows(3).filter(|tokens| {
            tokens[0].text == "@slot"
                && tokens[1].text == "("
                && tokens[0].range.start >= declaration.body
                && tokens[2].range.end <= declaration.close
        }) {
            items.add(marker[2].text, PROPERTY, "named component slot");
        }
        return items.values;
    }
    if let Some(close) = prior.last().filter(|token| token.text == "}")
        && index.scopes.iter().any(|branch| {
            matches!(branch.role, Role::StatementBlock)
                && branch.parent == scope
                && branch.close == close.range.start
                && matches!(&code[branch.header.clone()], [first, rest @ ..]
                    if first.text == "if" || first.text == "else"
                        && rest.first().is_some_and(|token| token.text == "if"))
        })
    {
        items.add("else", KEYWORD, "XRack conditional branch");
    }
    if suggest_position_child_argument(&index, scope, prior, &mut items) {
        return items.values;
    }
    if !matches!(prior.last().map(|token| token.text), Some(".")) {
        let expected = symbols::active_calls(prior.iter().copied())
            .find_map(|(callee, active)| {
                // Parentheses around an argument do not introduce a call.
                if !name(prior[callee].text) {
                    return None;
                }
                Some(
                    index
                        .builtin_receiver(scope, prior, callee)
                        .and_then(|receiver| builtins::function(receiver, prior[callee].text))
                        .and_then(|function| function.parameters.get(active))
                        .and_then(|parameter| parameter.expected),
                )
            })
            .flatten();
        if let Some(ty) = expected.filter(|ty| !ty.enum_variants().is_empty())
            && let Ok(Value::String(name)) = serde_json::to_value(ty)
        {
            items.add(&name, ENUM, "expected enum");
        }
    }
    if scope == 0
        && prior.last().is_some_and(|token| {
            token.text == "}"
                && index.scopes.iter().any(|scope| {
                    matches!(scope.role, Role::Import) && scope.close == token.range.start
                })
        })
    {
        items.add("from", KEYWORD, "import path");
    } else {
        suggest(&index, scope, head, &mut items);
    }
    if let Some(kind) = items
        .import_kind
        .filter(|_| !items.prefix.is_empty() && workspace.is_some())
    {
        let occupied = import_names(&index);
        for (label, path) in exports(items.prefix, kind) {
            if items.seen.contains(&label)
                || occupied.contains(&xrack_ir::semantic::rust_name(&label))
            {
                continue;
            }
            let edit = import_edit(&source, text, &tokens, &label, &path);
            items.values.push(json!({
                "label": label, "kind": kind, "detail": format!("Auto import from {path}"),
                "textEdit": { "range": items.range, "newText": label },
                "additionalTextEdits": [edit],
            }));
        }
    }
    if matches!(role, Role::Component { .. } | Role::Element { .. })
        && let Some(colon) = full_width_assignment(head)
    {
        let edit = json!({
            "range": source.range(colon.range.start, colon.range.end),
            "newText": ":",
        });
        for item in &mut items.values {
            if let Some(edits) = item["additionalTextEdits"].as_array_mut() {
                edits.push(edit.clone());
            } else {
                item["additionalTextEdits"] = json!([edit]);
            }
        }
    }
    items.values
}

fn full_width_assignment<'a, 't>(head: &[&'t Token<'a>]) -> Option<&'t Token<'a>> {
    match head {
        [id, colon, equals]
            if name(id.text)
                && colon.text == "："
                && equals.text == "="
                && colon.range.end == equals.range.start =>
        {
            Some(colon)
        }
        _ => None,
    }
}

fn member_head<'a, 't>(
    text: &str,
    tokens: &[Token<'a>],
    head: &'t [&'t Token<'a>],
    start: usize,
    role: Role<'a>,
) -> &'t [&'t Token<'a>] {
    if !matches!(role, Role::Component { .. } | Role::Element { .. }) {
        return head;
    }
    let line = text[..start].rfind(['\n', '\r']).map_or(0, |end| end + 1);
    let (previous, current) = head.split_at(head.partition_point(|token| token.range.end <= line));
    if !current.is_empty()
        && !matches!(current, [id, assign] if name(id.text) && assign.text == ":=")
        && full_width_assignment(current).is_none()
    {
        return head;
    }
    let Some(binding) = previous
        .iter()
        .position(|token| matches!(token.text, ":" | "<=>"))
    else {
        return head;
    };
    // A complete binding on the preceding line may be missing its semicolon.
    // Keep the original expression context for continuations and open calls.
    let last = tokens[..tokens.partition_point(|token| token.range.end <= line)]
        .iter()
        .rfind(|token| token.code() || token.quoted());
    if last.is_none_or(|token| {
        token.range.start < previous[binding].range.end
            || !(token.quoted()
                || name(token.text)
                || token
                    .text
                    .starts_with(|c: char| c.is_ascii_digit() || c == '#')
                || matches!(token.text, ")" | "]"))
    }) {
        return head;
    }
    let mut open = Vec::new();
    for token in &previous[binding + 1..] {
        match token.text {
            "(" => open.push(")"),
            "[" => open.push("]"),
            "?" => open.push(":"),
            ")" | "]" | ":" => {
                if open.pop() != Some(token.text) {
                    return head;
                }
            }
            _ => {}
        }
    }
    if open.is_empty() { current } else { head }
}

fn import_names(index: &Index<'_>) -> BTreeSet<String> {
    index
        .imports
        .iter()
        .copied()
        .chain(index.scopes.iter().filter_map(|scope| match scope.role {
            Role::Component { name, .. } | Role::Record { name, .. } => Some(name),
            _ => None,
        }))
        .map(xrack_ir::semantic::rust_name)
        .collect()
}

fn import_edit(
    source: &Source<'_>,
    text: &str,
    tokens: &[Token<'_>],
    name: &str,
    path: &str,
) -> Value {
    let first = tokens
        .iter()
        .find(|token| token.code())
        .map_or(0, |token| token.range.start);
    let insertion = text[..first].rfind('\n').map_or_else(
        || usize::from(text.starts_with('\u{feff}')) * '\u{feff}'.len_utf8(),
        |offset| offset + 1,
    );
    let line_break = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let path = Value::String(path.into()).to_string();
    json!({"range": source.range(insertion, insertion), "newText": format!("import {{ {name} }} from {path};{line_break}")})
}

pub(crate) fn import_fixes(
    uri: &str,
    text: &str,
    version: i64,
    diagnostics: &[Value],
    params: &Value,
    mut exports: impl FnMut(&str, u32) -> Vec<(String, String)>,
) -> Vec<Value> {
    if params["context"]["only"]
        .as_array()
        .is_some_and(|kinds| !kinds.iter().any(|kind| kind == "quickfix"))
    {
        return vec![];
    }
    let Some(requested) = params["context"]["diagnostics"].as_array() else {
        return vec![];
    };
    let source = Source::new(text);
    let tokens = editor_tokens::scan(text);
    let code: Vec<_> = tokens.iter().filter(|token| token.code()).collect();
    let occupied = import_names(&Index::new(&code, text.len()));
    let mut fixes = Vec::new();
    let mut seen = BTreeSet::new();
    for diagnostic in diagnostics {
        let Some(name) = diagnostic["data"]["missingComponent"].as_str() else {
            continue;
        };
        if occupied.contains(&xrack_ir::semantic::rust_name(name))
            || !requested.iter().any(|item| {
                item["range"] == diagnostic["range"]
                    && item["message"] == diagnostic["message"]
                    && item["code"] == diagnostic["code"]
                    && item["source"] == "xrack"
            })
        {
            continue;
        }
        for (candidate, path) in exports(name, CLASS) {
            if candidate != name || !seen.insert((candidate, path.clone())) {
                continue;
            }
            let edit = import_edit(&source, text, &tokens, name, &path);
            fixes.push(json!({
                "title": format!("Import {name} from {path}"),
                "kind": "quickfix", "diagnostics": [diagnostic],
                "edit": { "documentChanges": [{ "textDocument": {"uri": uri, "version": version}, "edits": [edit] }] },
            }));
        }
    }
    fixes
}

fn suggest<'a>(index: &Index<'a>, scope: usize, head: &[&Token<'a>], items: &mut Items<'a>) {
    let role = index.scopes[scope].role;
    let radio_group = matches!(
        role,
        Role::Component {
            base: "RadioGroup",
            ..
        } | Role::Element {
            kind: "RadioGroup",
            ..
        }
    );
    if items.prefix.starts_with('@') {
        if head.is_empty()
            && matches!(role, Role::Component { .. } | Role::Element { .. })
            && !index
                .ancestors(scope)
                .any(|ancestor| !index.scopes[ancestor].repeat_model.is_empty())
        {
            items.add("@children", KEYWORD, catalog::CHILDREN_DOC);
        } else {
            items.special_forms();
        }
        if head.is_empty() && matches!(role, Role::Component { .. } | Role::Element { .. }) {
            items.add("@slot", KEYWORD, "named component slot");
        }
        return;
    }
    let head = if matches!(role, Role::Record { .. } | Role::RecordValue(_)) {
        &head[head
            .iter()
            .rposition(|t| t.text == ",")
            .map_or(0, |i| i + 1)..]
    } else {
        head
    };
    let first = head.first().map_or("", |t| t.text);
    let last = head.last().map_or("", |t| t.text);
    if scope == 0 {
        let declaration = match head {
            [export, rest @ ..] if export.text == "export" => rest,
            _ => head,
        };
        match declaration {
            [component, id] if component.text == "component" && name(id.text) => {
                items.add("inherits", KEYWORD, "base component");
            }
            [component, id, inherits]
                if component.text == "component"
                    && name(id.text)
                    && inherits.text == "inherits" =>
            {
                items.elements(index, |_| false);
            }
            [] => items.words(
                if first == "export" {
                    "component global struct enum"
                } else {
                    "import translations export component global struct enum"
                },
                KEYWORD,
                "XRack declaration",
            ),
            _ => {}
        }
        return;
    }
    if last == "." {
        qualified(index, scope, head, items);
        return;
    }
    let full_width = matches!(role, Role::Component { .. } | Role::Element { .. })
        && full_width_assignment(head).is_some();
    if last == ":=" || full_width || (matches!(first, "if" | "for") && last == ":") {
        if radio_group {
            items.add("RadioButton", CLASS, "RadioGroup option");
            return;
        }
        let named =
            matches!(head, [id, assign] if name(id.text) && assign.text == ":=") || full_width;
        let root = named && matches!(role, Role::Component { base, .. } if base != "global");
        if root {
            items.add(
                "SoundEffect",
                CLASS,
                "nonvisual object · preloads a local WAV",
            );
        }
        // An imported base can itself inherit a Window.
        let window = matches!(role, Role::Component { base, .. }
            if !is_builtin(base) || xrack_ir::semantic::is_window(base));
        let action_group = matches!(
            role,
            Role::Element {
                kind: "ActionGroup",
                ..
            }
        );
        items.elements(index, |kind| match kind {
            "SecondaryWindow" => root && window,
            "Action" => root || named && action_group,
            _ => root,
        });
        return;
    }
    if last == "->"
        || matches!(role, Role::Record { .. }) && last == ":"
        || head.iter().any(|t| t.text == "property") && matches!(last, "<" | "[")
        || head
            .iter()
            .any(|t| matches!(t.text, "callback" | "function"))
            && last == ":"
    {
        items.types(index);
        return;
    }
    let kind = match role {
        Role::Component { base, .. } => base,
        Role::Element { kind, .. } => kind,
        Role::States if !head.iter().any(|t| t.text == "when") => {
            items.add("when", KEYWORD, "state condition");
            return;
        }
        Role::State | Role::StateTransition if !head.iter().any(|token| token.text == ":") => {
            let timing = matches!(role, Role::StateTransition);
            if timing && first != "animate" {
                items.add("animate", KEYWORD, "state transition");
                return;
            }
            if timing {
                items.add("*", PROPERTY, "all animatable state properties");
            } else {
                items.words("in out in-out", KEYWORD, "transition direction");
            }
            if let Some(element) = index.element(scope) {
                add_state_properties(index, element, timing, items);
            }
            if let Some(component) = index.component(scope) {
                for (i, candidate) in index.scopes.iter().enumerate() {
                    if let Role::Element { id: Some(id), .. } = candidate.role
                        && index.component(i) == Some(component)
                    {
                        items.add(id, VARIABLE, "state target");
                    }
                }
            }
            return;
        }
        Role::Animation if first == "enabled" && head.iter().any(|token| token.text == ":") => "",
        Role::Animation => {
            if first == "easing" && head.iter().any(|token| token.text == ":") {
                items.words(
                    "linear ease ease-in ease-out ease-in-out cubic-bezier",
                    ENUM_MEMBER,
                    "easing curve",
                );
            } else {
                items.words(
                    "duration delay easing enabled",
                    PROPERTY,
                    "animation option",
                );
            }
            return;
        }
        _ => "",
    };
    if (builtins::button_group_member(kind) || kind == "Action")
        && first == "group"
        && head.iter().any(|token| token.text == ":")
    {
        let group_kind = if kind == "Action" {
            "ActionGroup"
        } else {
            "ButtonGroup"
        };
        if let Some(component) = index.component(scope) {
            for (i, candidate) in index.scopes.iter().enumerate() {
                if let Role::Element { kind, id: Some(id) } = candidate.role
                    && kind == group_kind
                    && index.component(i) == Some(component)
                {
                    items.add(id, VARIABLE, group_kind);
                }
            }
        }
        return;
    }
    if kind == "SplitView" && first == "fill-pane" && last == ":" {
        for candidate in &index.scopes {
            if candidate.parent == scope
                && let Role::Element { id: Some(id), kind } = candidate.role
                && !matches!(
                    kind,
                    "Shortcut" | "Tooltip" | "Dialog" | "Drawer" | "PopupWindow" | "Menu"
                )
            {
                items.add(id, VARIABLE, "direct pane");
            }
        }
        return;
    }
    let in_handler = matches!(role, Role::Handler | Role::StatementBlock);
    let expression = in_handler
        || matches!(role, Role::States)
        || matches!(first, "if" | "for" | "..")
        || head
            .iter()
            .any(|t| matches!(t.text, ":" | "<=>" | "=" | "+=" | "-="));
    if expression {
        if in_handler && head.is_empty() {
            items.words("if for let", KEYWORD, "XRack statement");
        }
        items.add("match", KEYWORD, "exhaustive enum expression");
        if let Some((ty, _)) = index.combo_option_data(scope) {
            let detail =
                ty.map_or_else(|| "ComboBox model item".into(), |ty| ty.name().into_owned());
            items.add("option-data", VARIABLE, &detail);
        }
        if let Some(inputs) = index.template_inputs(scope) {
            for (name, ty) in inputs {
                items.add(name, VARIABLE, &ty.name());
            }
        }
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
        items.special_forms();
        for scope in &index.scopes {
            if let Role::Component {
                name,
                base: "global",
                ..
            } = scope.role
            {
                items.add(name, VARIABLE, "shared global");
            }
        }
        for namespace in builtins::NAMESPACES {
            let statements_allowed = in_handler && head.is_empty();
            let has_member = builtins::FUNCTIONS.iter().any(|function| {
                function.namespace == *namespace
                    && (statements_allowed || function.result.is_some())
            }) || builtins::CONSTANTS
                .iter()
                .any(|constant| constant.namespace == *namespace);
            if has_member && index.builtin_namespace_visible(scope, namespace) {
                items.add(namespace, CLASS, "built-in namespace");
            }
        }
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
        let prefix = items.prefix;
        for property in catalog::PROPERTIES
            .split_ascii_whitespace()
            .filter(|property| property.starts_with(prefix))
        {
            if xrack_ir::semantic::media_object_read_type(kind, property).is_some()
                || (builtins::text_submission_editor(kind)
                    && matches!(property, "acceptable-input" | "input-validity"))
                || (animation && builtins::text_submission_rule(kind, property))
                || matches!(property, "content-x" | "content-y")
                    && (animation || !viewport_offset_property(kind, property))
                || focus_property(kind, property)
                || xrack_ir::semantic::scroll_view_bar_metric(kind, property)
                || xrack_ir::semantic::scroll_visual_property(kind, property)
                || xrack_ir::semantic::scroll_state_property(kind, property)
                || xrack_ir::semantic::flick_motion_property(kind, property).is_some()
                || property == "current-index" && index.value_navigation.contains(&scope)
                || property == "count" && builtins::navigation_group(kind)
                || property == "active-index" && kind == "StackLayout"
                || xrack_ir::semantic::range_position_property(kind, property)
                || xrack_ir::semantic::combo_popup_property(kind, property).is_some()
                || view_count_property(kind, property)
                || xrack_ir::semantic::list_selection_property(kind, property)
                || text_command_property(kind, property)
                || hover_property(kind, property).is_some()
                || touch_area_property(kind, property).is_some()
                || kind == "ButtonGroup" && property == "check-state"
                || xrack_ir::semantic::is_window(kind) && window_status_property(property)
                || geometry_property(property).is_some()
                || table_sort_property(kind, property).is_some()
                || table_selection_property(kind, property).is_some()
                || table_history_property(kind, property).is_some()
                || (kind == "Loader" && property == "status")
                || (kind == "StackView"
                    && matches!(property, "current-page" | "depth" | "can-go-back"))
                || (kind == "StackPage" && matches!(property, "visible" | "active" | "stack-index"))
                || matches!(kind, "Tumbler" | "PathView") && property == "current-text"
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
                let detail = match (kind, property) {
                    ("Tumbler" | "PathView" | "ComboBox", "current-value") => {
                        index.choice_value_type(scope).map_or_else(
                            || "type determined by value-role".into(),
                            |ty| ty.name().into_owned(),
                        )
                    }
                    ("Tumbler" | "PathView", "model") => "[string] or [Record]".into(),
                    _ => ty.name().into_owned(),
                };
                items.add(property, PROPERTY, &detail);
            }
        }
        if kind == "Loader" && !animation {
            items.add("loaded", FUNCTION, "event handler");
            items.components(index);
            return;
        }
        if matches!(kind, "SoundEffect" | "ComboPopup") {
            return;
        }
        if matches!(kind, "Timer" | "FrameAnimation") {
            if !animation {
                let signature = if kind == "FrameAnimation" {
                    "triggered(frame-time: float, elapsed-time: float, current-frame: int)"
                } else {
                    "event handler"
                };
                items.add("triggered", FUNCTION, signature);
            }
            return;
        }
        if !animation {
            let instance = matches!(role, Role::Element { .. }) && !is_builtin(kind);
            if matches!(
                index.scopes[index.scopes[scope].parent].role,
                Role::Element {
                    kind: "TableRow",
                    ..
                }
            ) {
                items.add("clipboard-text", PROPERTY, "string");
            }
            if instance
                && index.ancestors(scope).any(|ancestor| {
                    matches!(
                        index.scopes[ancestor].role,
                        Role::Element {
                            kind: "TableRow",
                            ..
                        }
                    )
                })
            {
                items.add(
                    "clipboard-target",
                    PROPERTY,
                    "string · row-linked in-out property",
                );
                items.add(
                    "clipboard-paste",
                    FUNCTION,
                    "(string) → { accepted, value }",
                );
            }
            if matches!(
                kind,
                "Button" | "ToolButton" | "RoundButton" | "MenuItem" | "ItemDelegate"
            ) {
                items.add("action", PROPERTY, "Action id");
            }
            if kind == "Action" {
                if !matches!(
                    index.scopes[index.scopes[scope].parent].role,
                    Role::Element {
                        kind: "ActionGroup",
                        ..
                    }
                ) {
                    items.add("group", PROPERTY, "ActionGroup id");
                }
                items.add("value", PROPERTY, "unique string in the ActionGroup");
            }
            if matches!(kind, "ScrollBar" | "ScrollIndicator") {
                items.add("target", PROPERTY, "Viewport id");
            }
            if kind == "SplitView" {
                items.add("fill-pane", PROPERTY, "direct pane id");
                items.add("SplitHandle", CLASS, "split handle template");
            }
            for member in index.members(scope, false) {
                if instance
                    && member.visibility == "in-out"
                    && let Some(ty) = &member.ty
                {
                    items.add(
                        &member.name,
                        member.kind,
                        &format!("{} · supports two-way binding", ty.name()),
                    );
                } else if member.kind == FUNCTION
                    || matches!(member.visibility.as_str(), "in" | "in-out")
                {
                    items.member(member);
                }
            }
            if instance {
                items.add("states", KEYWORD, "conditional component inputs and layout");
                items.elements(index, |_| false);
                return;
            }
            for part in catalog::ELEMENTS.split_ascii_whitespace() {
                if xrack_ir::semantic::control_part_parents(part)
                    .contains(&xrack_ir::semantic::delegate_base(kind).unwrap_or(kind))
                {
                    let detail = match part {
                        "EmptyState" => "content shown when the view has no rows",
                        "SwitchThumb" => "switch thumb laid out at the current drag position",
                        "ComboIndicator" => "dropdown artwork at the trailing edge of a ComboBox",
                        "ComboContent" => {
                            "replacement content for the selected value of a ComboBox"
                        }
                        "ComboOption" => "visual template for a visible option in a ComboBox",
                        "TumblerOption" => "visual template for a visible row in a Tumbler",
                        "PathViewOption" => "visual template for an item on a PathView",
                        "ComboPopup" => {
                            "size, appearance and scrolling indicator of a ComboBox popup"
                        }
                        "ProgressTrack" => "artwork laid out over the whole progress track",
                        "ProgressFill" => "artwork resized and positioned by the current progress",
                        "ScrollTrack" => "artwork covering the resolved scrollbar track",
                        "ScrollThumb" => "artwork following the actual scrollbar thumb",
                        "TreeDisclosure" => "branch disclosure artwork driven by the TreeView",
                        "SectionHeader" => "visual template for each section heading in a list",
                        "SelectionStartHandle" => {
                            "artwork at the leading corner of a table selection"
                        }
                        "SelectionEndHandle" => {
                            "artwork at the trailing corner of a table selection"
                        }
                        _ => "replacement control part",
                    };
                    items.add(part, CLASS, detail);
                }
            }
            items.add("@children", KEYWORD, catalog::CHILDREN_DOC);
            items.add("@slot", KEYWORD, "named component slot");
            items.add("slot", KEYWORD, "supply content to a named slot");
            items.add("match", KEYWORD, "enum-based component branches");
            items.add(
                "pure function",
                KEYWORD,
                "component function with no side effects",
            );
            for event in catalog::EVENTS
                .split_ascii_whitespace()
                .filter(|event| builtin_event(kind, event))
            {
                if event == "value-selected" {
                    let detail = index.choice_value_type(scope).map_or_else(
                        || "event handler · value-role".into(),
                        |ty| format!("event handler ({})", ty.name()),
                    );
                    items.add(event, FUNCTION, &detail);
                } else if event == "editing-finished" {
                    items.add(event, FUNCTION, "event handler (string, bool)");
                } else if matches!(event, "files-failed" | "link-activated") {
                    items.add(event, FUNCTION, "event handler (string)");
                } else {
                    items.add(event, FUNCTION, "event handler");
                }
            }
            if radio_group {
                items.add("RadioButton", CLASS, "RadioGroup option");
            } else {
                items.elements(index, |_| false);
            }
            items.words(
                "property callback pure in out in-out private if for animate states",
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
    (name == "current-value")
        .then(|| index.choice_value_type(scope))
        .flatten()
        .or_else(|| builtin_property(kind, name))
        .or_else(|| members.into_iter().find(|m| m.name == name)?.ty.clone())
}

fn add_state_properties(index: &Index<'_>, owner: usize, timing: bool, items: &mut Items<'_>) {
    let role = index.scopes[owner].role;
    let kind = match role {
        Role::Element { kind, .. } | Role::Component { base: kind, .. } => kind,
        _ => return,
    };
    let instance = matches!(role, Role::Element { .. }) && !is_builtin(kind);
    for property in catalog::PROPERTIES.split_ascii_whitespace() {
        let ty = if instance {
            layout_item_property(property)
        } else {
            state_property(kind, property)
        };
        if let Some(ty) = ty
            && (!timing
                || matches!(
                    ty,
                    Type::Int | Type::Float | Type::Length | Type::Color | Type::Brush
                ))
        {
            items.add(property, PROPERTY, &ty.name());
        }
    }
    if instance && !timing {
        for member in index.members(owner, true) {
            if matches!(member.visibility.as_str(), "in" | "in-out") && member.ty.is_some() {
                items.member(member);
            }
        }
    }
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
    if let Some(global) = index.declared_component(first)
        && matches!(
            index.scopes[global].role,
            Role::Component { base: "global", .. }
        )
        && receiver.is_empty()
    {
        for member in index.members(global, true) {
            items.member(member);
        }
        return;
    }
    if receiver.is_empty()
        && index.imported_globals.contains(first)
        && let Some(members) = index.imported_members.get(first)
    {
        for member in members {
            items.member(member);
        }
        return;
    }
    if receiver.is_empty()
        && builtins::NAMESPACES.contains(&first)
        && index.builtin_namespace_visible(scope, first)
    {
        let statement = matches!(index.scopes[scope].role, Role::Handler) && head.len() == 2;
        for function in builtins::FUNCTIONS
            .iter()
            .filter(|function| function.namespace == first)
        {
            if function.result.is_some() || statement {
                items.add(function.member, FUNCTION, &function.signature());
            }
        }
        for constant in builtins::CONSTANTS
            .iter()
            .filter(|constant| constant.namespace == first)
        {
            items.add(constant.member, FIELD, "float constant");
        }
        return;
    }
    if receiver.is_empty()
        && let Some(ty) = Type::parse(first)
    {
        for &variant in ty.enum_variants() {
            items.add(variant, ENUM_MEMBER, first);
        }
    }
    if receiver.is_empty()
        && index.ancestors(scope).any(|scope| {
            index
                .members(scope, false)
                .into_iter()
                .any(|member| member.name == first && member.ty == Some(Type::Image))
        })
    {
        items.words("width height", FIELD, "read-only int");
        return;
    }
    if let Some(ty) = index
        .ancestors(scope)
        .find_map(|scope| {
            index
                .members(scope, false)
                .into_iter()
                .find(|member| member.name == first)?
                .ty
                .clone()
        })
        .filter(|ty| {
            ty.builtin_fields()
                .iter()
                .any(|field| ty.input_field(field).is_some())
        })
    {
        complete_builtin_fields(ty, receiver.iter().rev().copied(), items);
        return;
    }
    let owner = match first {
        "root" => index.component(scope),
        "self" => index.element(scope),
        id => index.named_element(scope, id),
    };
    let role = index.scopes[scope].role;
    if receiver.is_empty()
        && let Some(owner) = owner
        && matches!(role, Role::State | Role::StateTransition)
        && !head.iter().any(|token| token.text == ":")
    {
        add_state_properties(index, owner, matches!(role, Role::StateTransition), items);
        return;
    }
    if receiver.is_empty()
        && let Some(owner) = owner
        && head.iter().any(|token| token.text == "<=>")
        && head.iter().any(|token| token.text == "callback")
    {
        let kind = match index.scopes[owner].role {
            Role::Element { kind, .. } | Role::Component { base: kind, .. } => kind,
            _ => "",
        };
        for event in catalog::EVENTS
            .split_ascii_whitespace()
            .filter(|event| builtin_event(kind, event))
        {
            items.add(event, FUNCTION, "event");
        }
    }
    if receiver.is_empty()
        && let Some(owner) = owner
        && head.iter().any(|token| token.text == "<=>")
        && head.iter().any(|token| token.text == "property")
    {
        let kind = match index.scopes[owner].role {
            Role::Element { kind, .. } | Role::Component { base: kind, .. } => kind,
            _ => "",
        };
        for property in catalog::PROPERTIES.split_ascii_whitespace() {
            if let Some(ty) = control_alias_type(kind, property) {
                items.add(property, PROPERTY, &ty.name());
            }
        }
    }
    if receiver.is_empty()
        && let Some(owner) = owner
    {
        let kind = match index.scopes[owner].role {
            Role::Element { kind, .. } | Role::Component { base: kind, .. } => kind,
            _ => "",
        };
        if kind == "Loader" {
            items.add("status", PROPERTY, "read-only string");
        }
        if builtins::navigation_group(kind) {
            items.add(
                "count",
                PROPERTY,
                "read-only int · number of navigation items",
            );
        }
        if kind == "StackLayout" {
            items.add("active-index", PROPERTY, "read-only int");
        }
        if index.value_navigation.contains(&owner) {
            items.add(
                "current-index",
                PROPERTY,
                "read-only int · index of current-value",
            );
        }
        for property in [
            "dragging",
            "flicking",
            "moving",
            "rebounding",
            "horizontal-overshoot",
            "vertical-overshoot",
        ] {
            if let Some(ty) = xrack_ir::semantic::flick_motion_property(kind, property) {
                items.add(property, PROPERTY, &format!("read-only {}", ty.name()));
            }
        }
        for property in ["has-focus", "focus-within", "pressed"] {
            if focus_property(kind, property) {
                items.add(property, PROPERTY, "read-only bool");
            }
        }
        for property in ["popup-open", "highlighted-index"] {
            if let Some(ty) = xrack_ir::semantic::combo_popup_property(kind, property) {
                items.add(property, PROPERTY, &format!("read-only {}", ty.name()));
            }
        }
        if xrack_ir::semantic::button_checked_property(kind, "checked")
            || xrack_ir::semantic::radio_checked_property(kind, "checked")
            || xrack_ir::semantic::navigation_active_property(kind, "checked")
        {
            items.add("checked", PROPERTY, "effective bool · selected state");
        }
        if xrack_ir::semantic::navigation_active_property(kind, "active") {
            items.add("active", PROPERTY, "read-only bool · active page");
        }
        for property in ["checked", "check-state"] {
            if let Some(ty) = xrack_ir::semantic::toggle_state_property(kind, property) {
                items.add(property, PROPERTY, &format!("effective {}", ty.name()));
            }
        }
        for property in ["value", "lower-value", "upper-value"] {
            if let Some(ty) = xrack_ir::semantic::numeric_control_property(kind, property) {
                items.add(property, PROPERTY, &format!("committed {}", ty.name()));
            }
        }
        for property in [
            "count",
            "current-index",
            "current-text",
            "current-value",
            "edit-text",
        ] {
            if let Some(ty) = xrack_ir::semantic::combo_state_property(kind, property) {
                let detail = if property == "current-value" {
                    index.choice_value_type(owner).map_or_else(
                        || "effective value · model value-role".into(),
                        |ty| format!("effective {} · model value-role", ty.name()),
                    )
                } else {
                    format!("effective {}", ty.name())
                };
                items.add(property, PROPERTY, &detail);
            }
        }
        for property in ["active", "pressed", "hovered"] {
            if xrack_ir::semantic::scroll_state_property(kind, property) {
                items.add(property, PROPERTY, "read-only bool · scroll interaction");
            }
        }
        for property in ["visual-position", "visual-size"] {
            if xrack_ir::semantic::scroll_visual_property(kind, property) {
                items.add(
                    property,
                    PROPERTY,
                    "read-only float · normalized thumb geometry",
                );
            }
        }
        for property in [
            "position",
            "visual-position",
            "lower-position",
            "upper-position",
            "lower-visual-position",
            "upper-visual-position",
        ] {
            if xrack_ir::semantic::range_position_property(kind, property) {
                items.add(property, PROPERTY, "read-only float · normalized position");
            }
        }
        for method in catalog::dynamic_methods(kind) {
            if matches!(method.name, "get" | "status" | "is-mounted")
                || matches!(index.scopes[scope].role, Role::Handler)
            {
                items.add(method.name, FUNCTION, &method.signature());
            }
        }
        if matches!(index.scopes[scope].role, Role::Handler) {
            for method in catalog::focus_methods(kind)
                .iter()
                .chain(catalog::text_editor_methods(kind))
                .chain(index.button_methods(owner, kind))
                .chain(catalog::timer_methods(kind))
                .chain(catalog::scroll_bar_methods(kind))
            {
                items.add(method.name, FUNCTION, &method.signature());
            }
        }
        if xrack_ir::semantic::list_selection_property(kind, "selected-view-index") {
            items.add(
                "selected-view-index",
                PROPERTY,
                "read-only int · displayed selection position",
            );
        }
        for property in builtins::VIEW_COUNT_PROPERTIES {
            if view_count_property(kind, property) {
                items.add(property, PROPERTY, "read-only int");
            }
        }
        for property in builtins::TEXT_COMMAND_PROPERTIES {
            if text_command_property(kind, property) {
                items.add(property, PROPERTY, "read-only bool");
            }
        }
        for property in builtins::TEXT_SELECTION_PROPERTIES {
            if let Some(ty) = text_selection_property(kind, property) {
                items.add(property, PROPERTY, &format!("read-only {}", ty.name()));
            }
        }
        if xrack_ir::semantic::is_window(kind) {
            items.words(
                "window-active minimized minimized-known maximized full-screen",
                PROPERTY,
                "read-only bool",
            );
            if matches!(role, Role::Handler) {
                items.words(
                    "minimize maximize restore toggle-maximized toggle-full-screen",
                    FUNCTION,
                    "()",
                );
                items.add("set-full-screen", FUNCTION, "(bool)");
            }
        }
        if kind == "ButtonGroup" {
            items.add("check-state", PROPERTY, "CheckState · non-exclusive groups");
            if matches!(role, Role::Handler) {
                items.add("set-check-state", FUNCTION, "(CheckState)");
            }
        }
        if kind == "ActionGroup" {
            items.add(
                "current-value",
                PROPERTY,
                "read-only string · exclusive groups",
            );
        }
        for property in ["pending", "selected-file", "selected-files"] {
            if let Some(ty) = xrack_ir::semantic::native_dialog_property(kind, property) {
                items.add(property, PROPERTY, &format!("read-only {}", ty.name()));
            }
        }
        if geometry_owner(kind)
            && (is_builtin(kind)
                || index.declared_component(kind).is_some()
                || index.imported_members.contains_key(kind))
        {
            items.words(
                "layout-x layout-y layout-width layout-height window-bounds-x window-bounds-y window-bounds-width window-bounds-height",
                PROPERTY, "read-only length",
            );
            items.add("layout-ready", PROPERTY, "read-only bool");
            if matches!(kind, "Pane" | "ControlFrame" | "ApplicationWindow") {
                items.words(
                    "available-width available-height",
                    PROPERTY,
                    "read-only length",
                );
            }
        }
    }
    if receiver.is_empty()
        && let Some(owner) = owner
    {
        let kind = match index.scopes[owner].role {
            Role::Element { kind, .. } | Role::Component { base: kind, .. } => kind,
            _ => "",
        };
        if builtins::text_submission_editor(kind) {
            items.add(
                "acceptable-input",
                PROPERTY,
                "read-only bool · valid for submission",
            );
            items.add("input-validity", PROPERTY, "read-only InputValidity");
        }
        let media_methods = catalog::media_methods(kind);
        if !media_methods.is_empty() {
            for property in [
                "position",
                "duration",
                "playing",
                "ready",
                "ended",
                "error",
                "status",
                "seekable",
                "live",
                "seek-start",
                "seek-end",
                "at-live-edge",
                "has-audio",
                "has-video",
                "audio-tracks",
                "subtitle-tracks",
                "media-title",
                "subtitle",
                "audio-error",
                "subtitle-error",
                "buffer-progress",
                "bitrate",
                "loops-remaining",
            ] {
                if let Some(ty) = xrack_ir::semantic::media_object_read_type(kind, property) {
                    items.add(property, PROPERTY, &format!("read-only {}", ty.name()));
                }
            }
            for method in media_methods {
                items.add(method.name, FUNCTION, &method.signature());
            }
        }
        if kind == "Loader" {
            items.add("status", PROPERTY, "read-only string");
        }
        if matches!(kind, "ScrollBar" | "ScrollIndicator") {
            items.words(
                "position size",
                PROPERTY,
                "effective float · normalized range",
            );
        }
        for metric in ["active", "scale", "rotation", "center-x", "center-y"] {
            if let Some(ty) = scale_rotate_property(kind, metric) {
                items.add(metric, PROPERTY, &format!("read-only {}", ty.name()));
            }
        }
        for metric in ["hovered", "hover-x", "hover-y"] {
            if let Some(ty) = hover_property(kind, metric) {
                items.add(metric, PROPERTY, &format!("read-only {}", ty.name()));
            }
        }
        for metric in [
            "pressed",
            "contains-press",
            "has-hover",
            "active-touch-count",
            "touch-points",
            "mouse-x",
            "mouse-y",
            "pressed-x",
            "pressed-y",
        ] {
            if let Some(ty) = touch_area_property(kind, metric) {
                items.add(metric, PROPERTY, &format!("read-only {}", ty.name()));
            } else if metric == "pressed" && focus_property(kind, metric) {
                items.add(metric, PROPERTY, "read-only bool");
            }
        }
        for metric in [
            "swiping",
            "pressed-x",
            "pressed-y",
            "current-x",
            "current-y",
        ] {
            if let Some(ty) = swipe_property(kind, metric) {
                items.add(metric, PROPERTY, &format!("read-only {}", ty.name()));
            }
        }
        if kind == "SwipeGestureHandler" && matches!(role, Role::Handler) {
            items.add("cancel", FUNCTION, "()");
        }
        if matches!(kind, "TableView" | "StandardTableView") {
            items.add("current-column", PROPERTY, "read-only int");
            items.add("selection-count", PROPERTY, "read-only int");
            items.add("has-selection", PROPERTY, "read-only bool");
            items.add("current-sort-column", PROPERTY, "read-only int");
            items.add("current-sort-descending", PROPERTY, "read-only bool");
            items.words("can-undo can-redo", PROPERTY, "read-only bool");
            if matches!(role, Role::Handler) {
                items.words(
                    "select-cell set-current-cell toggle-cell",
                    FUNCTION,
                    "(int, int)",
                );
                items.words("select-row select-column", FUNCTION, "(int)");
                items.add("select-range", FUNCTION, "(int, int, int, int)");
                items.words("select-all clear-selection", FUNCTION, "()");
                items.add("sort-by", FUNCTION, "(int, bool)");
                items.add("clear-sort", FUNCTION, "()");
                items.add("edit-cell", FUNCTION, "(row: int, column: int)");
                items.add("cancel-edit", FUNCTION, "()");
                items.words("undo redo", FUNCTION, "()");
            }
        }
        for property in ["selection-active", "selection-dragging"] {
            if xrack_ir::semantic::table_selection_property(kind, property).is_some() {
                items.add(property, PROPERTY, "read-only bool");
            }
        }
        if xrack_ir::semantic::row_selection_view(kind) {
            items.add("selection-count", PROPERTY, "read-only int");
            items.add("has-selection", PROPERTY, "read-only bool");
            for method in catalog::row_selection_methods(kind)
                .iter()
                .chain(catalog::tree_selection_methods(kind))
            {
                if method.result.is_some() || matches!(role, Role::Handler) {
                    items.add(method.name, FUNCTION, &method.signature());
                }
            }
        }
        if matches!(role, Role::Handler) {
            for method in catalog::navigation_move_methods(kind) {
                items.add(method.name, FUNCTION, &method.signature());
            }
        }
        if kind == "SplitView" {
            items.add("resizing", PROPERTY, "read-only bool");
            items.add("save-state", FUNCTION, "() -> string");
            if matches!(role, Role::Handler) {
                items.add("restore-state", FUNCTION, "(string)");
            }
        }
        if kind == "TreeView" {
            items.add("is-expanded", FUNCTION, "(int) -> bool");
            items.add("depth", FUNCTION, "(int) -> int");
            items.add("row-for-key", FUNCTION, "(int) -> int");
            if matches!(role, Role::Handler) {
                items.words(
                    "expand collapse toggle-expanded expand-to",
                    FUNCTION,
                    "(int)",
                );
                items.add("expand-recursively", FUNCTION, "(key = -1, depth = -1)");
                items.add("collapse-recursively", FUNCTION, "(key = -1)");
            }
        }
        if viewport_kind(kind) {
            items.words("visible-width visible-height", PROPERTY, "read-only length");
            if kind == "ScrollView" {
                items.words(
                    "effective-scroll-bar-width effective-scroll-bar-height",
                    PROPERTY,
                    "read-only length",
                );
            }
            let batch = catalog::batch_selection_methods(kind);
            if !batch.is_empty() {
                items.add("selected-keys", PROPERTY, "read-only [int]");
            }
            for method in catalog::row_key_methods(kind).iter().chain(batch) {
                if method.result.is_some() || matches!(role, Role::Handler) {
                    items.add(method.name, FUNCTION, &method.signature());
                }
            }
            if !content_viewport(kind) {
                items.add("view-index-for-source", FUNCTION, "(source: int) -> int");
                items.add("source-index-for-view", FUNCTION, "(index: int) -> int");
            }
            for property in ["content-x", "content-y"] {
                let description = if viewport_offset_property(kind, property) {
                    "effective length · supports two-way binding"
                } else {
                    "read-only length"
                };
                items.add(property, PROPERTY, description);
            }
            if matches!(role, Role::Handler) {
                for method in catalog::viewport_move_methods(kind)
                    .iter()
                    .chain(catalog::child_position_methods(kind))
                {
                    items.add(method.name, FUNCTION, &method.signature());
                }
                if !content_viewport(kind) {
                    items.add("select-source-index", FUNCTION, "(source: int)");
                    items.add("position-view-at-index", FUNCTION, "(int, ScrollAlignment)");
                    items.add(
                        "position-view-at-source-index",
                        FUNCTION,
                        "(source: int, alignment: ScrollAlignment)",
                    );
                }
                if kind == "Flickable" {
                    items.add("cancel-flick", FUNCTION, "()");
                    items.add("return-to-bounds", FUNCTION, "()");
                }
                if matches!(kind, "TableView" | "StandardTableView") {
                    items.add("select-source-cell", FUNCTION, "(source: int, column: int)");
                    items.words(
                        "position-view-at-row position-view-at-column",
                        FUNCTION,
                        "(int, ScrollAlignment)",
                    );
                    items.add("position-view-at-cell", FUNCTION, "(row: int, column: int, horizontal: ScrollAlignment, vertical: ScrollAlignment)");
                    items.add("position-view-at-source-cell", FUNCTION, "(source: int, column: int, horizontal: ScrollAlignment, vertical: ScrollAlignment)");
                    items.add("move-column", FUNCTION, "(int, int)");
                    items.add("move-row", FUNCTION, "(int, int)");
                    items.add("set-row-height", FUNCTION, "(int, length)");
                }
            }
        }
    }
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
    if receiver.is_empty()
        && let Some(owner) = owner
        && matches!(
            index.scopes[owner].role,
            Role::Element {
                kind: "StackPage",
                ..
            }
        )
    {
        items.add("active", PROPERTY, "read-only bool");
        items.add("stack-index", PROPERTY, "read-only int");
    }
    let mut members = if let Some(owner) = owner {
        index.members(owner, false)
    } else {
        let ty = if first == "option-data" {
            index.combo_option_data(scope).and_then(|(ty, _)| ty)
        } else {
            index.ancestors(scope).find_map(|i| {
                index
                    .members(i, false)
                    .into_iter()
                    .find(|m| m.name == first)?
                    .ty
                    .clone()
            })
        };
        let Some(Type::Record(record)) = ty else {
            return;
        };
        index.record(&record).iter().collect()
    };
    let mut fields = receiver.into_iter().rev().peekable();
    while let Some(field) = fields.next() {
        match members
            .iter()
            .find(|member| member.name == field)
            .and_then(|member| member.ty.as_ref())
        {
            Some(Type::Record(record)) => members = index.record(record).iter().collect(),
            Some(Type::Image) if fields.peek().is_none() => {
                items.words("width height", FIELD, "read-only int");
                return;
            }
            Some(ty) if !ty.builtin_fields().is_empty() => {
                complete_builtin_fields(ty.clone(), fields, items);
                return;
            }
            _ => return,
        }
    }
    for member in members {
        items.member(member);
    }
}

fn complete_builtin_fields<'a>(
    mut ty: Type,
    fields: impl Iterator<Item = &'a str>,
    items: &mut Items,
) {
    for field in fields {
        let Some(next) = ty.builtin_field(field).map(|(_, ty)| ty) else {
            return;
        };
        ty = next;
    }
    for &field in ty.builtin_fields() {
        if let Some((_, result)) = ty.builtin_field(field) {
            let detail = if result == Type::Hit3DList {
                "[hit3d]".into()
            } else {
                result.name()
            };
            items.add(field, FIELD, &detail);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::complete;

    #[test]
    fn dynamic_container_methods_complete_in_handlers() {
        let source = "component App inherits Window { children := DynamicContainer {} button := Button { clicked => { children.mou";
        let entries = complete(source, 0, source.len(), None);
        for label in ["mount", "mount-async", "mount-url"] {
            assert!(entries.iter().any(|item| item["label"] == label), "{label}");
        }
    }

    #[test]
    fn loader_completes_bindings_and_declared_components() {
        let source =
            "component Details inherits VerticalLayout {} component App inherits Window { Lo";
        let entries = complete(source, 0, source.len(), None);
        assert!(entries.iter().any(|item| item["label"] == "Loader"));

        let source = "component Details inherits VerticalLayout {} component App inherits Window { Loader { ";
        let entries = complete(source, 0, source.len(), None);
        for label in [
            "active",
            "asynchronous",
            "source",
            "source-url",
            "loaded",
            "Details",
        ] {
            assert!(entries.iter().any(|item| item["label"] == label), "{label}");
        }
        for label in ["Button", "@children", "for", "status"] {
            assert!(
                !entries.iter().any(|item| item["label"] == label),
                "{label}"
            );
        }
    }

    #[test]
    fn loader_status_is_available_in_bindings() {
        let source = "component App inherits Window { area := Loader {} Text { text: area.st";
        let entries = complete(source, 0, source.len(), None);
        assert!(entries.iter().any(|item| item["label"] == "status"));
    }

    #[test]
    fn table_command_completions_respect_the_receiver_and_effect_context() {
        for kind in [
            "TableView",
            "StandardTableView",
            "ListView",
            "GridView",
            "StandardListView",
            "TreeView",
            "ScrollView",
            "Flickable",
        ] {
            for effect in [true, false] {
                let suffix = if effect {
                    "button := Button { clicked => { view."
                } else {
                    "out property <length> x: view."
                };
                let source =
                    format!("component App inherits Window {{ view := {kind} {{}} {suffix}");
                let entries = complete(&source, 0, source.len(), None);
                for name in [
                    "position-view-at-row",
                    "position-view-at-column",
                    "position-view-at-cell",
                    "position-view-at-source-cell",
                    "select-source-cell",
                ] {
                    assert_eq!(
                        entries.iter().any(|item| item["label"] == name),
                        effect && matches!(kind, "TableView" | "StandardTableView"),
                        "{source}: {name}"
                    );
                }
                let virtual_view = !matches!(kind, "ScrollView" | "Flickable");
                for name in ["position-view-at-source-index", "select-source-index"] {
                    assert_eq!(
                        entries.iter().any(|item| item["label"] == name),
                        effect && virtual_view,
                        "{source}: {name}"
                    );
                }
                for name in ["view-index-for-source", "source-index-for-view"] {
                    assert_eq!(
                        entries.iter().any(|item| item["label"] == name),
                        virtual_view,
                        "{source}: {name}"
                    );
                }
                for name in ["edit-cell", "cancel-edit", "undo", "redo"] {
                    assert_eq!(
                        entries.iter().any(|item| item["label"] == name),
                        effect && matches!(kind, "TableView" | "StandardTableView"),
                        "{source}: {name}"
                    );
                }
                for name in ["can-undo", "can-redo"] {
                    assert_eq!(
                        entries.iter().any(|item| item["label"] == name),
                        matches!(kind, "TableView" | "StandardTableView"),
                        "{source}: {name}"
                    );
                }
            }
        }
    }

    #[test]
    fn flickable_completes_motion_metrics_and_scoped_cancellation() {
        let source = "component App inherits Window { canvas := Flickable {} button := Button { clicked() => { canvas.";
        let entries = complete(source, 0, source.len(), None);
        for label in [
            "dragging",
            "flicking",
            "moving",
            "rebounding",
            "vertical-overshoot",
            "cancel-flick",
            "return-to-bounds",
            "scroll-to",
            "content-x",
        ] {
            assert!(entries.iter().any(|item| item["label"] == label));
        }
        assert!(
            !entries
                .iter()
                .any(|item| item["label"] == "position-view-at-index")
        );
    }

    #[test]
    fn qualified_viewport_completion_separates_metrics_and_commands() {
        let source = "component App inherits Window { view := ScrollView {} jump := Button { clicked() => { view.";
        let entries = complete(source, 0, source.len(), None);
        let has = |label| entries.iter().any(|item| item["label"] == label);
        assert!(has("content-y"));
        assert!(has("visible-height"));
        assert!(has("scroll-to"));
        assert!(!has("position-view-at-index"));
    }

    #[test]
    fn qualified_swipe_completion_includes_metrics_and_cancellation() {
        let source =
            "component App inherits Window { swipe := SwipeGestureHandler { moved() => { swipe.";
        let entries = complete(source, 0, source.len(), None);
        for label in [
            "swiping",
            "pressed-x",
            "pressed-y",
            "current-x",
            "current-y",
            "cancel",
        ] {
            assert!(entries.iter().any(|item| item["label"] == label));
        }
    }

    #[test]
    fn qualified_scale_rotate_completion_includes_live_metrics() {
        let source = "component App inherits Window { area := ScaleRotateGestureHandler {} out property <float> zoom: area.";
        let entries = complete(source, 0, source.len(), None);
        for label in ["scale", "rotation", "center-x", "center-y", "active"] {
            assert!(entries.iter().any(|item| item["label"] == label));
        }
    }

    #[test]
    fn scroll_snap_mode_completion_uses_the_typed_enum() {
        let source = "component App inherits Window { ScrollBar { snap-mode: ";
        let entries = complete(source, 0, source.len(), None);
        for label in ["none", "always", "on-release"] {
            assert!(entries.iter().any(|item| item["label"] == label));
        }
    }

    #[test]
    fn qualified_scroll_control_completion_includes_effective_range() {
        for declaration in [
            "ScrollBar { target: view; }",
            "ScrollIndicator { size: 0.25; }",
        ] {
            let source = format!(
                "component App inherits Window {{ view := ScrollView {{}} bar := {declaration} out property <float> progress: bar."
            );
            let entries = complete(&source, 0, source.len(), None);
            for label in ["position", "size"] {
                assert!(entries.iter().any(|item| item["label"] == label));
            }
        }
    }

    #[test]
    fn key_handler_completes_nested_modifier_fields() {
        let source = "component App inherits Window { focus := FocusScope { key-pressed(event) => { event.modifiers.";
        let labels = complete(source, 0, source.len(), None)
            .into_iter()
            .map(|item| item["label"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert!(labels.iter().any(|label| label == "primary"));
        assert!(labels.iter().any(|label| label == "shift"));
        assert!(!labels.iter().any(|label| label == "text"));
    }

    #[test]
    fn split_fill_completion_lists_only_direct_panes() {
        let source = "component App inherits Window { SplitView { fill-pane: ; sidebar := Rectangle { nested := Text {} } editor := Rectangle {} } }";
        let cursor = source.find(": ;").unwrap() + 2;
        let entries = complete(source, 0, cursor, None);
        for pane in ["sidebar", "editor"] {
            assert!(entries.iter().any(|item| item["label"] == pane), "{pane}");
        }
        assert!(!entries.iter().any(|item| item["label"] == "nested"));
    }

    #[test]
    fn split_resize_properties_complete_in_their_supported_contexts() {
        let member = "component App inherits Window { SplitView { ";
        let entries = complete(member, 0, member.len(), None);
        assert!(
            entries
                .iter()
                .any(|item| item["label"] == "handle-hit-size")
        );
        assert!(!entries.iter().any(|item| item["label"] == "resizing"));

        let expression =
            "component App inherits Window { view := SplitView {} property <bool> active: view.";
        let entries = complete(expression, 0, expression.len(), None);
        assert!(entries.iter().any(|item| item["label"] == "resizing"));
    }

    #[test]
    fn split_handle_template_inputs_complete_only_inside_the_template() {
        let source = "component App inherits Window { SplitView { ";
        let entries = complete(source, 0, source.len(), None);
        assert!(entries.iter().any(|item| item["label"] == "SplitHandle"));

        let source =
            "component App inherits Window { SplitView { SplitHandle { Rectangle { visible: ";
        let entries = complete(source, 0, source.len(), None);
        for label in [
            "handle-pressed",
            "handle-hovered",
            "handle-index",
            "handle-orientation",
        ] {
            assert!(entries.iter().any(|item| item["label"] == label), "{label}");
        }
        let source = "component App inherits Window { SplitView { Rectangle { visible: ";
        let entries = complete(source, 0, source.len(), None);
        assert!(!entries.iter().any(|item| item["label"] == "handle-pressed"));
    }

    #[test]
    fn application_window_completes_window_and_content_properties() {
        let source = "component App inherits ApplicationWindow { ";
        let entries = complete(source, 0, source.len(), None);
        for label in [
            "padding-left",
            "color-scheme",
            "close-requested",
            "MenuBar",
            "PageHeader",
            "PageFooter",
        ] {
            assert!(entries.iter().any(|item| item["label"] == label), "{label}");
        }
        let source = "component App inherits ApplicationWindow { out property <length> size: root.";
        let entries = complete(source, 0, source.len(), None);
        for label in ["available-width", "available-height", "window-active"] {
            assert!(entries.iter().any(|item| item["label"] == label), "{label}");
        }
    }

    #[test]
    fn round_button_and_tool_separator_complete_their_controls() {
        let source = "component App inherits Window { RoundButton { ";
        let entries = complete(source, 0, source.len(), None);
        for label in ["radius", "action", "clicked", "icon-only"] {
            assert!(entries.iter().any(|item| item["label"] == label), "{label}");
        }
        let source = "component App inherits Window { ToolSeparator { ";
        let entries = complete(source, 0, source.len(), None);
        for label in ["orientation", "stroke-width", "padding"] {
            assert!(entries.iter().any(|item| item["label"] == label), "{label}");
        }
        assert!(!entries.iter().any(|item| item["label"] == "clicked"));
    }

    #[test]
    fn pane_completes_content_bindings_and_read_only_available_size() {
        let source = "component App inherits Window { panel := Pane { ";
        let entries = complete(source, 0, source.len(), None);
        for name in ["content-width", "padding-left", "background", "clip"] {
            assert!(entries.iter().any(|item| item["label"] == name), "{name}");
        }
        assert!(
            !entries
                .iter()
                .any(|item| item["label"] == "available-width")
        );
        let source = "component App inherits Window { panel := ControlFrame {} out property <length> size: panel.";
        let entries = complete(source, 0, source.len(), None);
        assert!(
            entries
                .iter()
                .any(|item| item["label"] == "available-width")
        );
    }

    #[test]
    fn wheel_enabled_completes_on_numeric_controls() {
        for (kind, expected) in [("Slider", true), ("SpinBox", true), ("Button", false)] {
            let source = format!("component App inherits Window {{ {kind} {{ ");
            let entries = complete(&source, 0, source.len(), None);
            assert_eq!(
                entries.iter().any(|item| item["label"] == "wheel-enabled"),
                expected
            );
        }
    }

    #[test]
    fn wheel_handler_completes_delta_and_modifier_fields() {
        let source =
            "component App inherits Window { area := WheelHandler { wheel(event) => { event.";
        let entries = complete(source, 0, source.len(), None);
        for name in ["x", "delta-y", "lines-x", "is-line", "modifiers"] {
            assert!(entries.iter().any(|item| item["label"] == name), "{name}");
        }
        let source = format!("{source}modifiers.");
        let entries = complete(&source, 0, source.len(), None);
        assert!(entries.iter().any(|item| item["label"] == "control"));
    }

    #[test]
    fn touch_points_infer_the_repeater_item_type() {
        let source = "component App inherits Window { area := TouchArea { for point in area.touch-points : Rectangle { x: point.";
        let entries = complete(source, 0, source.len(), None);
        for field in ["id", "x", "y", "start-x", "start-y", "inside"] {
            assert!(entries.iter().any(|item| item["label"] == field), "{field}");
        }
    }

    #[test]
    fn touch_area_completes_scroll_event_and_its_payload() {
        let source = "component App inherits Window { area := TouchArea { ";
        let entries = complete(source, 0, source.len(), None);
        assert!(entries.iter().any(|item| item["label"] == "scroll-event"));
        let source = format!("{source}scroll-event(event) => {{ event.");
        let entries = complete(&source, 0, source.len(), None);
        for name in ["x", "delta-y", "lines-x", "is-line", "modifiers"] {
            assert!(entries.iter().any(|item| item["label"] == name), "{name}");
        }
    }

    #[test]
    fn touch_area_completes_state_reads_and_double_click() {
        let source =
            "component App inherits Window { area := TouchArea {} out property <bool> down: area.";
        let entries = complete(source, 0, source.len(), None);
        for name in [
            "pressed",
            "contains-press",
            "has-hover",
            "active-touch-count",
            "mouse-x",
            "pressed-y",
        ] {
            assert!(entries.iter().any(|item| item["label"] == name), "{name}");
        }
        let source = "component App inherits Window { TouchArea { ";
        let entries = complete(source, 0, source.len(), None);
        for name in [
            "double-clicked",
            "moved",
            "press-and-hold",
            "press-and-hold-interval",
            "accepted-buttons",
            "multi-touch",
        ] {
            assert!(entries.iter().any(|item| item["label"] == name), "{name}");
        }
        assert!(!entries.iter().any(|item| item["label"] == "pressed"));
    }

    #[test]
    fn touch_area_completes_pointer_event_fields() {
        let source = "component App inherits Window { TouchArea { pointer-event(event) => { event.";
        let entries = complete(source, 0, source.len(), None);
        for name in [
            "kind",
            "button",
            "x",
            "y",
            "inside",
            "is-touch",
            "touch-finger-id",
            "modifiers",
        ] {
            assert!(entries.iter().any(|item| item["label"] == name), "{name}");
        }
        let source = format!("{source}modifiers.");
        let entries = complete(&source, 0, source.len(), None);
        assert!(entries.iter().any(|item| item["label"] == "shift"));
    }

    #[test]
    fn hover_handler_completes_read_only_state_and_blocking() {
        let source = "component App inherits Window { area := HoverHandler {} out property <bool> inside: area.";
        let entries = complete(source, 0, source.len(), None);
        for name in ["hovered", "hover-x", "hover-y"] {
            assert!(entries.iter().any(|item| item["label"] == name), "{name}");
        }
        let source = "component App inherits Window { area := HoverHandler { ";
        let entries = complete(source, 0, source.len(), None);
        assert!(entries.iter().any(|item| item["label"] == "blocking"));
        assert!(entries.iter().any(|item| item["label"] == "cursor-shape"));
        assert!(!entries.iter().any(|item| item["label"] == "hovered"));
    }

    #[test]
    fn cursor_shape_completes_typed_values() {
        let source = "component App inherits Window { HoverHandler { cursor-shape: CursorShape.";
        let entries = complete(source, 0, source.len(), None);
        for name in ["auto", "crosshair", "zoom-in"] {
            assert!(entries.iter().any(|item| item["label"] == name), "{name}");
        }
    }

    #[test]
    fn navigation_shortcuts_complete_inside_their_string_bindings() {
        let source =
            "component App inherits Window { Shortcut { sequences: [\"Primary+[\", \"MouseB";
        let entries = complete(source, 0, source.len(), None);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["label"], "MouseBack");
        assert_eq!(entries[0]["textEdit"]["newText"], "MouseBack");

        let source = "component App inherits Window { Shortcut { sequence: \"Primary+\"; } }";
        let offset = source.find("Primary+").unwrap() + "Primary+".len();
        let labels: Vec<_> = complete(source, 0, offset, None)
            .into_iter()
            .map(|item| item["label"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(labels, ["Primary+[", "Primary+]"]);
        let source = "component App inherits Window { Label { text: \"MouseB";
        assert!(complete(source, 0, source.len(), None).is_empty());
    }

    #[test]
    fn modified_tab_shortcuts_complete_within_the_binding() {
        let source = "component App inherits Window { Shortcut { sequence: \"Ctrl+T";
        let entries = complete(source, 0, source.len(), None);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["textEdit"]["newText"], "Ctrl+Tab");

        let source = "component App inherits Window { Shortcut { sequence: \"Ctrl+Shift+T\"; } }";
        let cursor = source.find("Ctrl+Shift+T").unwrap() + "Ctrl+Shift+T".len();
        let entries = complete(source, 0, cursor, None);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["textEdit"]["newText"], "Ctrl+Shift+Tab");
    }

    #[test]
    fn state_targets_offer_visual_properties_and_filter_transition_types() {
        let source =
            "component App inherits Window { tile := Rectangle {} states [ expanded when true: { ";
        let labels = |suffix| {
            let text = format!("{source}{suffix}");
            complete(&text, 0, text.len(), None)
                .into_iter()
                .map(|item| item["label"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        let properties = labels("tile.");
        assert!(properties.iter().any(|label| label == "width"));
        assert!(properties.iter().any(|label| label == "visible"));
        let transitions = labels("in { animate tile.");
        assert!(transitions.iter().any(|label| label == "width"));
        assert!(!transitions.iter().any(|label| label == "visible"));
    }

    #[test]
    fn instance_states_complete_public_inputs_and_layout_without_read_only_members() {
        let source = "component Badge inherits Rectangle { in property <string> caption; out property <string> current; property <int> secret; } component App inherits Window { badge := Badge {} states [ attention when true: { ";
        let labels = |suffix| {
            let text = format!("{source}{suffix}");
            complete(&text, 0, text.len(), None)
                .into_iter()
                .map(|item| item["label"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        let properties = labels("badge.");
        for name in ["caption", "width"] {
            assert!(properties.iter().any(|label| label == name));
        }
        for name in ["secret", "current", "layout-width"] {
            assert!(!properties.iter().any(|label| label == name));
        }
        assert!(
            !labels("in { animate badge.")
                .iter()
                .any(|label| label == "caption")
        );
    }
}
