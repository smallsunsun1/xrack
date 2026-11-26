use crate::analysis::Source;
use crate::completion::{CLASS, Index, Member, Role, STRUCT};
use crate::editor_tokens::{self, Token, name};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Seek, SeekFrom},
    ops::Range,
    path::{Path, PathBuf},
};

pub(super) use xrack_ir::modules::normalize_source_path as normalize;
use xrack_parser::builtins::{
    HEIGHTFIELD_EXTENSION, MESH_SOURCE_EXTENSIONS, OBJ_EXTENSION, PhysicsFunction,
};
use xrack_parser::gltf_metadata::{
    INSTANCE_COLOR_ALIAS, INSTANCE_COLOR_ATTRIBUTE, INSTANCE_DATA_ATTRIBUTE,
};

pub(crate) struct Workspace<'a> {
    pub(crate) entry: PathBuf,
    open: BTreeMap<PathBuf, (&'a str, &'a str)>,
    indexed_modules: Option<&'a BTreeMap<PathBuf, Module>>,
    edited_modules: Option<&'a BTreeMap<PathBuf, Module>>,
}

impl<'a> Workspace<'a> {
    pub(crate) fn new(entry: &str, open: impl Iterator<Item = (&'a str, &'a str)>) -> Option<Self> {
        Some(Self {
            entry: file_path(entry)?,
            open: open
                .filter_map(|(uri, text)| Some((normalize(&file_path(uri)?), (uri, text))))
                .collect(),
            indexed_modules: None,
            edited_modules: None,
        })
    }

    pub(crate) fn buffers(&self) -> BTreeMap<PathBuf, &'a str> {
        self.open
            .iter()
            .map(|(path, &(_, text))| (path.clone(), text))
            .collect()
    }

    pub(super) fn indexed(entry: &Path, modules: &'a BTreeMap<PathBuf, Module>) -> Self {
        Self {
            entry: entry.to_owned(),
            open: BTreeMap::new(),
            indexed_modules: Some(modules),
            edited_modules: None,
        }
    }

    pub(super) fn with_edits(mut self, modules: &'a BTreeMap<PathBuf, Module>) -> Self {
        self.edited_modules = Some(modules);
        self
    }

    pub(super) fn for_entry(&self, path: &Path) -> Self {
        Self {
            entry: path.to_owned(),
            open: self.open.clone(),
            indexed_modules: self.indexed_modules,
            edited_modules: self.edited_modules,
        }
    }

    pub(super) fn imported_members(
        &self,
        tokens: &[Token<'_>],
    ) -> (BTreeMap<String, Vec<Member>>, BTreeSet<String>) {
        let imports = import_targets(&self.entry, tokens);
        let mut modules = BTreeMap::new();
        let members = self.interfaces(&imports, &mut modules);
        let globals = imports
            .iter()
            .filter_map(|(alias, target)| {
                let module = self.module(&target.file, &mut modules);
                let component = module.components.get(&target.name)?;
                (component.exported && component.base == "global").then(|| alias.clone())
            })
            .collect();
        (members, globals)
    }

    pub(super) fn interfaces(
        &self,
        imports: &BTreeMap<String, Target>,
        modules: &mut BTreeMap<PathBuf, Module>,
    ) -> BTreeMap<String, Vec<Member>> {
        imports
            .iter()
            .filter_map(|(alias, target)| {
                self.interface(target.clone(), modules)
                    .map(|members| (alias.clone(), members))
            })
            .collect()
    }

    pub(super) fn import_symbols(
        &self,
        tokens: &[Token<'_>],
        cursor: usize,
    ) -> Option<Vec<(String, u32)>> {
        let code: Vec<_> = tokens.iter().filter(|t| t.code() || t.quoted()).collect();
        let clause = named_imports(&code)
            .find(|clause| clause.body.start <= cursor && cursor <= clause.body.end)?;
        let before = &clause.names[..clause.names.partition_point(|t| t.range.end <= cursor)];
        let active = before.iter().filter(|t| t.text == ",").count();
        let used: BTreeSet<_> = clause
            .names
            .split(|t| t.text == ",")
            .enumerate()
            .filter_map(|(i, binding)| (i != active).then_some(binding.first()?.text))
            .collect();
        if before
            .iter()
            .rev()
            .take_while(|t| t.text != ",")
            .any(|t| t.text == "as" || t.range.end < cursor)
        {
            return Some(vec![]);
        }
        let file = module_path(&self.entry, clause.source.text);
        let module = file.and_then(|file| {
            self.load(&normalize(&file))
                .map(|text| Module::new(&file, &text))
        });
        Some(module.map_or_else(Vec::new, |module| {
            module
                .components
                .into_iter()
                .filter(|(name, component)| component.exported && !used.contains(name.as_str()))
                .map(|(name, _)| (name, CLASS))
                .chain(
                    module
                        .records
                        .into_iter()
                        .filter(|(name, record)| record.exported && !used.contains(name.as_str()))
                        .map(|(name, _)| (name, STRUCT)),
                )
                .collect()
        }))
    }

    pub(crate) fn load(&self, path: &Path) -> Option<Cow<'_, str>> {
        if let Some(&(_, text)) = self.open.get(path) {
            return Some(Cow::Borrowed(text));
        }
        fs::read_to_string(path).ok().map(Cow::Owned)
    }

    pub(crate) fn uri(&self, path: &Path) -> Option<String> {
        if let Some(&(uri, _)) = self.open.get(path) {
            return Some(uri.into());
        }
        let path = path.to_str()?;
        #[cfg(windows)]
        let path = if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
            unc.replace('\\', "/")
        } else if let Some(unc) = path
            .strip_prefix(r"\\")
            .filter(|path| !path.starts_with("?\\"))
        {
            unc.replace('\\', "/")
        } else {
            format!(
                "/{}",
                path.strip_prefix(r"\\?\")
                    .unwrap_or(path)
                    .replace('\\', "/")
            )
        };
        let mut uri = String::from("file://");
        for byte in path.bytes() {
            if byte.is_ascii_alphanumeric() || b"/:-._~".contains(&byte) {
                uri.push(char::from(byte));
            } else {
                use std::fmt::Write;
                write!(uri, "%{byte:02X}").ok()?;
            }
        }
        Some(uri)
    }

    pub(super) fn module<'m>(
        &'m self,
        path: &Path,
        modules: &'m mut BTreeMap<PathBuf, Module>,
    ) -> &'m Module {
        if let Some(module) = self.edited_modules.and_then(|modules| modules.get(path)) {
            return module;
        }
        if let Some(indexed) = self.indexed_modules {
            if let Some(module) = indexed.get(path) {
                return module;
            }
            return modules.entry(path.to_owned()).or_default();
        }
        let canonical = normalize(path);
        modules.entry(canonical.clone()).or_insert_with(|| {
            self.load(&canonical)
                .map_or_else(Module::default, |text| Module::new(path, &text))
        })
    }

    fn interface(
        &self,
        mut target: Target,
        modules: &mut BTreeMap<PathBuf, Module>,
    ) -> Option<Vec<Member>> {
        let mut members = Vec::new();
        let mut visited = BTreeSet::new();
        let mut exported = true;
        let mut resolved = false;
        loop {
            let canonical = normalize(&target.file);
            if !visited.insert((canonical.clone(), target.name.clone())) {
                break;
            }
            let module = self.module(&target.file, modules);
            let Some(component) = module.components.get(&target.name) else {
                break;
            };
            if exported && !component.exported {
                break;
            }
            resolved = true;
            members.extend(
                component
                    .members
                    .iter()
                    .filter(|member| member.visibility != "private")
                    .cloned(),
            );
            if module.components.contains_key(&component.base) {
                target.name.clone_from(&component.base);
                exported = false;
            } else if let Some(import) = module.imports.get(&component.base) {
                target = import.clone();
                exported = true;
            } else {
                break;
            }
        }
        resolved.then_some(members)
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(super) struct Target {
    pub(super) file: PathBuf,
    pub(super) input: PathBuf,
    pub(super) name: String,
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Module {
    pub(super) components: BTreeMap<String, Component>,
    pub(super) records: BTreeMap<String, Record>,
    pub(super) imports: BTreeMap<String, Target>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Component {
    base: String,
    pub(super) exported: bool,
    pub(super) range: Range<usize>,
    members: Vec<Member>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Record {
    pub(super) exported: bool,
    pub(super) range: Range<usize>,
    pub(super) fields: Vec<Member>,
}

impl Module {
    pub(super) fn symbols(&self) -> impl Iterator<Item = (&str, u32, &Range<usize>, bool)> {
        self.components
            .iter()
            .map(|(name, item)| (name.as_str(), CLASS, &item.range, item.exported))
            .chain(
                self.records
                    .iter()
                    .map(|(name, item)| (name.as_str(), STRUCT, &item.range, item.exported)),
            )
    }

    pub(super) fn new(path: &Path, text: &str) -> Self {
        Self::from_tokens(path, &editor_tokens::scan(text), text.len())
    }

    pub(super) fn from_tokens(path: &Path, tokens: &[Token<'_>], end: usize) -> Self {
        let code: Vec<_> = tokens.iter().filter(|t| t.code()).collect();
        let index = Index::new(&code, end);
        let mut components = BTreeMap::new();
        let mut records = BTreeMap::new();
        let canonical = normalize(path);
        for mut scope in index.scopes {
            if !matches!(scope.role, Role::Record { .. } | Role::Component { .. }) {
                continue;
            }
            let header = &code[scope.header];
            let Some(declaration) =
                header.get(if header.first().is_some_and(|t| t.text == "export") {
                    2
                } else {
                    1
                })
            else {
                continue;
            };
            for member in &mut scope.members {
                member.file = Some(canonical.clone());
                member.type_file.clone_from(&member.file);
            }
            let range = declaration.range.clone();
            match scope.role {
                Role::Record { name, exported } => {
                    records.insert(
                        name.into(),
                        Record {
                            exported,
                            range,
                            fields: scope.members,
                        },
                    );
                }
                Role::Component {
                    name,
                    base,
                    exported,
                } => {
                    components.insert(
                        name.into(),
                        Component {
                            base: base.into(),
                            exported,
                            range,
                            members: scope.members,
                        },
                    );
                }
                _ => {}
            }
        }
        Self {
            components,
            records,
            imports: import_targets(path, tokens),
        }
    }
}

pub(super) fn import_targets(path: &Path, tokens: &[Token<'_>]) -> BTreeMap<String, Target> {
    let code: Vec<_> = tokens.iter().filter(|t| t.code() || t.quoted()).collect();
    let mut imports = BTreeMap::new();
    for clause in named_imports(&code) {
        let Some(input) = import_input(path, clause.source.text) else {
            continue;
        };
        let file = normalize(&input);
        for binding in clause.names.split(|t| t.text == ",") {
            let (original, alias) = match binding {
                [original] => (original.text, original.text),
                [original, as_token, alias] if as_token.text == "as" => (original.text, alias.text),
                _ => continue,
            };
            if name(original) && name(alias) {
                imports.entry(alias.into()).or_insert_with(|| Target {
                    file: file.clone(),
                    input: input.clone(),
                    name: original.into(),
                });
            }
        }
    }
    imports
}

pub(super) struct ImportClause<'a, 'source> {
    body: Range<usize>,
    pub(super) names: &'a [&'a Token<'source>],
    pub(super) source: &'a Token<'source>,
}

enum Preamble<'a, 'source> {
    Named(ImportClause<'a, 'source>),
    Font(&'a Token<'source>),
    Translations,
}

fn preamble<'a, 'source>(
    code: &'a [&'a Token<'source>],
) -> impl Iterator<Item = Preamble<'a, 'source>> {
    let mut rest = code;
    std::iter::from_fn(move || {
        let [keyword, open, tail @ ..] = rest else {
            return None;
        };
        if matches!(keyword.text, "import" | "translations")
            && open.quoted()
            && tail.first().is_some_and(|token| token.text == ";")
        {
            rest = &tail[1..];
            return Some(if keyword.text == "import" {
                Preamble::Font(open)
            } else {
                Preamble::Translations
            });
        }
        if keyword.text != "import" || open.text != "{" {
            return None;
        }
        let close = tail.iter().position(|token| token.text == "}")?;
        let [from, source, next @ ..] = &tail[close + 1..] else {
            return None;
        };
        if from.text != "from" || !source.quoted() {
            return None;
        }
        rest = if next.first().is_some_and(|token| token.text == ";") {
            &next[1..]
        } else {
            &[]
        };
        Some(Preamble::Named(ImportClause {
            body: open.range.end..tail[close].range.start,
            names: &tail[..close],
            source,
        }))
    })
}

pub(super) fn named_imports<'a, 'source>(
    code: &'a [&'a Token<'source>],
) -> impl Iterator<Item = ImportClause<'a, 'source>> {
    preamble(code).filter_map(|item| match item {
        Preamble::Named(clause) => Some(clause),
        _ => None,
    })
}

pub(super) fn module_path(path: &Path, source: &str) -> Option<PathBuf> {
    import_input(path, source).map(|input| normalize(&input))
}

fn import_input(path: &Path, source: &str) -> Option<PathBuf> {
    let relative: PathBuf = serde_json::from_str(source).ok()?;
    if relative.is_absolute() {
        return None;
    }
    let requested = path.parent()?.join(relative);
    match xrack_ir::modules::package_entry(&requested).ok()? {
        Some(entry) => Some(entry),
        None if requested.extension().is_some_and(|ext| ext == "xr") => Some(requested),
        None => None,
    }
}

fn source_links(text: &str, workspace: &Workspace<'_>) -> Vec<(Range<usize>, String)> {
    let tokens = editor_tokens::scan(text);
    let code: Vec<_> = tokens
        .iter()
        .filter(|token| token.code() || token.quoted())
        .collect();
    let imports = preamble(&code).filter_map(|item| {
        let (token, path) = match item {
            Preamble::Named(clause) => (
                clause.source,
                module_path(&workspace.entry, clause.source.text)?,
            ),
            Preamble::Font(token) => {
                let path: PathBuf = serde_json::from_str(token.text).ok()?;
                if path.is_absolute() {
                    return None;
                }
                (token, normalize(&workspace.entry.parent()?.join(path)))
            }
            Preamble::Translations => return None,
        };
        // An unsaved buffer can be a valid import before the file exists on disk.
        if !workspace.open.contains_key(&path) && !path.is_file() {
            return None;
        }
        let uri = workspace.uri(&path)?;
        Some((token.range.start + 1..token.range.end - 1, uri))
    });
    let assets = tokens.windows(3).enumerate().filter_map(|(index, window)| {
        let [_, open, token] = window else {
            return None;
        };
        let extensions = scene_asset_extensions(&tokens, index)?;
        if open.text != "(" || !token.quoted() {
            return None;
        }
        let spelling: String = serde_json::from_str(token.text).ok()?;
        let relative = Path::new(&spelling);
        if relative.is_absolute()
            || spelling.contains(':')
            || !matches_asset_extension(relative, extensions)
        {
            return None;
        }
        let path = normalize(&workspace.entry.parent()?.join(relative));
        if !path.is_file() {
            return None;
        }
        Some((
            token.range.start + 1..token.range.end - 1,
            workspace.uri(&path)?,
        ))
    });
    imports.chain(assets).collect()
}

pub(crate) fn document_links(text: &str, workspace: &Workspace<'_>) -> Value {
    let source = Source::new(text);
    json!(
        source_links(text, workspace)
            .into_iter()
            .map(|(range, target)| {
                json!({"range": source.range(range.start, range.end), "target": target})
            })
            .collect::<Vec<_>>()
    )
}

fn scene_asset_extensions(tokens: &[Token<'_>], index: usize) -> Option<&'static [&'static str]> {
    match tokens.get(index)?.text {
        "@mesh-url" => Some(MESH_SOURCE_EXTENSIONS),
        "@environment-url" => Some(&["hdr"]),
        "@instance-table-url" => Some(&["xml", "bin"]),
        "@shader-url" | "@material-shader-url" => Some(&["wgsl"]),
        "@texture-url" => Some(&["png", "jpg", "jpeg"]),
        "@image-url"
            if index >= 2
                && matches!(
                    tokens[index - 2].text,
                    "scene-lut" | "sprite" | "color-table"
                )
                && tokens[index - 1].text == ":" =>
        {
            Some(&["png"])
        }
        _ => None,
    }
}

fn matches_asset_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            extensions
                .iter()
                .any(|expected| ext.eq_ignore_ascii_case(expected))
        })
}

pub(super) fn scene_asset_selector_values(
    source: &Source<'_>,
    tokens: &[Token<'_>],
    cursor: usize,
    workspace: &Workspace<'_>,
) -> Option<Vec<Value>> {
    let (field, kind, literal) = tokens.windows(3).enumerate().find_map(|(index, tokens)| {
        let [field, colon, string] = tokens else {
            return None;
        };
        (matches!(
            field.text,
            "animation" | "animation-to" | "material-variant"
        ) && colon.text == ":"
            && string.text.starts_with('"')
            && string.blocks_completion(cursor))
        .then_some((index, field.text, string))
    })?;
    let open = enclosing_block(tokens, field)?;
    if !name(tokens.get(open.checked_sub(1)?)?.text) {
        return None;
    }
    let metadata = scene_metadata(&scene_row_asset(tokens, open)?, workspace)?;
    let prefix = &literal.text[1..cursor - literal.range.start];
    let range = source.range(
        literal.range.start + 1,
        literal.range.end - usize::from(literal.quoted()),
    );
    let (selectors, detail) = if kind == "material-variant" {
        (
            metadata.material_variant_selectors(),
            "glTF material variant",
        )
    } else {
        (metadata.animation_selectors(), "glTF animation clip")
    };
    Some(
        selectors
            .into_iter()
            .filter(|label| label.starts_with(prefix))
            .map(|label| {
                let replacement = json!(label).to_string();
                json!({
                    "label": label, "kind": 12, "detail": detail,
                    "textEdit": {"range": range, "newText": &replacement[1..replacement.len() - 1]},
                })
            })
            .collect(),
    )
}

fn enclosing_block(tokens: &[Token<'_>], before: usize) -> Option<usize> {
    let mut nested = 0usize;
    (0..before).rev().find(|&index| {
        let token = &tokens[index];
        if token.code() {
            match token.text {
                "}" => nested += 1,
                "{" if nested == 0 => return true,
                "{" => nested -= 1,
                _ => {}
            }
        }
        false
    })
}

enum RowMesh {
    Asset(String),
    Primitive(String),
}

fn scene_row_asset(tokens: &[Token<'_>], open: usize) -> Option<String> {
    match scene_row_mesh(tokens, open)? {
        RowMesh::Asset(path) => Some(path),
        RowMesh::Primitive(_) => None,
    }
}

fn scene_row_mesh(tokens: &[Token<'_>], open: usize) -> Option<RowMesh> {
    let mut depth = 0usize;
    for (offset, token) in tokens[open + 1..].iter().enumerate() {
        if !token.code() {
            continue;
        }
        match token.text {
            "{" => depth += 1,
            "}" if depth == 0 => break,
            "}" => depth -= 1,
            "mesh" if depth == 0 => {
                let start = open + offset + 1;
                let Some([_, colon, value]) = tokens.get(start..start + 3) else {
                    continue;
                };
                if colon.text != ":" {
                    continue;
                }
                if value.quoted() {
                    let key: String = serde_json::from_str(value.text).ok()?;
                    if xrack_parser::builtins::scene_primitive_slots(&key).is_some() {
                        return Some(RowMesh::Primitive(key));
                    }
                    continue;
                }
                let Some([paren, path, end]) = tokens.get(start + 3..start + 6) else {
                    continue;
                };
                if value.text == "@mesh-url"
                    && paren.text == "("
                    && matches!(end.text, ")" | ",")
                    && path.quoted()
                {
                    return serde_json::from_str::<String>(path.text)
                        .ok()
                        .map(RowMesh::Asset);
                }
            }
            _ => {}
        }
    }
    None
}

pub(super) fn scene_instance_reference_values(
    source: &Source<'_>,
    tokens: &[Token<'_>],
    cursor: usize,
) -> Option<Vec<Value>> {
    let (field, literal) = tokens.windows(3).enumerate().find_map(|(index, window)| {
        let [field, colon, literal] = window else {
            return None;
        };
        (matches!(field.text, "instance-of" | "instance-root")
            && colon.text == ":"
            && literal.text.starts_with('"')
            && literal.blocks_completion(cursor))
        .then_some((index, literal))
    })?;
    let root = tokens[field].text == "instance-root";
    let row = enclosing_block(tokens, field)?;
    let record = tokens.get(row.checked_sub(1)?)?.text;
    if !name(record) {
        return None;
    }
    let list = enclosing_array(tokens, row)?;
    let mut depth = 0usize;
    let mut candidates = BTreeMap::new();
    for (index, token) in tokens
        .iter()
        .enumerate()
        .skip(list + 1)
        .filter(|(_, token)| token.code())
    {
        match token.text {
            "{" => {
                if depth == 0
                    && tokens
                        .get(index - 1)
                        .is_some_and(|token| token.text == record)
                    && let Some(label) = scene_row_string(tokens, index, "id")
                    && !label.is_empty()
                {
                    let links = scene_row_field(tokens, index, "instance-of")
                        .is_some_and(|target| target.text != r#""""#);
                    let alternative = scene_row_field(tokens, index, "lod-of")
                        .is_some_and(|target| target.text != r#""""#);
                    let eligible = !alternative
                        && if root {
                            index != row || !links
                        } else {
                            index != row
                                && !links
                                && scene_row_field(tokens, index, "mesh")
                                    .is_some_and(|mesh| mesh.text != r#""""#)
                        };
                    candidates
                        .entry(label)
                        .and_modify(|valid| *valid = false)
                        .or_insert(eligible);
                }
                depth += 1;
            }
            "[" | "(" => depth += 1,
            "]" | "}" | ")" if depth == 0 => break,
            "]" | "}" | ")" => depth -= 1,
            _ => {}
        }
    }
    let detail = if root {
        "Scene instance coordinate root"
    } else {
        "Scene model prototype"
    };
    let prefix = &literal.text[1..cursor - literal.range.start];
    let range = source.range(
        literal.range.start + 1,
        literal.range.end - usize::from(literal.quoted()),
    );
    Some(
        candidates
            .into_iter()
            .filter(|(label, valid)| *valid && label.starts_with(prefix))
            .map(|(label, _)| {
                let replacement = json!(label).to_string();
                json!({
                    "label": label, "kind": 12, "detail": detail,
                    "textEdit": {"range": range, "newText": &replacement[1..replacement.len() - 1]},
                })
            })
            .collect(),
    )
}

fn enclosing_array(tokens: &[Token<'_>], before: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens[..before]
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, token)| token.code())
    {
        match token.text {
            "]" | "}" | ")" => depth += 1,
            "[" if depth == 0 => return Some(index),
            "{" | "(" if depth == 0 => return None,
            "[" | "{" | "(" => depth -= 1,
            _ => {}
        }
    }
    None
}

fn scene_row_string(tokens: &[Token<'_>], open: usize, name: &str) -> Option<String> {
    let value = scene_row_field(tokens, open, name)?;
    serde_json::from_str(value.text).ok()
}

fn scene_row_field<'a>(tokens: &'a [Token<'_>], open: usize, name: &str) -> Option<&'a Token<'a>> {
    let mut depth = 0usize;
    for (offset, token) in tokens[open + 1..]
        .iter()
        .enumerate()
        .filter(|(_, token)| token.code())
    {
        match token.text {
            "{" | "[" | "(" => depth += 1,
            "}" | "]" | ")" if depth == 0 => break,
            "}" | "]" | ")" => depth -= 1,
            field if depth == 0 && field == name => {
                let start = open + offset + 2;
                let [colon, value] = tokens.get(start..start + 2)? else {
                    return None;
                };
                if colon.text == ":" {
                    return Some(value);
                }
            }
            _ => {}
        }
    }
    None
}

fn literal_field_value(tokens: &[Token<'_>], open: usize, name: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open + 1) {
        if !token.code() {
            continue;
        }
        match token.text {
            "{" | "[" | "(" => depth += 1,
            "}" | "]" | ")" if depth == 0 => return None,
            "}" | "]" | ")" => depth -= 1,
            field if depth == 0 && field == name && tokens.get(index + 1)?.text == ":" => {
                return Some(index + 2);
            }
            _ => {}
        }
    }
    None
}

fn literal_scene_rows(tokens: &[Token<'_>], view: usize, property: &str) -> Vec<usize> {
    let Some(array) = literal_field_value(tokens, view, property)
        .filter(|&array| tokens.get(array).is_some_and(|token| token.text == "["))
    else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    let mut depth = 0usize;
    for (index, token) in tokens
        .iter()
        .enumerate()
        .skip(array + 1)
        .filter(|(_, token)| token.code())
    {
        match token.text {
            "{" if depth == 0 => {
                rows.push(index);
                depth += 1;
            }
            "{" | "[" | "(" => depth += 1,
            "]" | "}" | ")" if depth == 0 => break,
            "]" | "}" | ")" => depth -= 1,
            _ => {}
        }
    }
    rows
}

struct SceneLightScope<'a> {
    literal: &'a Token<'a>,
    nodes: BTreeMap<String, Option<Range<usize>>>,
}

fn scene_light_scope<'a>(tokens: &'a [Token<'a>], cursor: usize) -> Option<SceneLightScope<'a>> {
    let (field, literal) = tokens.windows(3).enumerate().find_map(|(index, window)| {
        let [field, colon, literal] = window else {
            return None;
        };
        (field.text == "scope"
            && colon.text == ":"
            && literal.text.starts_with('"')
            && literal.blocks_completion(cursor))
        .then_some((index, literal))
    })?;
    let row = enclosing_block(tokens, field)?;
    let list = enclosing_array(tokens, row)?;
    let [property, colon] = tokens.get(list.checked_sub(2)?..list)? else {
        return None;
    };
    let view = enclosing_block(tokens, list)?;
    if property.text != "lights"
        || colon.text != ":"
        || tokens.get(view.checked_sub(1)?)?.text != "SceneView3D"
    {
        return None;
    }
    let model = literal_field_value(tokens, view, "model")?;
    let rows = if let Some([root, dot, declaration]) = tokens.get(model..model + 3)
        && root.text == "root"
        && dot.text == "."
        && name(declaration.text)
    {
        let mut container = view;
        loop {
            container = enclosing_block(tokens, container)?;
            if tokens[..container]
                .iter()
                .rev()
                .filter(|token| token.code())
                .take_while(|token| !matches!(token.text, "{" | "}" | ";"))
                .any(|token| token.text == "component")
            {
                break literal_scene_rows(tokens, container, declaration.text);
            }
        }
    } else {
        literal_scene_rows(tokens, view, "model")
    };
    let mut names = BTreeMap::new();
    for row in rows {
        let Some(id) = scene_row_field(tokens, row, "id").filter(|id| id.quoted()) else {
            continue;
        };
        let Ok(label) = serde_json::from_str::<String>(id.text) else {
            continue;
        };
        if label.is_empty()
            || scene_row_string(tokens, row, "lod-of").is_some_and(|id| !id.is_empty())
        {
            continue;
        }
        names
            .entry(label)
            .and_modify(|id| *id = None)
            .or_insert(Some(id.range.start + 1..id.range.end - 1));
    }
    Some(SceneLightScope {
        literal,
        nodes: names,
    })
}

pub(super) fn scene_light_scope_values(
    source: &Source<'_>,
    tokens: &[Token<'_>],
    cursor: usize,
) -> Option<Vec<Value>> {
    let SceneLightScope { literal, nodes } = scene_light_scope(tokens, cursor)?;
    let prefix = &literal.text[1..cursor - literal.range.start];
    let range = source.range(
        literal.range.start + 1,
        literal.range.end - usize::from(literal.quoted()),
    );
    Some(
        nodes
            .into_iter()
            .filter(|(name, id)| id.is_some() && name.starts_with(prefix))
            .map(|(name, _)| {
                let replacement = json!(name).to_string();
                json!({
                    "label": name, "kind": 12, "detail": "Light scope root node",
                    "textEdit": {"range": range, "newText": &replacement[1..replacement.len() - 1]},
                })
            })
            .collect(),
    )
}

pub(super) fn scene_light_scope_definition(
    source: &Source<'_>,
    tokens: &[Token<'_>],
    cursor: usize,
    uri: &str,
) -> Option<Value> {
    let SceneLightScope { literal, nodes } = scene_light_scope(tokens, cursor)?;
    let target: String = serde_json::from_str(literal.text).ok()?;
    let id = nodes.get(&target)?.as_ref()?;
    Some(json!({"uri": uri, "range": source.range(id.start, id.end)}))
}

fn authored_mesh_slots(tokens: &[Token<'_>], row: usize) -> Option<BTreeMap<String, String>> {
    let value = literal_field_value(tokens, row, "geometry")?;
    let geometry = value + 1;
    if tokens.get(geometry)?.text != "{" {
        return None;
    }
    literal_field_value(tokens, geometry, "vertices")?;
    let mut slots =
        BTreeMap::from([("surface".into(), "Material for unassigned triangles".into())]);
    if let Some(subsets) = literal_field_value(tokens, geometry, "subsets")
        && tokens.get(subsets)?.text == "["
    {
        let mut depth = 0usize;
        for (index, token) in tokens.iter().enumerate().skip(subsets + 1) {
            if !token.code() {
                continue;
            }
            match token.text {
                "]" if depth == 0 => break,
                "{" if depth == 0 => {
                    depth += 1;
                    if let Some(name) = literal_field_value(tokens, index, "name")
                        .and_then(|at| tokens.get(at))
                        .filter(|name| name.quoted())
                        .and_then(|name| serde_json::from_str::<String>(name.text).ok())
                        .filter(|name| !name.is_empty())
                    {
                        let selector = if name.starts_with('#') || name.starts_with("name:") {
                            format!("name:{name}")
                        } else {
                            name
                        };
                        slots.insert(selector, "Authored mesh subset".into());
                    }
                }
                "{" | "[" | "(" => depth += 1,
                "}" | "]" | ")" => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    Some(slots)
}

pub(super) fn scene_material_slot_values(
    source: &Source<'_>,
    tokens: &[Token<'_>],
    cursor: usize,
    workspace: Option<&Workspace<'_>>,
) -> Option<Vec<Value>> {
    let (field, literal) = tokens.windows(3).enumerate().find_map(|(index, window)| {
        let [field, colon, literal] = window else {
            return None;
        };
        (field.text == "slot"
            && colon.text == ":"
            && literal.text.starts_with('"')
            && literal.blocks_completion(cursor))
        .then_some((index, literal))
    })?;
    let style = enclosing_block(tokens, field)?;
    let palette = enclosing_block(tokens, style)?;
    let mut binding = tokens[..palette].iter().rev().filter(|token| token.code());
    binding.next()?;
    if binding.next()?.text != ":" || binding.next()?.text != "material-slots" {
        return None;
    }
    let row = enclosing_block(tokens, palette)?;
    let prefix = &literal.text[1..cursor - literal.range.start];
    let range = source.range(
        literal.range.start + 1,
        literal.range.end - usize::from(literal.quoted()),
    );
    let slots = if let Some(slots) = authored_mesh_slots(tokens, row) {
        slots
    } else {
        let mut slots = BTreeMap::new();
        match scene_row_mesh(tokens, row)? {
            RowMesh::Primitive(key) => {
                for (index, &name) in xrack_parser::builtins::scene_primitive_slots(&key)?
                    .iter()
                    .enumerate()
                {
                    slots.insert(format!("#{index}"), "Built-in material slot".to_owned());
                    if !name.is_empty() {
                        slots.insert(name.to_owned(), format!("Built-in material #{index}"));
                    }
                }
            }
            RowMesh::Asset(asset)
                if matches_asset_extension(Path::new(&asset), &[OBJ_EXTENSION]) =>
            {
                for (selector, name) in obj_metadata(&asset, workspace?)?.slots() {
                    slots.insert(selector.into_owned(), "OBJ material slot".into());
                    if let Some(name) = name {
                        let selector = if name.starts_with('#') || name.starts_with("name:") {
                            format!("name:{name}")
                        } else {
                            name.to_owned()
                        };
                        slots.insert(selector, "OBJ material slot".into());
                    }
                }
            }
            RowMesh::Asset(asset) => {
                let metadata = scene_metadata(&asset, workspace?)?;
                for index in 0..metadata.material_count() {
                    let name = metadata.material_name(index);
                    let detail = name.map_or_else(
                        || "glTF material index".into(),
                        |name| format!("glTF material {name}"),
                    );
                    slots.insert(format!("#{index}"), detail);
                    if let Some(name) = name {
                        let selector = if name.starts_with('#') || name.starts_with("name:") {
                            format!("name:{name}")
                        } else {
                            name.to_owned()
                        };
                        slots
                            .entry(selector)
                            .or_insert_with(|| format!("glTF material #{index}"));
                    }
                }
                if metadata.uses_default_material() {
                    slots.insert("#default".into(), "glTF implicit material".into());
                }
            }
        }
        slots
    };
    Some(
        slots
            .into_iter()
            .filter(|(name, _)| name.starts_with(prefix))
            .map(|(name, detail)| {
                let escaped = json!(name).to_string();
                json!({"label": name, "kind": 12, "detail": detail,
            "textEdit": {"range": range, "newText": &escaped[1..escaped.len() - 1]}})
            })
            .collect(),
    )
}

fn shader_capture_view(tokens: &[Token<'_>], mut open: usize) -> Option<usize> {
    loop {
        open = enclosing_block(tokens, open)?;
        if tokens.get(open.checked_sub(1)?)?.text == "SceneView3D" {
            return Some(open);
        }
        let mut header = tokens[..open]
            .iter()
            .rev()
            .filter(|token| token.code())
            .take_while(|token| !matches!(token.text, "{" | "}" | ";"));
        if header.any(|token| token.text == "component") {
            break;
        }
    }
    // A component property can supply a material without an enclosing view.
    // Offer targets when the component has one unambiguous declaration site.
    let mut depth = 1;
    let mut view = None;
    for (index, token) in tokens
        .iter()
        .enumerate()
        .skip(open + 1)
        .filter(|(_, token)| token.code())
    {
        match token.text {
            "{" => {
                depth += 1;
                if tokens[index - 1].text == "SceneView3D"
                    && literal_field_value(tokens, index, "scene-captures").is_some()
                {
                    if view.is_some() {
                        return None;
                    }
                    view = Some(index);
                }
            }
            "}" => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
    }
    view
}

fn scene_capture_values(
    source: &Source<'_>,
    tokens: &[Token<'_>],
    view: usize,
    literal: &Token<'_>,
    cursor: usize,
    exclude_row: Option<usize>,
) -> Option<Vec<Value>> {
    if tokens.get(view.checked_sub(1)?)?.text != "SceneView3D" {
        return None;
    }
    let prefix = &literal.text[1..cursor - literal.range.start];
    let range = source.range(
        literal.range.start + 1,
        literal.range.end - usize::from(literal.quoted()),
    );
    let names: BTreeSet<_> = literal_scene_rows(tokens, view, "scene-captures")
        .into_iter()
        .filter(|&row| {
            exclude_row != Some(row)
                && scene_row_field(tokens, row, "enabled").is_none_or(|value| value.text != "false")
        })
        .filter_map(|row| scene_row_string(tokens, row, "name"))
        .filter(|name| !name.is_empty() && name.starts_with(prefix))
        .collect();
    Some(
        names
            .into_iter()
            .map(|name| {
                let escaped = json!(name).to_string();
                json!({"label": name, "kind": 12, "detail": "Named scene capture",
            "textEdit": {"range": range, "newText": &escaped[1..escaped.len() - 1]}})
            })
            .collect(),
    )
}

pub(super) fn scene_effect_buffer_values(
    source: &Source<'_>,
    tokens: &[Token<'_>],
    cursor: usize,
) -> Option<Vec<Value>> {
    let (field, kind, literal) = tokens.windows(3).enumerate().find_map(|(index, window)| {
        let [field, colon, literal] = window else {
            return None;
        };
        (matches!(
            field.text,
            "input"
                | "name"
                | "capture"
                | "scene-color-capture"
                | "scene-depth-capture"
                | "color-source"
                | "depth-source"
                | "stencil-source"
        ) && colon.text == ":"
            && literal.text.starts_with('"')
            && literal.blocks_completion(cursor))
        .then_some((index, field.text, literal))
    })?;
    let row = enclosing_block(tokens, field)?;
    if matches!(kind, "scene-color-capture" | "scene-depth-capture") {
        return scene_capture_values(source, tokens, row, literal, cursor, None);
    }
    let previous_frame = kind == "name"
        && scene_row_field(tokens, row, "previous-frame").is_some_and(|value| value.text == "true");
    let binding_array = |list: usize, property| {
        list.checked_sub(2)
            .is_some_and(|start| tokens[start].text == property && tokens[start + 1].text == ":")
    };
    if matches!(kind, "color-source" | "depth-source" | "stencil-source") {
        let captures = enclosing_array(tokens, row)?;
        if !binding_array(captures, "scene-captures") {
            return None;
        }
        let view = enclosing_block(tokens, captures)?;
        return scene_capture_values(source, tokens, view, literal, cursor, Some(row));
    }
    if kind == "capture"
        && let Some(textures) = enclosing_array(tokens, row)
        && binding_array(textures, "shader-textures")
    {
        let view = shader_capture_view(tokens, textures)?;
        return scene_capture_values(source, tokens, view, literal, cursor, None);
    }
    if kind == "capture"
        && let Some(textures) = enclosing_array(tokens, row)
        && binding_array(textures, "augment-textures")
    {
        let capture = enclosing_block(tokens, textures)?;
        let captures = enclosing_array(tokens, capture)?;
        if !binding_array(captures, "scene-captures") {
            return None;
        }
        let view = enclosing_block(tokens, captures)?;
        return scene_capture_values(source, tokens, view, literal, cursor, Some(capture));
    }
    let pass = if kind != "input" {
        let buffers = enclosing_array(tokens, row)?;
        if !binding_array(buffers, "buffers") {
            return None;
        }
        enclosing_block(tokens, buffers)?
    } else {
        row
    };
    let list = enclosing_array(tokens, pass)?;
    if !binding_array(list, "scene-effects") {
        return None;
    }
    if kind == "capture" {
        let view = enclosing_block(tokens, list)?;
        return scene_capture_values(source, tokens, view, literal, cursor, None);
    }
    let prefix = &literal.text[1..cursor - literal.range.start];
    let range = source.range(
        literal.range.start + 1,
        literal.range.end - usize::from(literal.quoted()),
    );
    let mut depth = 0;
    let mut names = BTreeSet::new();
    let end = if previous_frame { tokens.len() } else { pass };
    for (offset, token) in tokens[list + 1..end]
        .iter()
        .enumerate()
        .filter(|(_, token)| token.code())
    {
        match token.text {
            "{" => {
                let output = list + 1 + offset;
                if depth == 0
                    && (!previous_frame
                        || scene_row_field(tokens, output, "history")
                            .is_some_and(|value| value.text == "true"))
                    && let Some(name) = scene_row_string(tokens, output, "output")
                    && !name.is_empty()
                    && name.starts_with(prefix)
                {
                    names.insert(name);
                }
                depth += 1;
            }
            "[" | "(" => depth += 1,
            "]" | "}" | ")" if depth == 0 => break,
            "]" | "}" | ")" => depth -= 1,
            _ => {}
        }
    }
    Some(
        names
            .into_iter()
            .map(|name| {
                let escaped = json!(name).to_string();
                let detail = if previous_frame {
                    "Previous frame scene effect buffer"
                } else {
                    "Earlier scene effect buffer"
                };
                json!({"label": name, "kind": 12, "detail": detail,
            "textEdit": {"range": range, "newText": &escaped[1..escaped.len() - 1]}})
            })
            .collect(),
    )
}

pub(super) fn scene_item_texture_values(
    source: &Source<'_>,
    tokens: &[Token<'_>],
    cursor: usize,
    text_len: usize,
) -> Option<Vec<Value>> {
    let (field, literal) = tokens.windows(3).enumerate().find_map(|(index, window)| {
        let [field, colon, literal] = window else {
            return None;
        };
        ((field.text == "base-item"
            || field.text == "texture-input"
            || field.text == "source"
                && enclosing_block(tokens, index)
                    .and_then(|block| tokens.get(block.checked_sub(3)?..block))
                    .is_some_and(|prefix| prefix[0].text == "base-item" && prefix[1].text == ":")
            || field.text == "item" && in_shader_texture_inputs(tokens, index))
            && colon.text == ":"
            && literal.text.starts_with('"')
            && literal.blocks_completion(cursor))
        .then_some((field, literal))
    })?;
    let code: Vec<_> = tokens.iter().filter(|token| token.code()).collect();
    let index = Index::new(&code, text_len);
    if field.text == "texture-input"
        && !matches!(
            index.scopes[index.scope_at(cursor)].role,
            Role::Element { kind: "Image", .. }
        )
    {
        return None;
    }
    let component = index.component(index.scope_at(cursor))?;
    let prefix = &literal.text[1..cursor - literal.range.start];
    let range = source.range(
        literal.range.start + 1,
        literal.range.end - usize::from(literal.quoted()),
    );
    let names: BTreeSet<_> = tokens
        .windows(3)
        .filter_map(|window| {
            let [field, colon, value] = window else {
                return None;
            };
            if field.text != "texture-source" || colon.text != ":" || !value.quoted() {
                return None;
            }
            let scope = index.scope_at(field.range.start);
            if index.component(scope) != Some(component)
                || !matches!(
                    index.scopes[scope].role,
                    Role::Element {
                        kind: "Rectangle" | "SceneView3D",
                        ..
                    }
                )
            {
                return None;
            }
            serde_json::from_str::<String>(value.text)
                .ok()
                .filter(|name| !name.is_empty() && name.starts_with(prefix))
        })
        .collect();
    Some(
        names
            .into_iter()
            .map(|name| {
                let escaped = json!(name).to_string();
                json!({"label": name, "kind": 12, "detail": "Live item texture source",
            "textEdit": {"range": range, "newText": &escaped[1..escaped.len() - 1]}})
            })
            .collect(),
    )
}

fn in_shader_texture_inputs(tokens: &[Token<'_>], field: usize) -> bool {
    enclosing_block(tokens, field)
        .and_then(|row| enclosing_array(tokens, row))
        .and_then(|array| tokens.get(array.checked_sub(2)?..array))
        .is_some_and(|prefix| {
            matches!(
                prefix[0].text,
                "buffers" | "shader-textures" | "augment-textures"
            ) && prefix[1].text == ":"
        })
}

pub(super) fn scene_camera_values(
    source: &Source<'_>,
    tokens: &[Token<'_>],
    cursor: usize,
    workspace: Option<&Workspace<'_>>,
) -> Option<Vec<Value>> {
    let (field, literal) = tokens.windows(3).enumerate().find_map(|(index, window)| {
        let [field, colon, literal] = window else {
            return None;
        };
        (field.text == "camera"
            && colon.text == ":"
            && literal.text.starts_with('"')
            && literal.blocks_completion(cursor))
        .then_some((index, literal))
    })?;
    let container = enclosing_block(tokens, field)?;
    let view = if tokens.get(container.checked_sub(1)?)?.text == "SceneView3D" {
        container
    } else {
        let captures = enclosing_array(tokens, container)?;
        let [property, colon] = tokens.get(captures.checked_sub(2)?..captures)? else {
            return None;
        };
        if property.text != "scene-captures" || colon.text != ":" {
            return None;
        }
        let view = enclosing_block(tokens, captures)?;
        if tokens.get(view.checked_sub(1)?)?.text != "SceneView3D" {
            return None;
        }
        view
    };
    let prefix = &literal.text[1..cursor - literal.range.start];
    if !prefix.contains('/') {
        let range = source.range(
            literal.range.start + 1,
            literal.range.end - usize::from(literal.quoted()),
        );
        let cameras: BTreeSet<_> = literal_scene_rows(tokens, view, "model")
            .into_iter()
            .filter(|&row| {
                scene_row_string(tokens, row, "camera-kind").is_some_and(|kind| {
                    matches!(kind.as_str(), "perspective" | "orthographic" | "frustum")
                })
            })
            .filter_map(|row| scene_row_string(tokens, row, "id"))
            .filter(|name| !name.is_empty() && name.starts_with(prefix))
            .collect();
        return Some(
            cameras
                .into_iter()
                .map(|name| {
                    let escaped = json!(name).to_string();
                    json!({"label": name, "kind": 12, "detail": "Scene camera",
                "textEdit": {"range": range, "newText": &escaped[1..escaped.len() - 1]}})
                })
                .collect(),
        );
    }
    let workspace = workspace?;
    let (owner, _) = prefix.rsplit_once('/')?;
    let mut open = view;
    let component = loop {
        open = enclosing_block(tokens, open)?;
        let mut header = tokens[..open]
            .iter()
            .rev()
            .filter(|token| token.code())
            .take_while(|token| !matches!(token.text, "{" | "}" | ";"));
        if header.any(|token| token.text == "component") {
            break open;
        }
    };
    let mut depth = 1usize;
    let tail = &tokens[component + 1..];
    let end = tail
        .iter()
        .position(|token| {
            if token.code() {
                match token.text {
                    "{" => depth += 1,
                    "}" => depth -= 1,
                    _ => {}
                }
            }
            depth == 0
        })
        .unwrap_or(tail.len());
    let assets: BTreeSet<_> = tail[..end]
        .windows(3)
        .enumerate()
        .filter_map(|(offset, window)| {
            let [id, colon, name] = window else {
                return None;
            };
            if id.text != "id"
                || colon.text != ":"
                || !name.quoted()
                || serde_json::from_str::<String>(name.text).ok().as_deref() != Some(owner)
            {
                return None;
            }
            scene_row_asset(tokens, enclosing_block(tokens, component + 1 + offset)?)
        })
        .collect();
    if assets.len() != 1 {
        return None;
    }
    let metadata = scene_metadata(assets.first()?, workspace)?;
    let range = source.range(
        literal.range.start + 1,
        literal.range.end - usize::from(literal.quoted()),
    );
    let range = &range;
    Some(metadata.camera_nodes().into_iter().flat_map(|(index, kind, alias)| {
        [Some(format!("{owner}/#{index}")), alias.map(|alias| format!("{owner}/{alias}"))]
            .into_iter().flatten().filter(|label| label.starts_with(prefix)).map(move |label| {
                let replacement = json!(label).to_string();
                json!({"label": label, "kind": 12, "detail": format!("glTF {kind} camera"),
                    "textEdit": {"range": range, "newText": &replacement[1..replacement.len() - 1]}})
            })
    }).collect())
}

fn in_capture_record(tokens: &[Token<'_>], position: usize, name: &str) -> bool {
    let Some(record) =
        enclosing_block(tokens, position).and_then(|open| tokens.get(open.checked_sub(1)?))
    else {
        return false;
    };
    tokens
        .windows(3)
        .any(|field| field[0].text == name && field[1].text == ":" && field[2].text == record.text)
}

fn in_collision_shapes(tokens: &[Token<'_>], position: usize) -> bool {
    let Some(record) =
        enclosing_block(tokens, position).and_then(|open| tokens.get(open.checked_sub(1)?))
    else {
        return false;
    };
    tokens.windows(4).any(|field| {
        field[0].text == "collision-shapes"
            && field[1].text == ":"
            && field[2].text == "["
            && field[3].text == record.text
    })
}

fn physics_shape_argument(tokens: &[Token<'_>], position: usize) -> Option<PhysicsFunction> {
    use xrack_parser::builtins::{self, FunctionKind};
    let list = enclosing_array(tokens, enclosing_block(tokens, position)?)?;
    super::symbols::active_calls(tokens[..list].iter()).find_map(|(callee, argument)| {
        let [receiver, dot, name] = tokens.get(callee.checked_sub(2)?..=callee)? else {
            return None;
        };
        if argument != 3 || receiver.text != "Physics3D" || dot.text != "." {
            return None;
        }
        let FunctionKind::Physics(kind) = builtins::function(receiver.text, name.text)?.kind else {
            return None;
        };
        kind.uses_shape_list().then_some(kind)
    })
}

pub(super) fn scene_string_options(
    source: &Source<'_>,
    tokens: &[Token<'_>],
    cursor: usize,
    text_len: usize,
) -> Option<Vec<Value>> {
    let end = tokens.partition_point(|token| token.range.start < cursor);
    let [field, colon, literal] = tokens.get(end.checked_sub(3)?..end)? else {
        return None;
    };
    let physics = if field.text == "shape" {
        physics_shape_argument(tokens, end - 3)
    } else {
        None
    };
    let options = match field.text {
        "mesh" | "emitter-mesh" | "particle-model" => {
            xrack_parser::builtins::SCENE_PRIMITIVE_MESHES
        }
        "shape" if physics.is_some() || in_collision_shapes(tokens, end - 3) => {
            xrack_parser::builtins::SCENE_COLLISION_SHAPES
        }
        "shape" => xrack_parser::builtins::SCENE_PARAMETRIC_SHAPES,
        "body-kind" => xrack_parser::builtins::SCENE_PHYSICS_BODY_KINDS,
        "joint-kind" => xrack_parser::builtins::SCENE_PHYSICS_JOINT_KINDS,
        "camera-kind" => xrack_parser::builtins::SCENE_CAMERA_KINDS,
        "camera-clip-space" => xrack_parser::builtins::SCENE_CAMERA_CLIP_SPACES,
        "material-workflow" => xrack_parser::builtins::SCENE_MATERIAL_WORKFLOWS,
        "alpha-mode" => xrack_parser::builtins::SCENE_ALPHA_MODES,
        "color-model" => xrack_parser::builtins::SCENE_RANDOM_COLOR_MODELS,
        "blend-mode" if in_capture_record(tokens, end - 3, "pipeline-state") => {
            xrack_parser::builtins::SCENE_CAPTURE_BLENDS
        }
        "blend-mode" => xrack_parser::builtins::SCENE_MATERIAL_BLEND_MODES,
        "compare" if in_capture_record(tokens, end - 3, "stencil") => {
            xrack_parser::builtins::SCENE_CAPTURE_DEPTH_COMPARES
        }
        "fail" | "depth-fail" | "pass" if in_capture_record(tokens, end - 3, "stencil") => {
            xrack_parser::builtins::SCENE_CAPTURE_STENCIL_OPS
        }
        "color-operation" | "alpha-operation"
            if in_capture_record(tokens, end - 3, "target-blend") =>
        {
            xrack_parser::builtins::SCENE_CAPTURE_BLEND_OPERATIONS
        }
        "source-color" | "destination-color" | "source-alpha" | "destination-alpha"
            if in_capture_record(tokens, end - 3, "target-blend") =>
        {
            xrack_parser::builtins::SCENE_CAPTURE_BLEND_FACTORS
        }
        "depth-compare" => xrack_parser::builtins::SCENE_CAPTURE_DEPTH_COMPARES,
        "polygon-mode" => xrack_parser::builtins::SCENE_CAPTURE_POLYGON_MODES,
        "depth-draw-mode" => xrack_parser::builtins::SCENE_DEPTH_DRAW_MODES,
        "cull-mode" => xrack_parser::builtins::SCENE_CULL_MODES,
        "wrap-u" | "wrap-v" => xrack_parser::builtins::SCENE_TEXTURE_WRAP,
        "min-filter" | "mag-filter" => xrack_parser::builtins::SCENE_TEXTURE_FILTER,
        "filter" if in_shader_texture_inputs(tokens, end - 3) => {
            xrack_parser::builtins::SCENE_TEXTURE_FILTER
        }
        "attachment" if in_shader_texture_inputs(tokens, end - 3) => {
            xrack_parser::builtins::SCENE_CAPTURE_ATTACHMENTS
        }
        "color-space" if in_shader_texture_inputs(tokens, end - 3) => {
            xrack_parser::builtins::SCENE_EFFECT_COLOR_SPACES
        }
        "renderables"
            if enclosing_block(tokens, end - 3)
                .and_then(|row| enclosing_array(tokens, row))
                .and_then(|list| tokens.get(list.checked_sub(2)?..list))
                .is_some_and(|prefix| {
                    prefix[0].text == "scene-captures" && prefix[1].text == ":"
                }) =>
        {
            xrack_parser::builtins::SCENE_CAPTURE_RENDERABLES
        }
        "output-format"
        | "color-format"
        | "material-data-format"
        | "material-data-1-format"
        | "material-data-2-format" => xrack_parser::builtins::SCENE_EFFECT_FORMATS,
        "mip-filter" => xrack_parser::builtins::SCENE_TEXTURE_MIPS,
        "refresh-mode" => xrack_parser::builtins::REFLECTION_REFRESH_MODES,
        "time-slicing" => xrack_parser::builtins::REFLECTION_TIME_SLICING,
        "shadow-filter" => xrack_parser::builtins::SCENE_SHADOW_FILTERS,
        "bake-mode" => xrack_parser::builtins::SCENE_LIGHT_BAKE_MODES,
        "lightmap-mode" => xrack_parser::builtins::LIGHTMAP_MODES,
        "bake-role" => xrack_parser::builtins::SCENE_BAKE_ROLES,
        "fog-shape" => xrack_parser::builtins::FOG_VOLUME_SHAPES,
        "sprite-playback" => xrack_parser::builtins::PARTICLE_SPRITE_PLAYBACK,
        "particle-blend-mode" => xrack_parser::builtins::PARTICLE_BLEND_MODES,
        "emitter-shape" => xrack_parser::builtins::PARTICLE_EMITTER_SHAPES,
        "affector-kind" => xrack_parser::builtins::PARTICLE_AFFECTOR_KINDS,
        "velocity-mode" => xrack_parser::builtins::PARTICLE_VELOCITY_MODES,
        "temporal-aa-mode" => xrack_parser::builtins::TEMPORAL_AA_MODES,
        "render-mode" => xrack_parser::builtins::SCENE_RENDER_MODES,
        "render-format" => xrack_parser::builtins::SCENE_RENDER_FORMATS,
        "oit-method" => xrack_parser::builtins::SCENE_OIT_METHODS,
        "material-debug" => xrack_parser::builtins::SCENE_MATERIAL_DEBUG_MODES,
        "shadow-map-precision" => xrack_parser::builtins::SCENE_SHADOW_PRECISIONS,
        "render-overrides" => xrack_parser::builtins::SCENE_RENDER_OVERRIDES,
        property if xrack_parser::builtins::SCENE_VERTEX_MASK_FIELDS.contains(&property) => {
            xrack_parser::builtins::SCENE_VERTEX_MASK_CHANNELS
        }
        property => {
            let (_, rgb) = xrack_parser::builtins::SCENE_MATERIAL_CHANNEL_FIELDS
                .iter()
                .find(|(name, _)| *name == property)?;
            if *rgb {
                xrack_parser::builtins::COLOR_TEXTURE_CHANNELS
            } else {
                xrack_parser::builtins::SCALAR_TEXTURE_CHANNELS
            }
        }
    };
    if colon.text != ":" || !literal.text.starts_with('"') || !literal.blocks_completion(cursor) {
        return None;
    }
    let code: Vec<_> = tokens.iter().filter(|token| token.code()).collect();
    let index = Index::new(&code, text_len);
    let role = index.scopes[index.scope_at(cursor)].role;
    if !(if matches!(
        field.text,
        "temporal-aa-mode"
            | "render-mode"
            | "render-format"
            | "render-overrides"
            | "material-debug"
            | "oit-method"
    ) {
        matches!(
            role,
            Role::Element {
                kind: "SceneView3D",
                ..
            }
        )
    } else {
        matches!(role, Role::RecordValue(_))
            || field.text == "shadow-map-precision"
                && matches!(
                    role,
                    Role::Element {
                        kind: "SceneView3D",
                        ..
                    }
                )
    }) {
        return None;
    }
    let inherit_precision =
        field.text == "shadow-map-precision" && matches!(role, Role::RecordValue(_));
    let prefix = &literal.text[1..cursor - literal.range.start];
    let range = source.range(
        literal.range.start + 1,
        literal.range.end - usize::from(literal.quoted()),
    );
    Some(
        options
            .iter()
            .copied()
            .filter(|(kind, _)| physics != Some(PhysicsFunction::AddTrigger) || *kind != "mesh")
            .chain(
                physics
                    .is_some()
                    .then_some(("convex-mesh", "Convex hull of the mesh resource")),
            )
            .chain(
                physics
                    .is_some_and(|kind| kind != PhysicsFunction::AddTrigger)
                    .then_some((
                        "heightfield",
                        "Grid of heights from a heightfield PNG for a fixed or kinematic body",
                    )),
            )
            .chain(
                (physics == Some(PhysicsFunction::AddBody))
                    .then_some(("plane", "Fixed plane, the only shape of a zero-mass body")),
            )
            .chain(inherit_precision.then_some(("inherit", "Use the viewport's shadow precision")))
            .filter(|(kind, _)| kind.starts_with(prefix))
            .map(|(kind, detail)| {
                json!({
                    "label": kind,
                    "kind": 12,
                    "detail": detail,
                    "textEdit": {"range": range, "newText": kind},
                })
            })
            .collect(),
    )
}

fn scene_metadata(
    asset: &str,
    workspace: &Workspace<'_>,
) -> Option<xrack_parser::gltf_metadata::GltfMetadata> {
    let relative = Path::new(asset);
    if relative.is_absolute()
        || asset.contains(':')
        || !matches_asset_extension(relative, &["glb", "gltf"])
    {
        return None;
    }
    let path = workspace.entry.parent()?.join(relative);
    let metadata = fs::metadata(&path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let size = usize::try_from(metadata.len()).ok()?;
    let mut file = fs::File::open(path).ok()?;
    let mut header = [0; 20];
    let header = &mut header[..size.min(20)];
    file.read_exact(header).ok()?;
    let json = xrack_parser::gltf_metadata::json_range(header, size)?;
    // Large GLBs can still have a small JSON chunk. The editor only reads that chunk.
    const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;
    if json.len() > MAX_JSON_BYTES {
        return None;
    }
    file.seek(SeekFrom::Start(json.start as u64)).ok()?;
    let mut bytes = vec![0; json.len()];
    file.read_exact(&mut bytes).ok()?;
    serde_json::from_slice(&bytes).ok()
}

struct ObjMetadata {
    vertices: usize,
    faces: usize,
    default_slot: bool,
    names: Vec<String>,
}

impl ObjMetadata {
    fn slots(&self) -> impl Iterator<Item = (Cow<'_, str>, Option<&str>)> {
        self.default_slot
            .then_some((Cow::Borrowed("#default"), None))
            .into_iter()
            .chain(
                self.names
                    .iter()
                    .enumerate()
                    .map(|(index, name)| (Cow::Owned(format!("#{index}")), Some(name.as_str()))),
            )
    }
}

fn obj_metadata(asset: &str, workspace: &Workspace<'_>) -> Option<ObjMetadata> {
    const MAX_BYTES: u64 = 16 * 1024 * 1024;
    let relative = Path::new(asset);
    if relative.is_absolute() || asset.contains(':') {
        return None;
    }
    let path = workspace.entry.parent()?.join(relative);
    let metadata = fs::metadata(&path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        return None;
    }
    let mut text = String::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_BYTES + 1)
        .read_to_string(&mut text)
        .ok()?;
    if text.len() as u64 > MAX_BYTES {
        return None;
    }
    let mut current = None;
    let mut seen = BTreeSet::new();
    let mut info = ObjMetadata {
        vertices: 0,
        faces: 0,
        default_slot: false,
        names: Vec::new(),
    };
    for (_, command, value) in xrack_parser::wavefront::directives(&text) {
        match command {
            "v" => info.vertices += 1,
            "usemtl" => current = Some(value).filter(|value| !value.is_empty()),
            "f" => {
                info.faces += 1;
                if let Some(name) = current {
                    if info.names.len() < 1024 && seen.insert(name) {
                        info.names.push(name.to_owned());
                    }
                } else {
                    info.default_slot = true;
                }
            }
            _ => {}
        }
    }
    Some(info)
}

fn heightfield_description(asset: &str, workspace: &Workspace<'_>) -> Option<String> {
    let relative = Path::new(asset);
    if relative.is_absolute() || asset.contains(':') {
        return None;
    }
    let path = workspace.entry.parent()?.join(relative);
    if !fs::metadata(&path).ok()?.is_file() {
        return None;
    }
    let mut header = [0; 29];
    fs::File::open(path).ok()?.read_exact(&mut header).ok()?;
    if header[..16] != *b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR"
        || header[25] != 0
        || !matches!(header[24], 8 | 16)
    {
        return None;
    }
    let width = u32::from_be_bytes(header[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(header[20..24].try_into().ok()?);
    let depth = header[24];
    Some(format!(
        "PNG heightfield · {width} × {height} samples · {depth}-bit grayscale\n\nImage columns map to +X and rows to +Z. Black is Y = 0 and white is Y = 1 before the model transform."
    ))
}

pub(super) fn mesh_asset_hover(
    source: &Source<'_>,
    tokens: &[Token<'_>],
    cursor: usize,
    workspace: &Workspace<'_>,
) -> Option<Value> {
    use std::fmt::Write;

    let (url, path) = tokens.windows(4).find_map(|tokens| {
        let [url, open, path, close] = tokens else {
            return None;
        };
        (url.text == "@mesh-url"
            && open.text == "("
            && path.quoted()
            && matches!(close.text, ")" | ",")
            && (url.range.contains(&cursor) || path.range.contains(&cursor)))
        .then_some((url, path))
    })?;
    let asset: String = serde_json::from_str(path.text).ok()?;
    if matches_asset_extension(Path::new(&asset), &[HEIGHTFIELD_EXTENSION]) {
        return Some(json!({
            "contents": {"kind": "plaintext", "value": heightfield_description(&asset, workspace)?},
            "range": source.range(url.range.start, path.range.end),
        }));
    }
    if matches_asset_extension(Path::new(&asset), &[OBJ_EXTENSION]) {
        let info = obj_metadata(&asset, workspace)?;
        let mut message = format!(
            "Wavefront OBJ · {} vertices · {} faces",
            info.vertices, info.faces
        );
        for (slot, name) in info.slots().take(16) {
            let _ = write!(message, "\n{slot}  {}", name.unwrap_or("implicit material"));
        }
        if info.names.len() + usize::from(info.default_slot) > 16 {
            message.push_str("\n…");
        }
        return Some(json!({
            "contents": {"kind": "plaintext", "value": message},
            "range": source.range(url.range.start, path.range.end),
        }));
    }
    let metadata = scene_metadata(&asset, workspace)?;
    let mut meshes = metadata.mesh_morph_counts().peekable();
    let mut message = String::new();
    let mut groups = metadata.gpu_instance_groups().peekable();
    if groups.peek().is_some() {
        message.push_str("glTF GPU instancing (EXT_mesh_gpu_instancing)");
        for (index, name, count, attributes) in groups.by_ref().take(8) {
            let _ = write!(message, "\nNode #{index}");
            if let Some(name) =
                name.filter(|name| name.len() <= 80 && !name.chars().any(char::is_control))
            {
                let _ = write!(message, "  {name}");
            }
            if let Some(count) = count {
                let _ = write!(message, "  {count} instances");
            }
            let color = [INSTANCE_COLOR_ATTRIBUTE, INSTANCE_COLOR_ALIAS]
                .into_iter()
                .find(|name| attributes.contains_key(*name));
            if let Some(color) = color {
                let _ = write!(message, "\n  {color} → instance tint");
            }
            if attributes.contains_key(INSTANCE_DATA_ATTRIBUTE) {
                let _ = write!(
                    message,
                    "\n  {INSTANCE_DATA_ATTRIBUTE} → WGSL input.instance_data"
                );
            }
        }
        if groups.peek().is_some() {
            message.push_str("\n…");
        }
    }
    if meshes.peek().is_some() {
        if !message.is_empty() {
            message.push_str("\n\n");
        }
        message.push_str("glTF morph targets for morph-weights");
        for (mesh, count) in meshes.by_ref().take(8) {
            let names = if count <= 32 {
                metadata.morph_target_names(mesh, count)
            } else {
                Vec::new()
            };
            let _ = write!(message, "\n\nMesh #{mesh}");
            for index in 0..count.min(32) {
                let _ = write!(message, "\ntarget-{index}");
                if let Some(alias) = names.get(index)
                    && name(alias)
                    && alias.len() <= 80
                    && alias
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                {
                    let _ = write!(message, "  {alias}");
                }
            }
            if count > 32 {
                message.push_str("\n…");
            }
        }
        if meshes.peek().is_some() {
            message.push_str("\n…");
        }
    }
    if (0..metadata.material_count()).any(|index| metadata.material_name(index).is_some()) {
        if !message.is_empty() {
            message.push_str("\n\n");
        }
        message.push_str("glTF material slots");
        for index in 0..metadata.material_count().min(64) {
            let _ = write!(message, "\n#{index}");
            if let Some(name) = metadata.material_name(index) {
                let _ = write!(message, "  {name}");
            }
        }
        if metadata.uses_default_material() {
            message.push_str("\n#default  implicit material");
        }
    }
    let variants = metadata.material_variant_names();
    if variants.len() != 0 {
        if !message.is_empty() {
            message.push_str("\n\n");
        }
        message.push_str("glTF material variants (material-variant)");
        for (index, name) in variants.take(64).enumerate() {
            let _ = write!(message, "\n#{index}");
            if name.len() <= 80 && !name.chars().any(char::is_control) {
                let _ = write!(message, "  {name}");
            }
        }
    }
    let mut extensions = metadata.material_extensions().peekable();
    if extensions.peek().is_some() {
        if !message.is_empty() {
            message.push_str("\n\n");
        }
        message.push_str("glTF material extensions");
        for extension in extensions {
            let _ = write!(message, "\n{extension}");
        }
    }
    if metadata.uses_texture_transform() {
        if !message.is_empty() {
            message.push_str("\n\n");
        }
        message.push_str(
            "glTF texture coordinates\nKHR_texture_transform (offset, rotation, scale, UV set)",
        );
    }
    let cameras = metadata.camera_nodes();
    if !cameras.is_empty() {
        if !message.is_empty() {
            message.push_str("\n\n");
        }
        message.push_str("glTF camera nodes (use row-id/selector)");
        for &(index, kind, alias) in cameras.iter().take(8) {
            let _ = write!(message, "\n#{index}  {kind}");
            if let Some(alias) = alias.filter(|alias| alias.len() <= 80) {
                let _ = write!(message, "  {alias}");
            }
        }
        if cameras.len() > 8 {
            message.push_str("\n…");
        }
    }
    let mut lights = metadata.punctual_lights().peekable();
    if lights.peek().is_some() {
        if !message.is_empty() {
            message.push_str("\n\n");
        }
        message.push_str("glTF punctual lights");
        for (index, kind, name) in lights.by_ref().take(8) {
            let _ = write!(message, "\nLight #{index}  {kind}");
            if let Some(name) = name.filter(|name| {
                !name.is_empty() && name.len() <= 80 && !name.chars().any(char::is_control)
            }) {
                let _ = write!(message, "  {name}");
            }
        }
        if lights.peek().is_some() {
            message.push_str("\n…");
        }
    }
    let mut filters = metadata.texture_min_filters().peekable();
    if filters.peek().is_some() {
        if !message.is_empty() {
            message.push_str("\n\n");
        }
        message.push_str("glTF texture minification");
        for (index, filter) in filters.by_ref().take(8) {
            let _ = write!(message, "\nTexture #{index}  {filter}");
        }
        if filters.peek().is_some() {
            message.push_str("\n…");
        }
    }
    if message.is_empty() {
        return None;
    }
    let hovered = if url.range.contains(&cursor) {
        url
    } else {
        path
    };
    Some(json!({
        "contents": { "kind": "plaintext", "value": message },
        "range": source.range(hovered.range.start, hovered.range.end),
    }))
}

pub(super) fn scene_asset_paths(
    source: &Source<'_>,
    tokens: &[Token<'_>],
    cursor: usize,
    workspace: &Workspace<'_>,
) -> Option<Vec<Value>> {
    let (extensions, path) = tokens.windows(3).enumerate().find_map(|(index, window)| {
        let [_, open, path] = window else {
            return None;
        };
        let extensions = scene_asset_extensions(tokens, index)?;
        (open.text == "(" && path.text.starts_with('"') && path.blocks_completion(cursor))
            .then_some((extensions, path))
    })?;
    let prefix = &path.text[1..cursor - path.range.start];
    let (directory, name) = prefix.rsplit_once('/').unwrap_or(("", prefix));
    if prefix.starts_with('/') || prefix.contains([':', '\\']) {
        return Some(Vec::new());
    }
    let base = workspace.entry.parent()?;
    let paths = fs::read_dir(base.join(directory)).ok()?;
    let start = cursor - name.len();
    let end = path.range.end - usize::from(path.quoted());
    let range = source.range(start, end);
    let mut candidates = BTreeMap::new();
    for path in paths.filter_map(Result::ok) {
        let Ok(mut label) = path.file_name().into_string() else {
            continue;
        };
        if !label.starts_with(name) {
            continue;
        }
        let is_folder = path.path().is_dir();
        if is_folder {
            label.push('/');
        } else if !matches_asset_extension(&path.path(), extensions) {
            continue;
        }
        let replacement = json!(label).to_string();
        candidates.insert(
            label.clone(),
            json!({
                "label": label,
                "kind": if is_folder { 19 } else { 17 },
                "textEdit": {"range": range, "newText": &replacement[1..replacement.len() - 1]},
            }),
        );
        if candidates.len() == 256 {
            break;
        }
    }
    Some(candidates.into_values().collect())
}

pub(crate) fn import_definition(
    text: &str,
    position: (usize, usize),
    workspace: &Workspace<'_>,
) -> Option<Value> {
    let cursor = Source::new(text).offset_at(position.0, position.1)?;
    let (_, target) = source_links(text, workspace)
        .into_iter()
        .find(|(range, _)| range.start.saturating_sub(1) <= cursor && cursor <= range.end)?;
    let start = json!({"line": 0, "character": 0});
    Some(json!({"uri": target, "range": {"start": start, "end": start}}))
}

pub(crate) fn file_path(uri: &str) -> Option<PathBuf> {
    let encoded = uri.strip_prefix("file://")?;
    let encoded = encoded
        .strip_prefix("localhost")
        .filter(|path| path.starts_with('/'))
        .unwrap_or(encoded);
    if (!cfg!(windows) && !encoded.starts_with('/')) || encoded.contains(['?', '#']) {
        return None;
    }
    let mut bytes = Vec::with_capacity(encoded.len());
    let mut raw = encoded.as_bytes();
    while let [first, rest @ ..] = raw {
        if *first == b'%' {
            let [high, low, tail @ ..] = rest else {
                return None;
            };
            let digit = |byte| char::from(byte).to_digit(16).map(|digit| digit as u8);
            bytes.push(digit(*high)? * 16 + digit(*low)?);
            raw = tail;
        } else {
            bytes.push(*first);
            raw = rest;
        }
    }
    let path = String::from_utf8(bytes).ok()?;
    if path.contains('\0') {
        return None;
    }
    #[cfg(windows)]
    let path = if let Some(local) = path.strip_prefix('/') {
        local.to_owned()
    } else {
        format!(r"\\{}", path.replace('/', "\\"))
    };
    let path = Path::new(&path);
    path.is_absolute().then(|| path.to_owned())
}
