use super::{
    CLASS, Workspace,
    imports::{Module, normalize},
    index_cache::{Cache, disk_text},
    symbols::{Occurrence, duplicate_declaration, occurrences},
};
use crate::{
    analysis::Source,
    editor_tokens,
    server_events::{Cancellation, Event, IndexProgress, Reply},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs, io,
    ops::Range,
    path::{Path, PathBuf},
    sync::{
        Arc,
        mpsc::{self, Sender},
    },
    thread::{self, JoinHandle},
};
use xrack_ir::{modules::resolve_indexed_document, semantic::resolve_symbols};
use xrack_parser::ast::Document;

#[derive(Clone, Eq, Hash, PartialEq, Serialize, Deserialize)]
struct Declaration {
    uri: String,
    range: Range<usize>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Reference {
    range: Range<usize>,
    target: Declaration,
    declaration: bool,
    definition: Declaration,
}

struct IndexedFile {
    text: Arc<str>,
    lines: Vec<usize>,
    references: Vec<Reference>,
    path: Option<PathBuf>,
    semantically_valid: bool,
}

#[derive(Default)]
struct RenameSnapshot {
    files: BTreeMap<String, EditedFile>,
    modules: BTreeMap<PathBuf, Module>,
    parsed: BTreeMap<PathBuf, Option<Document>>,
    uris: BTreeMap<PathBuf, String>,
}

struct EditedFile {
    text: String,
    ranges: Vec<(Range<usize>, Range<usize>)>,
}

impl EditedFile {
    fn new(text: &str, ranges: &[Range<usize>], spelling: &str) -> Self {
        let mut edited = String::new();
        let mut previous = 0;
        let mut mapped = Vec::with_capacity(ranges.len());
        for range in ranges {
            edited.push_str(&text[previous..range.start]);
            let start = edited.len();
            edited.push_str(spelling);
            mapped.push((range.clone(), start..edited.len()));
            previous = range.end;
        }
        edited.push_str(&text[previous..]);
        Self {
            text: edited,
            ranges: mapped,
        }
    }

    fn offset(&self, offset: usize) -> usize {
        self.ranges[..self.ranges.partition_point(|(old, _)| old.end <= offset)]
            .last()
            .map_or(offset, |(old, new)| new.end + (offset - old.end))
    }
}

impl RenameSnapshot {
    fn offset(&self, uri: &str, offset: usize) -> usize {
        self.files
            .get(uri)
            .map_or(offset, |file| file.offset(offset))
    }

    fn declaration(&self, site: &Declaration) -> Declaration {
        Declaration {
            uri: site.uri.clone(),
            range: self.offset(&site.uri, site.range.start)..self.offset(&site.uri, site.range.end),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct CachedWorkspace {
    pub(super) schema: u32,
    pub(super) build: String,
    pub(super) roots: BTreeSet<PathBuf>,
    disk_paths: BTreeMap<PathBuf, PathBuf>,
    sources: BTreeMap<PathBuf, CachedFile>,
    rebuild: BTreeSet<PathBuf>,
}

#[derive(Serialize, Deserialize)]
struct CachedFile {
    uri: String,
    text: String,
    module: Module,
    parsed: bool,
    references: Vec<Reference>,
    semantically_valid: bool,
}

struct Buffer {
    path: Option<PathBuf>,
    text: Arc<str>,
}

#[derive(Default)]
pub(crate) struct Index {
    buffers: BTreeMap<String, Buffer>,
    roots: BTreeSet<PathBuf>,
    disk_paths: BTreeMap<PathBuf, PathBuf>,
    // A missing module still has reverse edges, so creating it reindexes importers.
    imports: BTreeMap<PathBuf, BTreeSet<PathBuf>>,
    importers: BTreeMap<PathBuf, BTreeSet<PathBuf>>,
    modules: BTreeMap<PathBuf, Module>,
    texts: BTreeMap<PathBuf, Arc<str>>,
    parsed: BTreeMap<PathBuf, Document>,
    parsed_sources: BTreeSet<PathBuf>,
    file_uris: BTreeMap<PathBuf, String>,
    files: BTreeMap<String, IndexedFile>,
    exports: BTreeMap<String, BTreeSet<(PathBuf, u32)>>,
    references: HashMap<Declaration, BTreeMap<String, Vec<usize>>>,
    #[cfg(test)]
    updates: BTreeMap<String, usize>,
}

impl Index {
    #[cfg(test)]
    fn update_count(&self, uri: &str) -> usize {
        self.updates.get(uri).copied().unwrap_or_default()
    }

    fn include_importers(&self, paths: &mut BTreeSet<PathBuf>) {
        let mut pending: Vec<_> = paths.iter().cloned().collect();
        while let Some(path) = pending.pop() {
            for importer in self.importers.get(&path).into_iter().flatten() {
                if paths.insert(importer.clone()) {
                    pending.push(importer.clone());
                }
            }
        }
    }

    fn snapshot(&self) -> CachedWorkspace {
        let mut dirty = BTreeSet::new();
        for (path, uri) in &self.file_uris {
            let file = &self.files[uri];
            let disk_uri = Workspace::indexed(path, &self.modules).uri(path);
            if disk_uri.as_ref() != Some(uri)
                || disk_text(path).as_deref() != Some(file.text.as_ref())
            {
                dirty.insert(path.clone());
            }
        }
        // A saved importer can still resolve against an unsaved dependency.
        // Exclude that whole reverse dependency closure from the disk snapshot.
        self.include_importers(&mut dirty);
        let sources = self
            .modules
            .iter()
            .filter(|(path, _)| !dirty.contains(*path))
            .filter_map(|(path, module)| {
                let uri = self.file_uris.get(path)?;
                let file = self.files.get(uri)?;
                Some((
                    path.clone(),
                    CachedFile {
                        uri: uri.clone(),
                        text: file.text.to_string(),
                        module: module.clone(),
                        parsed: self.parsed_sources.contains(path),
                        references: file.references.clone(),
                        semantically_valid: file.semantically_valid,
                    },
                ))
            })
            .collect();
        CachedWorkspace {
            schema: 0,
            build: String::new(),
            roots: self.roots.clone(),
            disk_paths: self.disk_paths.clone(),
            sources,
            rebuild: dirty,
        }
    }

    fn restore(
        &mut self,
        cache: CachedWorkspace,
        paths: &BTreeMap<PathBuf, PathBuf>,
    ) -> BTreeSet<PathBuf> {
        let mut changed = cache.rebuild;
        changed.extend(
            paths
                .values()
                .filter(|path| !cache.sources.contains_key(*path))
                .cloned(),
        );
        for (input, old) in &cache.disk_paths {
            match paths.get(input) {
                Some(new) if old != new => changed.extend([old.clone(), new.clone()]),
                None => {
                    changed.insert(old.clone());
                }
                _ => {}
            }
        }
        let available: BTreeMap<_, _> = cache
            .sources
            .values()
            .map(|file| (file.uri.as_str(), file.text.as_str()))
            .collect();
        let mut valid_references = BTreeSet::new();
        for (path, file) in &cache.sources {
            let valid = file
                .references
                .windows(2)
                .all(|pair| pair[0].range.end <= pair[1].range.start)
                && file.references.iter().all(|item| {
                    file.text.get(item.range.clone()).is_some()
                        && [&item.target, &item.definition].into_iter().all(|site| {
                            available
                                .get(site.uri.as_str())
                                .is_some_and(|text| text.get(site.range.clone()).is_some())
                        })
                });
            if valid {
                valid_references.insert(path.clone());
            }
        }
        let cached_paths: BTreeSet<_> = cache.sources.keys().cloned().collect();
        for (path, file) in cache.sources {
            if !path.is_absolute() || self.modules.contains_key(&path) {
                changed.insert(path);
                continue;
            }
            let inputs: BTreeSet<_> = file
                .module
                .imports
                .values()
                .map(|target| target.file.clone())
                .collect();
            for input in &inputs {
                self.importers
                    .entry(input.clone())
                    .or_default()
                    .insert(path.clone());
                if !cached_paths.contains(input) && disk_text(input).is_some() {
                    changed.insert(input.clone());
                }
            }
            let text_matches = self.buffer(&path).map_or_else(
                || disk_text(&path).as_deref() == Some(&file.text),
                |(_, text)| text.as_ref() == file.text,
            );
            let uri_matches = self.uri(&path).as_deref() == Some(&file.uri);
            let imports_match = file
                .module
                .imports
                .values()
                .all(|target| normalize(&target.input) == target.file);
            let references_match = valid_references.contains(&path);
            self.imports.insert(path.clone(), inputs);
            if file.parsed {
                self.parsed_sources.insert(path.clone());
                self.add_exports(&path, &file.module);
            }
            self.modules.insert(path.clone(), file.module);
            if !text_matches || !uri_matches || !imports_match || !references_match {
                changed.insert(path.clone());
            }
            let text: Arc<str> = file.text.into();
            self.texts.insert(path.clone(), text.clone());
            self.file_uris.insert(path.clone(), file.uri.clone());
            if references_match && uri_matches {
                self.install_file(
                    file.uri,
                    text,
                    Some(&path),
                    file.references,
                    file.semantically_valid,
                );
            }
        }
        // Cached references may point into a buffer that was already indexed
        // before the root scan. Re-resolve its importers even if the overlay's
        // interface matches what is already in memory.
        self.include_importers(&mut changed);
        changed
    }

    #[cfg(test)]
    fn set_roots(&mut self, roots: &BTreeSet<PathBuf>) {
        self.scan_roots(roots, None, &Cancellation::default(), |_| {});
    }

    fn scan_roots(
        &mut self,
        roots: &BTreeSet<PathBuf>,
        cache: Option<&Cache>,
        stop: &Cancellation,
        mut report: impl FnMut(IndexProgress),
    ) -> bool {
        report(IndexProgress::Begin);
        let mut paths = BTreeMap::new();
        let mut pending: Vec<_> = roots.difference(&self.roots).cloned().collect();
        paths.extend(
            self.disk_paths
                .iter()
                .filter(|(path, _)| roots.iter().any(|root| path.starts_with(root)))
                .map(|(path, canonical)| (path.clone(), canonical.clone())),
        );
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(directory).into_iter().flatten().flatten() {
                if stop.cancelled() {
                    return false;
                }
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                if kind.is_dir() {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if index_directory(&name) {
                        pending.push(entry.path());
                    }
                } else if (kind.is_file() || kind.is_symlink())
                    && entry.path().extension().is_some_and(|ext| ext == "xr")
                {
                    let path = entry.path();
                    paths.insert(path.clone(), normalize(&path));
                }
            }
        }
        let restored = self
            .roots
            .is_empty()
            .then(|| cache.and_then(|cache| cache.load(roots)))
            .flatten();
        let cold = restored.is_none();
        let changed: Vec<_> = if let Some(snapshot) = restored {
            self.restore(snapshot, &paths).into_iter().collect()
        } else {
            paths
                .iter()
                .filter(|(path, file)| self.disk_paths.get(*path) != Some(*file))
                .flat_map(|(path, file)| self.disk_paths.get(path).into_iter().chain([file]))
                .cloned()
                .collect()
        };
        let dirty = cold || !changed.is_empty();
        self.roots = roots.clone();
        self.disk_paths = paths;
        self.refresh_with_progress(changed, stop, &mut report);
        if !stop.cancelled() {
            self.prune();
            report(IndexProgress::End);
        }
        dirty
    }

    fn watched(&mut self, changes: Vec<(PathBuf, bool)>) {
        let mut dirty = Vec::new();
        let mut removed_paths = false;
        for (path, deleted) in changes {
            // VS Code sends paths using the workspace spelling. Match the
            // canonical root while keeping the final name for delete events.
            let Some((parent, name)) = path.parent().zip(path.file_name()) else {
                continue;
            };
            let path = normalize(parent).join(name);
            let previous = self.disk_paths.remove(&path);
            let file = if deleted {
                previous.clone().unwrap_or_else(|| normalize(&path))
            } else {
                normalize(&path)
            };
            let inside_root = path.extension().is_some_and(|ext| ext == "xr")
                && self.roots.iter().any(|root| {
                    path.strip_prefix(root).is_ok_and(|relative| {
                        relative
                            .parent()
                            .into_iter()
                            .flat_map(Path::components)
                            .all(|directory| {
                                index_directory(&directory.as_os_str().to_string_lossy())
                            })
                    })
                });
            if !deleted && (inside_root || previous.is_some()) {
                self.disk_paths.insert(path.clone(), file.clone());
            }
            removed_paths |= previous
                .as_ref()
                .is_some_and(|previous| deleted || previous != &file);
            if let Some(previous) = previous
                .as_ref()
                .filter(|previous| *previous != &path && (deleted || *previous != &file))
            {
                // Resolve import paths again when their symlink changes target.
                dirty.extend(self.importers.get(previous).into_iter().flatten().cloned());
            }
            for file in previous.into_iter().chain([file]) {
                if self.buffer(&file).is_none()
                    && (inside_root
                        || self.modules.contains_key(&file)
                        || self.importers.contains_key(&file))
                {
                    dirty.push(file);
                }
            }
        }
        if self.refresh(dirty) || removed_paths {
            self.prune();
        }
    }

    #[cfg(test)]
    fn open(&mut self, uri: String, path: Option<PathBuf>, text: Arc<str>) {
        self.open_batch(BTreeMap::from([(uri, Buffer { path, text })]));
    }

    fn open_batch(&mut self, buffers: BTreeMap<String, Buffer>) {
        let mut dirty = Vec::new();
        let mut untitled = Vec::new();
        for (uri, buffer) in buffers {
            if let Some(path) = &buffer.path {
                if !self
                    .files
                    .get(&uri)
                    .is_some_and(|file| file.text == buffer.text)
                    || self.file_uris.get(path) != Some(&uri)
                {
                    dirty.push(path.clone());
                }
            } else {
                untitled.push((uri.clone(), buffer.text.clone()));
            }
            self.buffers.insert(uri, buffer);
        }
        if self.refresh(dirty) {
            self.prune();
        }
        for (uri, text) in untitled {
            self.index_file(uri, text, None);
        }
    }

    fn close(&mut self, uri: &str) {
        if let Some(buffer) = self.buffers.remove(uri) {
            self.remove_file(uri);
            if let Some(path) = buffer.path {
                self.refresh(vec![path]);
                self.prune();
            }
        }
    }

    fn buffer(&self, path: &Path) -> Option<(&str, &Arc<str>)> {
        self.buffers.iter().find_map(|(uri, buffer)| {
            (buffer.path.as_deref() == Some(path)).then_some((uri.as_str(), &buffer.text))
        })
    }

    fn uri(&self, path: &Path) -> Option<String> {
        self.buffer(path)
            .map(|(uri, _)| uri.to_owned())
            .or_else(|| Workspace::indexed(path, &self.modules).uri(path))
    }

    fn refresh(&mut self, changed: Vec<PathBuf>) -> bool {
        self.refresh_with_progress(changed, &Cancellation::default(), &mut |_| {})
    }

    fn refresh_with_progress(
        &mut self,
        changed: Vec<PathBuf>,
        stop: &Cancellation,
        report: &mut impl FnMut(IndexProgress),
    ) -> bool {
        // Load updated modules first. A batch may update a declaration and its
        // uses, and both need to be resolved against the same module snapshot.
        let mut pending = changed;
        let mut dirty = BTreeSet::new();
        let mut changed_interfaces = Vec::new();
        let mut removed_imports = false;
        while let Some(path) = pending.pop() {
            if stop.cancelled() {
                return removed_imports;
            }
            if !dirty.insert(path.clone()) {
                continue;
            }
            let text = self
                .buffer(&path)
                .map(|(_, text)| text.clone())
                .or_else(|| disk_text(&path).map(Arc::from));
            let module = text.as_ref().map(|text| Module::new(&path, text));
            if module.as_ref() != self.modules.get(&path)
                || self.file_uris.get(&path) != self.uri(&path).as_ref()
            {
                changed_interfaces.push(path.clone());
            }
            let inputs: BTreeSet<_> = module
                .iter()
                .flat_map(|module| module.imports.values())
                .map(|target| target.file.clone())
                .collect();
            if let Some(previous) = self.modules.remove(&path) {
                self.remove_exports(&path, &previous);
            }
            let previous = self.imports.remove(&path).unwrap_or_default();
            for input in previous.difference(&inputs) {
                removed_imports = true;
                if let Some(users) = self.importers.get_mut(input) {
                    users.remove(&path);
                    if users.is_empty() {
                        self.importers.remove(input);
                    }
                }
            }
            for input in &inputs {
                if !previous.contains(input) {
                    self.importers
                        .entry(input.clone())
                        .or_default()
                        .insert(path.clone());
                }
                if !self.modules.contains_key(input) {
                    pending.push(input.clone());
                }
            }
            if let Some((text, module)) = text.zip(module) {
                self.imports.insert(path.clone(), inputs);
                let parsed = xrack_parser::parse(&text).ok();
                if self.parsed_sources.contains(&path) != parsed.is_some() {
                    changed_interfaces.push(path.clone());
                }
                if let Some(document) = parsed {
                    self.parsed.insert(path.clone(), document);
                    self.parsed_sources.insert(path.clone());
                    self.add_exports(&path, &module);
                } else {
                    self.parsed.remove(&path);
                    self.parsed_sources.remove(&path);
                }
                self.texts.insert(path.clone(), text);
                self.modules.insert(path, module);
            } else {
                if self.texts.remove(&path).is_some() {
                    changed_interfaces.push(path.clone());
                }
                self.parsed.remove(&path);
                self.parsed_sources.remove(&path);
            }
        }
        let mut invalidated = BTreeSet::new();
        let mut pending = changed_interfaces;
        while let Some(path) = pending.pop() {
            for importer in self.importers.get(&path).into_iter().flatten() {
                if invalidated.insert(importer.clone()) {
                    dirty.insert(importer.clone());
                    pending.push(importer.clone());
                }
            }
        }
        let total = dirty.len();
        report(IndexProgress::Report { done: 0, total });
        let mut last_percent = 0;
        for (index, path) in dirty.into_iter().enumerate() {
            if stop.cancelled() {
                return removed_imports;
            }
            if let Some(previous_uri) = self.file_uris.remove(&path) {
                self.remove_file(&previous_uri);
            }
            if let Some((uri, text)) = self.uri(&path).zip(self.texts.get(&path).cloned()) {
                self.index_file(uri.clone(), text, Some(&path));
                self.file_uris.insert(path, uri);
            }
            let done = index + 1;
            let percent = done * 100 / total;
            if percent >= last_percent + 5 || done == total {
                report(IndexProgress::Report { done, total });
                last_percent = percent;
            }
        }
        removed_imports
    }

    fn prune(&mut self) {
        // Keep files in the workspace, open buffers and all reachable imports.
        // Walk the cached graph so closing a file can evict an external cycle.
        let mut live = BTreeSet::new();
        let mut pending: Vec<_> = self
            .disk_paths
            .values()
            .chain(
                self.buffers
                    .values()
                    .filter_map(|buffer| buffer.path.as_ref()),
            )
            .cloned()
            .collect();
        while let Some(path) = pending.pop() {
            if live.insert(path.clone()) {
                pending.extend(self.imports.get(&path).into_iter().flatten().cloned());
            }
        }
        let obsolete: Vec<_> = self
            .modules
            .keys()
            .filter(|path| !live.contains(*path))
            .cloned()
            .collect();
        for path in obsolete {
            if let Some(uri) = self.file_uris.remove(&path) {
                self.remove_file(&uri);
            }
            self.texts.remove(&path);
            self.parsed.remove(&path);
            self.parsed_sources.remove(&path);
            if let Some(module) = self.modules.remove(&path) {
                self.remove_exports(&path, &module);
            }
            self.imports.remove(&path);
        }
        self.importers.retain(|_, importers| {
            importers.retain(|path| live.contains(path));
            !importers.is_empty()
        });
    }

    fn add_exports(&mut self, path: &Path, module: &Module) {
        for (name, kind, _, exported) in module.symbols() {
            if exported {
                self.exports
                    .entry(name.to_owned())
                    .or_default()
                    .insert((path.to_owned(), kind));
            }
        }
    }

    fn remove_exports(&mut self, path: &Path, module: &Module) {
        for (name, kind, _, exported) in module.symbols() {
            if exported && let Some(exports) = self.exports.get_mut(name) {
                exports.remove(&(path.to_owned(), kind));
                if exports.is_empty() {
                    self.exports.remove(name);
                }
            }
        }
    }

    fn remove_file(&mut self, uri: &str) {
        if let Some(file) = self.files.remove(uri) {
            for reference in file.references {
                if let Some(files) = self.references.get_mut(&reference.target) {
                    files.remove(uri);
                    if files.is_empty() {
                        self.references.remove(&reference.target);
                    }
                }
            }
        }
    }

    fn materialize_parsed(&mut self, path: &Path) {
        let mut pending = vec![path.to_owned()];
        let mut seen = BTreeSet::new();
        while let Some(path) = pending.pop() {
            if !seen.insert(path.clone()) {
                continue;
            }
            if self.parsed_sources.contains(&path)
                && !self.parsed.contains_key(&path)
                && let Some(text) = self.texts.get(&path)
                && let Ok(document) = xrack_parser::parse(text)
            {
                self.parsed.insert(path.clone(), document);
            }
            pending.extend(self.imports.get(&path).into_iter().flatten().cloned());
        }
    }

    fn index_file(&mut self, uri: String, text: Arc<str>, path: Option<&Path>) {
        self.remove_file(&uri);
        if let Some(path) = path {
            self.materialize_parsed(path);
        }
        let (references, semantically_valid) =
            self.resolve_file(&uri, &text, path, &RenameSnapshot::default());
        #[cfg(test)]
        {
            *self.updates.entry(uri.clone()).or_default() += 1;
        }
        self.install_file(uri, text, path, references, semantically_valid);
    }

    fn install_file(
        &mut self,
        uri: String,
        text: Arc<str>,
        path: Option<&Path>,
        references: Vec<Reference>,
        semantically_valid: bool,
    ) {
        for (index, reference) in references.iter().enumerate() {
            self.references
                .entry(reference.target.clone())
                .or_default()
                .entry(uri.clone())
                .or_default()
                .push(index);
        }
        let lines = Source::line_starts(&text);
        self.files.insert(
            uri,
            IndexedFile {
                text,
                lines,
                references,
                path: path.map(Path::to_owned),
                semantically_valid,
            },
        );
    }

    fn resolve_file(
        &self,
        uri: &str,
        text: &str,
        path: Option<&Path>,
        snapshot: &RenameSnapshot,
    ) -> (Vec<Reference>, bool) {
        let workspace =
            path.map(|path| Workspace::indexed(path, &self.modules).with_edits(&snapshot.modules));
        let tokens = editor_tokens::scan(text);
        let fallback = occurrences(&tokens, text.len(), workspace.as_ref());
        let semantic = self.semantic_sites(uri, text, path, &tokens, snapshot);
        let semantically_valid = semantic.is_some();
        let semantic = semantic.unwrap_or_default();
        let mut references = BTreeMap::new();
        for item in fallback {
            let Occurrence {
                range,
                site,
                declaration,
                definition,
                alias,
            } = item;
            let Some(target_uri) = site
                .file
                .as_deref()
                .map_or_else(|| Some(uri.to_owned()), |path| self.uri(path))
            else {
                continue;
            };
            let target = Declaration {
                uri: target_uri,
                range: site.range,
            };
            let definition = semantic
                .get(&range.start)
                .cloned()
                .or_else(|| {
                    let uri = definition
                        .file
                        .as_deref()
                        .map_or_else(|| Some(uri.to_owned()), |path| self.uri(path))?;
                    Some(Declaration {
                        uri,
                        range: definition.range,
                    })
                })
                .unwrap_or_else(|| target.clone());
            // An explicit import alias has its own local identity for rename.
            let target = if alias { target } else { definition.clone() };
            references.insert(
                range.start,
                Reference {
                    range,
                    target,
                    declaration,
                    definition,
                },
            );
        }
        for (start, definition) in semantic {
            if let std::collections::btree_map::Entry::Vacant(entry) = references.entry(start)
                && let Ok(index) = tokens.binary_search_by_key(&start, |token| token.range.start)
            {
                entry.insert(Reference {
                    range: tokens[index].range.clone(),
                    target: definition.clone(),
                    declaration: false,
                    definition,
                });
            }
        }
        (references.into_values().collect(), semantically_valid)
    }

    fn semantic_sites(
        &self,
        uri: &str,
        text: &str,
        path: Option<&Path>,
        tokens: &[editor_tokens::Token<'_>],
        snapshot: &RenameSnapshot,
    ) -> Option<BTreeMap<usize, Declaration>> {
        let parsed_at = |path: &Path| match snapshot.parsed.get(path) {
            Some(document) => document.as_ref(),
            None => self.parsed.get(path),
        };
        let entry = match path {
            Some(path) => parsed_at(path).cloned(),
            None => xrack_parser::parse(text).ok(),
        }?;
        let mut files = Vec::new();
        let mut inputs = BTreeMap::new();
        // Use the same immutable source snapshots as the editor index.
        let mut parsed = BTreeMap::new();
        let mut pending: Vec<_> = path.into_iter().collect();
        let mut seen = BTreeSet::new();
        while let Some(path) = pending.pop() {
            if seen.insert(path) {
                let document = parsed_at(path)?;
                parsed.insert(path.to_owned(), document);
                pending.extend(
                    self.imports
                        .get(path)
                        .into_iter()
                        .flatten()
                        .map(PathBuf::as_path),
                );
            }
        }
        let document = if let Some(path) = path {
            resolve_indexed_document(entry, path, &parsed, &mut inputs, &mut files).ok()?
        } else {
            entry
        };
        let uses = resolve_symbols(&document).ok()?;
        let source_path = |span: xrack_parser::ast::Span| {
            span.source
                .checked_sub(1)
                .and_then(|index| files.get(index))
                .map(PathBuf::as_path)
                .or(path)
        };
        let source = Source::new(text);
        let mut sites = BTreeMap::new();
        let mut declaration_sources = HashMap::new();
        for usage in uses {
            if source_path(usage.usage) != path || usage.usage.line == 0 {
                continue;
            }
            let offset = source.offset(usage.usage);
            let index = tokens.partition_point(|token| token.range.start < offset);
            let mut code = tokens[index..].iter().filter(|token| token.code());
            let Some(mut token) = code.next().filter(|token| token.range.start == offset) else {
                continue;
            };
            if !usage.spelling.is_empty() {
                let mut matched = true;
                for (segment, name) in usage
                    .spelling
                    .split('.')
                    .take(usage.segment + 1)
                    .enumerate()
                {
                    if segment != 0 {
                        let Some(next) = code
                            .next()
                            .filter(|token| token.text == ".")
                            .and_then(|_| code.next())
                        else {
                            matched = false;
                            break;
                        };
                        token = next;
                    }
                    if token.text != name {
                        matched = false;
                        break;
                    }
                }
                if !matched {
                    continue;
                }
            }
            let declaration_path = source_path(usage.declaration);
            let name = usage.name.as_str();
            let (source, tokens, uri) = if declaration_path == path {
                // This file's source and tokens already belong to the index update.
                (&source, tokens, uri.to_owned())
            } else {
                let Some(path) = declaration_path else {
                    continue;
                };
                let Some(uri) = self.uri(path) else {
                    continue;
                };
                let Some(text) = snapshot
                    .uris
                    .get(path)
                    .map(|uri| snapshot.files[uri].text.as_str())
                    .or_else(|| self.texts.get(path).map(AsRef::as_ref))
                else {
                    continue;
                };
                let (source, tokens) = declaration_sources
                    .entry(path)
                    .or_insert_with(|| (Source::new(text), editor_tokens::scan(text)));
                (&*source, tokens.as_slice(), uri)
            };
            if let Some(range) = declaration_range(tokens, source.offset(usage.declaration), name) {
                sites.insert(token.range.start, Declaration { uri, range });
            }
        }
        Some(sites)
    }

    pub(crate) fn workspace_symbols(&self, query: &str, cancellation: &Cancellation) -> Value {
        let query = query.to_ascii_lowercase();
        let mut results = Vec::new();
        for (path, module) in &self.modules {
            if cancellation.cancelled() {
                break;
            }
            let Some((uri, file)) = self
                .file_uris
                .get(path)
                .and_then(|uri| Some((uri, self.files.get(uri)?)))
            else {
                continue;
            };
            let source = Source::with_lines(&file.text, &file.lines);
            for (name, kind, range, _) in module.symbols() {
                let folded = name.to_ascii_lowercase();
                if folded.contains(&query) {
                    results.push((!folded.starts_with(&query), name, uri, json!({
                        "name": name, "kind": if kind == CLASS { 5 } else { 23 },
                        "location": { "uri": uri, "range": source.range(range.start, range.end) },
                    })));
                }
            }
        }
        results.sort_by(|a, b| (a.0, a.1, a.2).cmp(&(b.0, b.1, b.2)));
        json!(
            results
                .into_iter()
                .take(200)
                .map(|(_, _, _, symbol)| symbol)
                .collect::<Vec<_>>()
        )
    }

    pub(crate) fn auto_imports(
        &self,
        entry: &Path,
        prefix: &str,
        kind: u32,
        cancellation: &Cancellation,
    ) -> Vec<(String, String)> {
        let Some(directory) = entry.parent() else {
            return vec![];
        };
        let mut blocked = BTreeSet::from([entry]);
        let mut pending = vec![entry];
        while let Some(path) = pending.pop() {
            if cancellation.cancelled() {
                return vec![];
            }
            for importer in self.importers.get(path).into_iter().flatten() {
                if blocked.insert(importer) {
                    pending.push(importer);
                }
            }
        }
        self.exports
            .range(prefix.to_owned()..)
            .take_while(|(name, _)| !cancellation.cancelled() && name.starts_with(prefix))
            .flat_map(|(name, exports)| {
                exports.iter().filter_map({
                    let blocked = &blocked;
                    move |(path, symbol_kind)| {
                        if *symbol_kind != kind || blocked.contains(path.as_path()) {
                            return None;
                        }
                        Some((name.clone(), relative_import(directory, path)?))
                    }
                })
            })
            .take(100)
            .collect()
    }

    pub(crate) fn refactor(
        &mut self,
        method: &str,
        uri: &str,
        position: (usize, usize),
        params: &Value,
        cancellation: &Cancellation,
    ) -> Result<Value, &'static str> {
        let file = self.files.get(uri).ok_or("document is not indexed")?;
        let source = Source::with_lines(&file.text, &file.lines);
        let cursor = source
            .offset_at(position.0, position.1)
            .ok_or("invalid position")?;
        let occurrence = file
            .references
            .partition_point(|item| item.range.start <= cursor)
            .checked_sub(1)
            .and_then(|index| file.references.get(index))
            .filter(|item| cursor <= item.range.end);
        let Some(occurrence) = occurrence else {
            return if method == "textDocument/definition" {
                Ok(Value::Null)
            } else if method == "textDocument/references" {
                Ok(json!([]))
            } else {
                Err("no renameable XRack symbol at this position")
            };
        };
        let spelling = &file.text[occurrence.range.clone()];
        if method == "textDocument/definition" {
            let site = &occurrence.definition;
            let Some(file) = self.files.get(&site.uri) else {
                return Ok(Value::Null);
            };
            return Ok(
                json!({ "uri": site.uri, "range": Source::with_lines(&file.text, &file.lines).range(site.range.start, site.range.end) }),
            );
        }
        if method == "textDocument/prepareRename" {
            return Ok(
                json!({"range": source.range(occurrence.range.start, occurrence.range.end), "placeholder": spelling}),
            );
        }
        let new_name = if method == "textDocument/rename" {
            let value = params["newName"].as_str().ok_or("missing new name")?;
            if value == spelling {
                return Ok(json!({"changes": {}}));
            }
            validate_name(value)?;
            Some(value)
        } else {
            None
        };
        if let Some(new_name) = new_name {
            let target = occurrence.target.clone();
            let occurrences = self.references[&target].clone();
            return self.rename(&target, &occurrences, new_name, cancellation);
        }
        let occurrences = &self.references[&occurrence.target];
        let include_declaration = params["context"]["includeDeclaration"] == true;
        let mut locations = Vec::new();
        for (uri, indices) in occurrences {
            let file = &self.files[uri];
            let source = Source::with_lines(&file.text, &file.lines);
            for &index in indices {
                if cancellation.cancelled() {
                    return Err("Request cancelled");
                }
                let item = &file.references[index];
                if include_declaration || !item.declaration {
                    let range = source.range(item.range.start, item.range.end);
                    locations.push(json!({"uri": uri, "range": range}));
                }
            }
        }
        Ok(json!(locations))
    }

    fn rename(
        &mut self,
        target: &Declaration,
        occurrences: &BTreeMap<String, Vec<usize>>,
        new_name: &str,
        cancellation: &Cancellation,
    ) -> Result<Value, &'static str> {
        let mut snapshot = RenameSnapshot::default();
        let mut changes = BTreeMap::new();
        let mut found_declaration = false;
        let mut affected = BTreeSet::new();
        let mut pending = Vec::new();
        for (uri, indices) in occurrences {
            if cancellation.cancelled() {
                return Err("Request cancelled");
            }
            let file = &self.files[uri];
            let source = Source::with_lines(&file.text, &file.lines);
            let ranges: Vec<_> = indices
                .iter()
                .map(|&index| {
                    let item = &file.references[index];
                    found_declaration |= item.declaration;
                    item.range.clone()
                })
                .collect();
            let edited = EditedFile::new(&file.text, &ranges, new_name);
            if let Some(path) = &file.path {
                snapshot
                    .modules
                    .insert(path.clone(), Module::new(path, &edited.text));
                snapshot
                    .parsed
                    .insert(path.clone(), xrack_parser::parse(&edited.text).ok());
                snapshot.uris.insert(path.clone(), uri.clone());
                pending.push(path.as_path());
            }
            snapshot.files.insert(uri.clone(), edited);
            affected.insert(uri.clone());
            changes.insert(uri, ranges.iter().map(|range| {
                json!({"range": source.range(range.start, range.end), "newText": new_name})
            }).collect::<Vec<_>>());
        }
        if !found_declaration {
            return Err("the declaration could not be included in the rename");
        }
        // An inherited member can shadow a reference in a file that has no
        // edits of its own. Check the reverse import closure as well.
        let mut visited = BTreeSet::new();
        while let Some(path) = pending.pop() {
            if cancellation.cancelled() {
                return Err("Request cancelled");
            }
            if visited.insert(path) {
                affected.extend(self.file_uris.get(path).cloned());
                pending.extend(
                    self.importers
                        .get(path)
                        .into_iter()
                        .flatten()
                        .map(PathBuf::as_path),
                );
            }
        }

        let renamed = snapshot.declaration(target);
        for edited in snapshot.files.values() {
            if cancellation.cancelled() {
                return Err("Request cancelled");
            }
            let tokens = editor_tokens::scan(&edited.text);
            if duplicate_declaration(&tokens, edited.text.len(), &edited.ranges, new_name) {
                return Err("rename would introduce a duplicate declaration in this scope");
            }
        }
        for uri in affected {
            if let Some(path) = self.files[&uri].path.clone() {
                self.materialize_parsed(&path);
            }
            if cancellation.cancelled() {
                return Err("Request cancelled");
            }
            let file = &self.files[&uri];
            let text = snapshot
                .files
                .get(&uri)
                .map_or(file.text.as_ref(), |edited| &edited.text);
            let (resolved, valid) = self.resolve_file(&uri, text, file.path.as_deref(), &snapshot);
            if file.semantically_valid && !valid {
                return Err("rename would introduce a semantic error in an affected file");
            }
            let mut after = resolved.iter().peekable();
            for before in &file.references {
                let offset = snapshot.offset(&uri, before.range.start);
                while after.peek().is_some_and(|item| item.range.start < offset) {
                    if after
                        .next()
                        .is_some_and(|item| item.target == renamed || item.definition == renamed)
                    {
                        return Err("rename would capture an existing reference");
                    }
                }
                let Some(item) = after.next_if(|item| item.range.start == offset) else {
                    return Err("rename would leave an existing reference unresolved");
                };
                if item.target != snapshot.declaration(&before.target)
                    || item.definition != snapshot.declaration(&before.definition)
                {
                    return Err(
                        "rename would change the declaration an existing reference resolves to",
                    );
                }
            }
            if after.any(|item| item.target == renamed || item.definition == renamed) {
                return Err("rename would capture an existing reference");
            }
        }
        Ok(json!({"changes": changes}))
    }
}

fn declaration_range(
    tokens: &[editor_tokens::Token<'_>],
    offset: usize,
    name: &str,
) -> Option<Range<usize>> {
    let start = tokens.partition_point(|token| token.range.start < offset);
    let mut code = tokens[start..].iter().filter(|token| token.code());
    let first = code.next()?;
    let token = match first.text {
        "export" => {
            code.next()?;
            code.next()?
        }
        "component" | "struct" | "callback" => code.next()?,
        "pure" => {
            code.next()?;
            code.next()?
        }
        "property" | "in" | "out" | "in-out" | "private" => {
            code.by_ref().find(|token| token.text == ">")?;
            code.next()?
        }
        "if" | "for" => {
            // Element declarations with a guard or repeat prefix use :=.
            let mut previous = first;
            loop {
                let next = code.next()?;
                if next.text == ":=" {
                    break previous;
                }
                if matches!(next.text, "{" | ";" | "}") {
                    return None;
                }
                previous = next;
            }
        }
        _ => first,
    };
    (token.text == name
        || name
            .strip_prefix("XrImport")
            .and_then(|suffix| suffix.split_once('-'))
            .is_some_and(|(number, original)| {
                !number.is_empty()
                    && number.bytes().all(|byte| byte.is_ascii_digit())
                    && original == token.text
            }))
    .then(|| token.range.clone())
}

fn relative_import(directory: &Path, file: &Path) -> Option<String> {
    let common = directory
        .components()
        .zip(file.components())
        .take_while(|(a, b)| a == b)
        .count();
    if common == 0 {
        return None;
    }
    let mut path: PathBuf =
        std::iter::repeat_n("..", directory.components().count() - common).collect();
    path.extend(file.components().skip(common));
    Some(path.to_str()?.replace(std::path::MAIN_SEPARATOR, "/"))
}

fn index_directory(name: &str) -> bool {
    !name.starts_with('.')
        && !name.starts_with("bazel-")
        && !matches!(name, "target" | "node_modules")
}

fn validate_name(value: &str) -> Result<(), &'static str> {
    let tokens = editor_tokens::scan(value);
    if !matches!(&tokens[..], [token] if token.text == value && editor_tokens::name(value))
        || matches!(value, "_" | "root" | "self" | "Self" | "super" | "crate")
        || value.starts_with("__")
        || xrack_parser::parse(&format!(
            "component RenameTest inherits Window {{ property <int> {value}; }}"
        ))
        .is_err()
    {
        return Err("invalid XRack identifier");
    }
    Ok(())
}

type Query = Box<dyn FnOnce(&mut Index, &Cancellation) -> Reply + Send>;

enum Message {
    Cache(PathBuf),
    Roots(BTreeSet<PathBuf>, Option<u64>),
    Watched(Vec<(PathBuf, bool)>),
    Open(BTreeMap<String, Buffer>),
    Close(String),
    Save,
    Query(Value, Cancellation, Query),
}

pub(crate) struct BackgroundIndex {
    sender: Option<Sender<Message>>,
    worker: Option<JoinHandle<()>>,
    stop: Cancellation,
}

impl BackgroundIndex {
    pub(crate) fn new(events: Sender<Event>) -> io::Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let stop = Cancellation::default();
        let worker_stop = stop.clone();
        let worker = thread::Builder::new()
            .name("xrack-workspace-index".into())
            .spawn(move || {
                let mut index = Index::default();
                let mut cache: Option<Cache> = None;
                let mut dirty = false;
                let mut next = None;
                while let Some(message) = next.take().or_else(|| {
                    if dirty && cache.is_some() {
                        match receiver.recv_timeout(std::time::Duration::from_secs(2)) {
                            Ok(message) => Some(message),
                            Err(mpsc::RecvTimeoutError::Timeout) => Some(Message::Save),
                            Err(mpsc::RecvTimeoutError::Disconnected) => None,
                        }
                    } else {
                        receiver.recv().ok()
                    }
                }) {
                    if worker_stop.cancelled() {
                        break;
                    }
                    match message {
                        Message::Cache(path) => cache = Cache::new(path),
                        Message::Roots(roots, progress) => {
                            dirty |=
                                index.scan_roots(&roots, cache.as_ref(), &worker_stop, |update| {
                                    if let Some(id) = progress {
                                        let _ = events.send(Event::Progress(id, update));
                                    }
                                });
                        }
                        Message::Watched(paths) => {
                            index.watched(paths);
                            dirty = true;
                        }
                        Message::Open(mut buffers) => {
                            // A slow initial scan can leave several edit batches queued.
                            // Merge them up to the next query or other ordering boundary.
                            while let Ok(message) = receiver.try_recv() {
                                if let Message::Open(updates) = message {
                                    buffers.extend(updates);
                                } else {
                                    next = Some(message);
                                    break;
                                }
                            }
                            if !worker_stop.cancelled() {
                                index.open_batch(buffers);
                            }
                        }
                        Message::Close(uri) => {
                            index.close(&uri);
                            dirty = true;
                        }
                        Message::Save => {
                            if dirty && let Some(cache) = &cache {
                                let _ = cache.save(index.snapshot());
                                dirty = false;
                            }
                        }
                        Message::Query(id, cancellation, query) => {
                            if !cancellation.cancelled() {
                                let reply = query(&mut index, &cancellation);
                                if !cancellation.cancelled() {
                                    let _ = events.send(Event::Reply {
                                        id,
                                        cancellation,
                                        reply,
                                    });
                                }
                            }
                        }
                    }
                }
            })?;
        Ok(Self {
            sender: Some(sender),
            worker: Some(worker),
            stop,
        })
    }

    fn send(&self, message: Message) -> io::Result<()> {
        self.sender
            .as_ref()
            .expect("active index")
            .send(message)
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "workspace index stopped"))
    }

    pub(crate) fn stop(&self) {
        self.stop.cancel();
    }

    pub(crate) fn cache_directory(&self, path: PathBuf) -> io::Result<()> {
        self.send(Message::Cache(path))
    }

    pub(crate) fn save_cache(&self) -> io::Result<()> {
        self.send(Message::Save)
    }

    pub(crate) fn set_roots(
        &self,
        roots: BTreeSet<PathBuf>,
        progress: Option<u64>,
    ) -> io::Result<()> {
        self.send(Message::Roots(roots, progress))
    }

    pub(crate) fn watched(&self, paths: Vec<(PathBuf, bool)>) -> io::Result<()> {
        self.send(Message::Watched(paths))
    }

    pub(crate) fn open(
        &self,
        buffers: impl IntoIterator<Item = (String, Option<PathBuf>, Arc<str>)>,
    ) -> io::Result<()> {
        self.send(Message::Open(
            buffers
                .into_iter()
                .map(|(uri, path, text)| (uri, Buffer { path, text }))
                .collect(),
        ))
    }

    pub(crate) fn close(&self, uri: String) -> io::Result<()> {
        self.send(Message::Close(uri))
    }

    pub(crate) fn request(
        &self,
        id: Value,
        cancellation: Cancellation,
        query: impl FnOnce(&mut Index, &Cancellation) -> Reply + Send + 'static,
    ) -> io::Result<()> {
        self.send(Message::Query(id, cancellation, Box::new(query)))
    }
}

impl Drop for BackgroundIndex {
    fn drop(&mut self) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(Message::Save);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::workspace_fixture;

    const LEAF: &str = "export component Leaf inherits Rectangle { in property <int> value; }";
    const BRIDGE: &str =
        "import { Leaf } from \"leaf.xr\"; export component Bridge inherits Leaf {}";
    const APP: &str = "import { Bridge } from \"bridge.xr\"; component App inherits Window { Bridge { value: 7; } }";
    const OTHER: &str = "component Other inherits Window { property <int> value; }";

    fn references(index: &mut Index, uri: &str, text: &str, name: &str) -> Vec<Value> {
        let position = Source::new(text).coordinates(text.find(name).unwrap());
        index
            .refactor(
                "textDocument/references",
                uri,
                position,
                &json!({}),
                &Cancellation::default(),
            )
            .unwrap()
            .as_array()
            .unwrap()
            .clone()
    }

    #[test]
    fn restart_restores_references_and_invalidates_only_changed_dependencies() {
        let root = workspace_fixture("index-restart");
        let cache = Cache::new(root.join(".cache")).unwrap();
        let roots = BTreeSet::from([root.clone()]);
        let leaf = root.join("leaf.xr");
        let bridge = root.join("bridge.xr");
        let app = root.join("app.xr");
        let other = root.join("other.xr");
        for (path, source) in [
            (&leaf, LEAF),
            (&bridge, BRIDGE),
            (&app, APP),
            (&other, OTHER),
        ] {
            fs::write(path, source).unwrap();
        }
        let start = || {
            let mut index = Index::default();
            index.scan_roots(&roots, Some(&cache), &Cancellation::default(), |_| {});
            index
        };
        let mut first = start();
        let leaf_uri = first.uri(&leaf).unwrap();
        let bridge_uri = first.uri(&bridge).unwrap();
        let app_uri = first.uri(&app).unwrap();
        let other_uri = first.uri(&other).unwrap();
        let expected = references(&mut first, &leaf_uri, LEAF, "value");
        assert_eq!(expected.len(), 1);
        assert_eq!(expected[0]["uri"], app_uri);
        cache.save(first.snapshot()).unwrap();
        drop(first);

        let mut warm = start();
        assert!(warm.updates.is_empty());
        assert!(warm.parsed.is_empty());
        assert_eq!(references(&mut warm, &leaf_uri, LEAF, "value"), expected);
        assert!(
            warm.parsed.is_empty(),
            "cached references should not require parsing"
        );
        let position = Source::new(LEAF).coordinates(LEAF.find("value").unwrap());
        let rename = warm
            .refactor(
                "textDocument/rename",
                &leaf_uri,
                position,
                &json!({"newName":"amount"}),
                &Cancellation::default(),
            )
            .unwrap();
        assert_eq!(rename["changes"][&app_uri].as_array().unwrap().len(), 1);
        assert!(!warm.parsed.contains_key(&other));

        // An equal-length edit still invalidates the file and its importers.
        let changed_leaf = LEAF.replace("value", "other");
        fs::write(&leaf, &changed_leaf).unwrap();
        let mut changed = start();
        for uri in [&leaf_uri, &bridge_uri, &app_uri] {
            assert_eq!(changed.update_count(uri), 1, "{uri}");
        }
        assert_eq!(changed.update_count(&other_uri), 0);
        assert!(references(&mut changed, &leaf_uri, &changed_leaf, "other").is_empty());

        // A broken cache falls back to indexing rather than exposing stale ranges.
        let cache_file = root.join(".cache/index-v1.json");
        let mut incompatible: Value =
            serde_json::from_slice(&fs::read(&cache_file).unwrap()).unwrap();
        incompatible["build"] = json!("previous server build");
        fs::write(&cache_file, incompatible.to_string()).unwrap();
        assert_eq!(start().update_count(&other_uri), 1);
        fs::write(cache_file, "{broken").unwrap();
        assert_eq!(start().update_count(&other_uri), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn restart_reindexes_importers_when_a_missing_file_appears_or_is_deleted() {
        let root = workspace_fixture("index-import-change");
        let cache = Cache::new(root.join(".cache")).unwrap();
        let roots = BTreeSet::from([root.clone()]);
        let leaf = root.join("leaf.xr");
        let app = root.join("app.xr");
        let other = root.join("other.xr");
        fs::write(root.join("bridge.xr"), BRIDGE).unwrap();
        fs::write(&app, APP).unwrap();
        fs::write(&other, OTHER).unwrap();
        let start = || {
            let mut index = Index::default();
            index.scan_roots(&roots, Some(&cache), &Cancellation::default(), |_| {});
            index
        };

        let first = start();
        let app_uri = first.uri(&app).unwrap();
        let other_uri = first.uri(&other).unwrap();
        let leaf_uri = first.uri(&leaf).unwrap();
        assert!(!first.files[&app_uri].semantically_valid);
        cache.save(first.snapshot()).unwrap();

        fs::write(&leaf, LEAF).unwrap();
        let mut created = start();
        assert_eq!(created.update_count(&app_uri), 1);
        assert_eq!(created.update_count(&other_uri), 0);
        assert!(created.files[&app_uri].semantically_valid);
        assert_eq!(references(&mut created, &leaf_uri, LEAF, "value").len(), 1);
        cache.save(created.snapshot()).unwrap();

        fs::remove_file(leaf).unwrap();
        let deleted = start();
        assert_eq!(deleted.update_count(&app_uri), 1);
        assert_eq!(deleted.update_count(&other_uri), 0);
        assert!(!deleted.files[&app_uri].semantically_valid);
        assert!(!deleted.files.contains_key(&leaf_uri));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn restart_does_not_persist_an_unsaved_overlay_or_its_importers() {
        let root = workspace_fixture("index-overlay");
        let cache_dir = root.join(".cache");
        let cache = Cache::new(cache_dir.clone()).unwrap();
        let roots = BTreeSet::from([root.clone()]);
        let leaf = root.join("leaf.xr");
        let app = root.join("app.xr");
        let saved_app = "import { Leaf } from \"leaf.xr\"; component App inherits Window { Leaf { value: 7; } }";
        fs::write(&leaf, LEAF).unwrap();
        fs::write(&app, saved_app).unwrap();
        let mut index = Index::default();
        index.scan_roots(&roots, Some(&cache), &Cancellation::default(), |_| {});
        let leaf_uri = index.uri(&leaf).unwrap();
        let app_uri = index.uri(&app).unwrap();
        let overlay = format!("// unsaved-marker\n{LEAF}");
        index.open(leaf_uri.clone(), Some(leaf.clone()), overlay.clone().into());
        assert_eq!(
            references(&mut index, &leaf_uri, &overlay, "value").len(),
            1
        );
        let snapshot = index.snapshot();
        assert!(!snapshot.sources.contains_key(&leaf));
        assert!(!snapshot.sources.contains_key(&app));
        cache.save(snapshot).unwrap();
        assert!(
            !fs::read_to_string(cache_dir.join("index-v1.json"))
                .unwrap()
                .contains("unsaved-marker")
        );

        let mut warm = Index::default();
        warm.scan_roots(&roots, Some(&cache), &Cancellation::default(), |_| {});
        assert_eq!(
            references(&mut warm, &leaf_uri, LEAF, "value")[0]["uri"],
            app_uri
        );
        // An overlay that arrives before the root scan wins over the disk cache.
        cache.save(warm.snapshot()).unwrap();
        let mut before_scan = Index::default();
        before_scan.open(leaf_uri.clone(), Some(leaf.clone()), overlay.clone().into());
        before_scan.scan_roots(&roots, Some(&cache), &Cancellation::default(), |_| {});
        assert_eq!(before_scan.files[&leaf_uri].text.as_ref(), overlay);
        assert_eq!(
            references(&mut before_scan, &leaf_uri, &overlay, "value")[0]["uri"],
            app_uri
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn worker_merges_queued_buffers_without_crossing_a_query() {
        let (events, receiver) = mpsc::channel();
        let worker = BackgroundIndex::new(events).unwrap();
        let (entered, running) = mpsc::channel();
        let (resume, blocked) = mpsc::channel();
        worker
            .request(json!(0), Cancellation::default(), move |_, _| {
                entered.send(()).unwrap();
                blocked.recv().unwrap();
                Ok(Value::Null)
            })
            .unwrap();
        running.recv().unwrap();

        let uri = "untitled:batched.xr";
        let observe = |index: &mut Index, _: &Cancellation| {
            Ok(json!([
                index.update_count(uri),
                index.files[uri].text.as_ref()
            ]))
        };
        worker.open([(uri.into(), None, Arc::from(LEAF))]).unwrap();
        worker
            .open([(uri.into(), None, Arc::from(BRIDGE))])
            .unwrap();
        worker
            .request(json!(1), Cancellation::default(), observe)
            .unwrap();
        worker.open([(uri.into(), None, Arc::from(APP))]).unwrap();
        worker
            .request(json!(2), Cancellation::default(), observe)
            .unwrap();
        resume.send(()).unwrap();

        for (id, expected) in [
            (0, Value::Null),
            (1, json!([1, BRIDGE])),
            (2, json!([2, APP])),
        ] {
            let Event::Reply {
                id: actual, reply, ..
            } = receiver
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap()
            else {
                panic!("expected an index response")
            };
            assert_eq!(actual, id);
            assert_eq!(reply.unwrap(), expected);
        }
    }

    #[test]
    fn worker_skips_cancelled_queries_and_drops_cancelled_results() {
        let (events, receiver) = mpsc::channel();
        let worker = BackgroundIndex::new(events).unwrap();
        let (entered, running) = mpsc::channel();
        let (resume, blocked) = mpsc::channel();
        let active = Cancellation::default();
        worker
            .request(json!(1), active.clone(), move |_, cancellation| {
                entered.send(()).unwrap();
                blocked.recv().unwrap();
                assert!(cancellation.cancelled());
                Ok(json!("obsolete"))
            })
            .unwrap();
        running
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let queued = Cancellation::default();
        worker
            .request(json!(2), queued.clone(), |_, _| {
                panic!("cancelled query ran")
            })
            .unwrap();
        worker
            .request(json!(3), Cancellation::default(), |_, _| Ok(json!("ready")))
            .unwrap();
        active.cancel();
        queued.cancel();
        resume.send(()).unwrap();
        let Event::Reply { id, reply, .. } = receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
        else {
            panic!("expected an index response");
        };
        assert_eq!(id, 3);
        assert_eq!(reply.unwrap(), "ready");
    }

    #[test]
    fn queries_reuse_the_index_and_updates_only_rebuild_affected_sources() {
        let root = workspace_fixture("incremental-index");
        let leaf = root.join("leaf.xr");
        let bridge = root.join("bridge.xr");
        let app = root.join("app.xr");
        let other = root.join("other.xr");
        for (path, text) in [
            (&leaf, LEAF),
            (&bridge, BRIDGE),
            (&app, APP),
            (&other, OTHER),
        ] {
            fs::write(path, text).unwrap();
        }
        let mut index = Index::default();
        index.set_roots(&BTreeSet::from([root.clone()]));
        let leaf_uri = index.uri(&leaf).unwrap();
        let bridge_uri = index.uri(&bridge).unwrap();
        let app_uri = index.uri(&app).unwrap();
        let other_uri = index.uri(&other).unwrap();
        let target = |text: &str| Source::new(text).coordinates(text.find("value").unwrap());

        let initial_updates = index.updates.clone();
        fs::remove_file(&app).unwrap();
        let expected = references(&mut index, &leaf_uri, LEAF, "value");
        assert_eq!(expected.len(), 1);
        assert_eq!(expected[0]["uri"], app_uri);
        for _ in 0..2 {
            assert_eq!(references(&mut index, &leaf_uri, LEAF, "value"), expected);
            let rename = index
                .refactor(
                    "textDocument/rename",
                    &leaf_uri,
                    target(LEAF),
                    &json!({"newName":"quantity"}),
                    &Cancellation::default(),
                )
                .unwrap();
            assert_eq!(rename["changes"][&app_uri].as_array().unwrap().len(), 1);
        }
        assert_eq!(index.updates, initial_updates);
        fs::write(&app, APP).unwrap();

        let body_edit = LEAF.replace("value;", "value: 1;");
        index.open(
            leaf_uri.clone(),
            Some(leaf.clone()),
            body_edit.clone().into(),
        );
        assert_eq!(index.update_count(&leaf_uri), 2);
        assert_eq!(index.update_count(&app_uri), 1);
        assert_eq!(index.update_count(&bridge_uri), 1);
        assert_eq!(
            references(&mut index, &leaf_uri, &body_edit, "value"),
            expected
        );

        // Moving the declaration changes its identity, including users of an
        // inherited property in a transitive importer.
        let moved = format!("// unsaved\n{body_edit}");
        index.open(leaf_uri.clone(), Some(leaf.clone()), moved.clone().into());
        assert_eq!(index.update_count(&app_uri), 2);
        assert_eq!(index.update_count(&bridge_uri), 2);
        assert_eq!(index.update_count(&other_uri), 1);
        assert_eq!(references(&mut index, &leaf_uri, &moved, "value"), expected);
        let rename = index
            .refactor(
                "textDocument/rename",
                &leaf_uri,
                target(&moved),
                &json!({"newName":"quantity"}),
                &Cancellation::default(),
            )
            .unwrap();
        assert_eq!(rename["changes"][&leaf_uri][0]["range"]["start"]["line"], 1);

        let saved = LEAF.replace("value", "quantity");
        fs::write(&leaf, &saved).unwrap();
        index.watched(vec![(leaf.clone(), false)]);
        assert_eq!(references(&mut index, &leaf_uri, &moved, "value"), expected);
        index.close(&leaf_uri);
        assert!(references(&mut index, &leaf_uri, &saved, "quantity").is_empty());

        // Recreating a previously missing source restores its importer edges.
        fs::remove_file(&leaf).unwrap();
        index.watched(vec![(leaf.clone(), true)]);
        assert!(!index.files.contains_key(&leaf_uri));
        fs::write(&leaf, LEAF).unwrap();
        index.watched(vec![(leaf.clone(), false)]);
        assert_eq!(references(&mut index, &leaf_uri, LEAF, "value"), expected);
        assert_eq!(index.update_count(&other_uri), 1);

        index.set_roots(&BTreeSet::new());
        assert!(index.files.is_empty());
        assert!(index.references.is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}
