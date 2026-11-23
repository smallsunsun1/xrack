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
use xrack_parser::builtins::{HEIGHTFIELD_EXTENSION, MESH_SOURCE_EXTENSIONS};

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

pub(super) fn scene_animation_values(
    source: &Source<'_>,
    tokens: &[Token<'_>],
    cursor: usize,
    workspace: &Workspace<'_>,
) -> Option<Vec<Value>> {
    let (field, literal) = tokens.windows(3).enumerate().find_map(|(index, tokens)| {
        let [field, colon, string] = tokens else {
            return None;
        };
        (matches!(field.text, "animation" | "animation-to")
            && colon.text == ":"
            && string.text.starts_with('"')
            && string.blocks_completion(cursor))
        .then_some((index, string))
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
    Some(
        metadata
            .animation_selectors()
            .into_iter()
            .filter(|label| label.starts_with(prefix))
            .map(|label| {
                let replacement = json!(label).to_string();
                json!({
                    "label": label, "kind": 12, "detail": "glTF animation clip",
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

fn scene_row_asset(tokens: &[Token<'_>], open: usize) -> Option<String> {
    let mut depth = 0usize;
    let mut asset = None;
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
                let Some([_, colon, url, paren, path, end]) = tokens.get(start..start + 6) else {
                    continue;
                };
                if colon.text == ":"
                    && url.text == "@mesh-url"
                    && paren.text == "("
                    && matches!(end.text, ")" | ",")
                    && path.quoted()
                {
                    asset = serde_json::from_str::<String>(path.text).ok();
                    break;
                }
            }
            _ => {}
        }
    }
    asset
}

pub(super) fn scene_camera_values(
    source: &Source<'_>,
    tokens: &[Token<'_>],
    cursor: usize,
    workspace: &Workspace<'_>,
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
    let view = enclosing_block(tokens, field)?;
    if tokens.get(view.checked_sub(1)?)?.text != "SceneView3D" {
        return None;
    }
    let prefix = &literal.text[1..cursor - literal.range.start];
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
    let options = match field.text {
        "camera-kind" => xrack_parser::builtins::SCENE_CAMERA_KINDS,
        "refresh-mode" => xrack_parser::builtins::REFLECTION_REFRESH_MODES,
        "time-slicing" => xrack_parser::builtins::REFLECTION_TIME_SLICING,
        "lightmap-mode" => xrack_parser::builtins::LIGHTMAP_MODES,
        "fog-shape" => xrack_parser::builtins::FOG_VOLUME_SHAPES,
        "sprite-playback" => xrack_parser::builtins::PARTICLE_SPRITE_PLAYBACK,
        "particle-blend-mode" => xrack_parser::builtins::PARTICLE_BLEND_MODES,
        "emitter-shape" => xrack_parser::builtins::PARTICLE_EMITTER_SHAPES,
        "affector-kind" => xrack_parser::builtins::PARTICLE_AFFECTOR_KINDS,
        "velocity-mode" => xrack_parser::builtins::PARTICLE_VELOCITY_MODES,
        "temporal-aa-mode" => xrack_parser::builtins::TEMPORAL_AA_MODES,
        _ => return None,
    };
    if colon.text != ":" || !literal.text.starts_with('"') || !literal.blocks_completion(cursor) {
        return None;
    }
    let code: Vec<_> = tokens.iter().filter(|token| token.code()).collect();
    let index = Index::new(&code, text_len);
    let role = index.scopes[index.scope_at(cursor)].role;
    if !(if field.text == "temporal-aa-mode" {
        matches!(
            role,
            Role::Element {
                kind: "SceneView3D",
                ..
            }
        )
    } else {
        matches!(role, Role::RecordValue(_))
    }) {
        return None;
    }
    let prefix = &literal.text[1..cursor - literal.range.start];
    let range = source.range(
        literal.range.start + 1,
        literal.range.end - usize::from(literal.quoted()),
    );
    Some(
        options
            .iter()
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
    let metadata = scene_metadata(&asset, workspace)?;
    let mut meshes = metadata.mesh_morph_counts().peekable();
    let mut message = String::new();
    if meshes.peek().is_some() {
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
