use crate::completion::{Index, Member, Role};
use crate::editor_tokens::{self, Token, name};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

pub(crate) struct Workspace<'a> {
    entry: PathBuf,
    open: BTreeMap<PathBuf, &'a str>,
}

impl<'a> Workspace<'a> {
    pub(crate) fn new(entry: &str, open: impl Iterator<Item = (&'a str, &'a str)>) -> Option<Self> {
        Some(Self {
            entry: file_path(entry)?,
            open: open
                .filter_map(|(uri, text)| Some((normalize(&file_path(uri)?), text)))
                .collect(),
        })
    }

    pub(super) fn imported_members(&self, tokens: &[Token<'_>]) -> BTreeMap<String, Vec<Member>> {
        let mut modules = BTreeMap::new();
        import_targets(&self.entry, tokens)
            .into_iter()
            .filter_map(|(alias, target)| {
                self.interface(target, &mut modules)
                    .map(|members| (alias, members))
            })
            .collect()
    }

    fn load(&self, path: &Path) -> Option<Cow<'_, str>> {
        if let Some(&text) = self.open.get(path) {
            return Some(Cow::Borrowed(text));
        }
        fs::read_to_string(path).ok().map(Cow::Owned)
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
            let module = modules.entry(canonical.clone()).or_insert_with(|| {
                self.load(&canonical)
                    .map_or_else(Module::default, |text| Module::new(&target.file, &text))
            });
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
struct Target {
    file: PathBuf,
    name: String,
}

#[derive(Default)]
struct Module {
    components: BTreeMap<String, Component>,
    imports: BTreeMap<String, Target>,
}

struct Component {
    base: String,
    exported: bool,
    members: Vec<Member>,
}

impl Module {
    fn new(path: &Path, text: &str) -> Self {
        let tokens = editor_tokens::scan(text);
        let code: Vec<_> = tokens.iter().filter(|t| t.code()).collect();
        let index = Index::new(&code, text.len());
        let components = index
            .scopes
            .into_iter()
            .filter_map(|scope| {
                let Role::Component {
                    name,
                    base,
                    exported,
                } = scope.role
                else {
                    return None;
                };
                Some((
                    name.into(),
                    Component {
                        base: base.into(),
                        exported,
                        members: scope.members,
                    },
                ))
            })
            .collect();
        Self {
            components,
            imports: import_targets(path, &tokens),
        }
    }
}

fn import_targets(path: &Path, tokens: &[Token<'_>]) -> BTreeMap<String, Target> {
    let Some(directory) = path.parent() else {
        return BTreeMap::new();
    };
    let code: Vec<_> = tokens.iter().filter(|t| t.code() || t.quoted()).collect();
    let mut imports = BTreeMap::new();
    let mut rest = code.as_slice();
    // Imports precede declarations. Scan only that prefix so the rest of the file
    // can contain unfinished expressions or missing braces during completion.
    while let [keyword, open, tail @ ..] = rest {
        if keyword.text != "import" || open.text != "{" {
            break;
        }
        let Some(close) = tail.iter().position(|t| t.text == "}") else {
            break;
        };
        let [from, source, semicolon, next @ ..] = &tail[close + 1..] else {
            break;
        };
        if from.text != "from" || !source.quoted() || semicolon.text != ";" {
            break;
        }
        rest = next;
        let Ok(relative) = serde_json::from_str::<PathBuf>(source.text) else {
            continue;
        };
        if relative.is_absolute() || relative.extension().is_none_or(|ext| ext != "xr") {
            continue;
        }
        let file = directory.join(relative);
        for binding in tail[..close].split(|t| t.text == ",") {
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

fn file_path(uri: &str) -> Option<PathBuf> {
    let encoded = uri.strip_prefix("file://")?;
    let encoded = encoded
        .strip_prefix("localhost")
        .filter(|path| path.starts_with('/'))
        .unwrap_or(encoded);
    if !encoded.starts_with('/') || encoded.contains(['?', '#']) {
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
    let path = path.strip_prefix('/').unwrap_or(&path);
    let path = Path::new(&path);
    path.is_absolute().then(|| path.to_owned())
}

fn normalize(path: &Path) -> PathBuf {
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
