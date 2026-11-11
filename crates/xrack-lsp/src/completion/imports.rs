use crate::completion::{CLASS, Index, Member, Role, STRUCT};
use crate::editor_tokens::{self, Token, name};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    fs,
    ops::Range,
    path::{Path, PathBuf},
};

pub(crate) struct Workspace<'a> {
    pub(super) entry: PathBuf,
    open: BTreeMap<PathBuf, (&'a str, &'a str)>,
}

impl<'a> Workspace<'a> {
    pub(crate) fn new(entry: &str, open: impl Iterator<Item = (&'a str, &'a str)>) -> Option<Self> {
        Some(Self {
            entry: file_path(entry)?,
            open: open
                .filter_map(|(uri, text)| Some((normalize(&file_path(uri)?), (uri, text))))
                .collect(),
        })
    }

    pub(super) fn imported_members(&self, tokens: &[Token<'_>]) -> BTreeMap<String, Vec<Member>> {
        self.interfaces(&import_targets(&self.entry, tokens), &mut BTreeMap::new())
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

    pub(super) fn load(&self, path: &Path) -> Option<Cow<'_, str>> {
        if let Some(&(_, text)) = self.open.get(path) {
            return Some(Cow::Borrowed(text));
        }
        fs::read_to_string(path).ok().map(Cow::Owned)
    }

    pub(super) fn uri(&self, path: &Path) -> Option<String> {
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
        &self,
        path: &Path,
        modules: &'m mut BTreeMap<PathBuf, Module>,
    ) -> &'m Module {
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

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Target {
    pub(super) file: PathBuf,
    pub(super) name: String,
}

#[derive(Default)]
pub(super) struct Module {
    pub(super) components: BTreeMap<String, Component>,
    pub(super) records: BTreeMap<String, Record>,
    pub(super) imports: BTreeMap<String, Target>,
}

pub(super) struct Component {
    base: String,
    pub(super) exported: bool,
    pub(super) range: Range<usize>,
    members: Vec<Member>,
}

pub(super) struct Record {
    pub(super) exported: bool,
    pub(super) range: Range<usize>,
    pub(super) fields: Vec<Member>,
}

impl Module {
    fn new(path: &Path, text: &str) -> Self {
        let tokens = editor_tokens::scan(text);
        let code: Vec<_> = tokens.iter().filter(|t| t.code()).collect();
        let index = Index::new(&code, text.len());
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
            imports: import_targets(path, &tokens),
        }
    }
}

pub(super) fn import_targets(path: &Path, tokens: &[Token<'_>]) -> BTreeMap<String, Target> {
    let code: Vec<_> = tokens.iter().filter(|t| t.code() || t.quoted()).collect();
    let mut imports = BTreeMap::new();
    for clause in named_imports(&code) {
        let Some(file) = module_path(path, clause.source.text) else {
            continue;
        };
        for binding in clause.names.split(|t| t.text == ",") {
            let (original, alias) = match binding {
                [original] => (original.text, original.text),
                [original, as_token, alias] if as_token.text == "as" => (original.text, alias.text),
                _ => continue,
            };
            if name(original) && name(alias) {
                imports.entry(alias.into()).or_insert_with(|| Target {
                    file: file.clone(),
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

pub(super) fn named_imports<'a, 'source>(
    code: &'a [&'a Token<'source>],
) -> impl Iterator<Item = ImportClause<'a, 'source>> {
    let mut rest = code;
    // Imports precede declarations, and a font import may occur between named imports.
    std::iter::from_fn(move || {
        while let [keyword, open, tail @ ..] = rest {
            if keyword.text != "import" {
                break;
            }
            if open.quoted() && tail.first().is_some_and(|t| t.text == ";") {
                rest = &tail[1..];
                continue;
            }
            if open.text != "{" {
                break;
            }
            let close = tail.iter().position(|t| t.text == "}")?;
            let [from, source, next @ ..] = &tail[close + 1..] else {
                break;
            };
            if from.text != "from" || !source.quoted() {
                break;
            }
            rest = if next.first().is_some_and(|t| t.text == ";") {
                &next[1..]
            } else {
                &[]
            };
            return Some(ImportClause {
                body: open.range.end..tail[close].range.start,
                names: &tail[..close],
                source,
            });
        }
        None
    })
}

pub(super) fn module_path(path: &Path, source: &str) -> Option<PathBuf> {
    let relative: PathBuf = serde_json::from_str(source).ok()?;
    if relative.is_absolute() || relative.extension().is_none_or(|ext| ext != "xr") {
        return None;
    }
    Some(path.parent()?.join(relative))
}

fn file_path(uri: &str) -> Option<PathBuf> {
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

pub(super) fn normalize(path: &Path) -> PathBuf {
    if let Ok(file) = path.canonicalize() {
        return file;
    }
    // An open editor can supply a new file that is not on disk yet.
    if let Some((parent, file)) = path.parent().zip(path.file_name())
        && let Ok(parent) = parent.canonicalize()
    {
        return parent.join(file);
    }
    path.to_owned()
}
