use crate::{CompilerError, ast, parse, semantic};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

pub fn parse_document(
    source: &str,
    filename: &str,
    declared_sources: Option<&[PathBuf]>,
) -> Result<ast::Document, CompilerError> {
    let entry = parse(source).map_err(|diagnostics| CompilerError::Diagnostics {
        filename: filename.into(),
        diagnostics,
    })?;
    if entry.imports.is_empty() {
        return Ok(entry);
    }
    let file = Path::new(filename);
    let directory = file
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let base = directory
        .canonicalize()
        .map_err(|source| CompilerError::Io {
            path: file.into(),
            source,
        })?;
    let declared = declared_sources
        .map(|sources| {
            sources
                .iter()
                .map(|path| {
                    path.canonicalize().map_err(|source| CompilerError::Io {
                        path: path.clone(),
                        source,
                    })
                })
                .collect::<Result<BTreeSet<_>, _>>()
        })
        .transpose()?;
    let mut loader = Loader {
        base,
        declared,
        active: BTreeSet::new(),
        exports: BTreeMap::new(),
        used_names: BTreeSet::new(),
        next_module: 0,
        components: Vec::new(),
        records: Vec::new(),
    };
    if let Ok(canonical) = file.canonicalize() {
        loader.active.insert(canonical);
    }
    loader.resolve(entry, file, true)?;
    Ok(ast::Document {
        imports: Vec::new(),
        records: loader.records,
        components: loader.components,
    })
}

struct Loader {
    base: PathBuf,
    declared: Option<BTreeSet<PathBuf>>,
    active: BTreeSet<PathBuf>,
    exports: BTreeMap<PathBuf, BTreeMap<String, String>>,
    used_names: BTreeSet<String>,
    next_module: usize,
    components: Vec<ast::Component>,
    records: Vec<ast::RecordDef>,
}

impl Loader {
    fn resolve(
        &mut self,
        doc: ast::Document,
        file: &Path,
        entry: bool,
    ) -> Result<BTreeMap<String, String>, CompilerError> {
        let mut names = BTreeMap::new();
        let mut exports = BTreeMap::new();
        let mut local_names = BTreeSet::new();
        let components = doc
            .components
            .iter()
            .map(|c| (&c.name, c.span, c.exported, "component"));
        let records = doc
            .records
            .iter()
            .map(|r| (&r.name, r.span, r.exported, "record"));
        for (name, span, exported, kind) in components.chain(records) {
            if semantic::is_builtin(name) {
                return Err(CompilerError::at(
                    file,
                    span,
                    format!("{kind} name `{name}` conflicts with a built-in element"),
                ));
            }
            if !local_names.insert(semantic::rust_name(name)) {
                return Err(CompilerError::at(
                    file,
                    span,
                    format!("duplicate {kind} `{name}`"),
                ));
            }
            let symbol = if entry {
                self.used_names.insert(semantic::rust_name(name));
                name.clone()
            } else {
                loop {
                    let candidate = format!("XrImport{}-{name}", self.next_module);
                    self.next_module += 1;
                    if self.used_names.insert(semantic::rust_name(&candidate)) {
                        break candidate;
                    }
                }
            };
            names.insert(name.clone(), symbol.clone());
            if exported {
                exports.insert(name.clone(), symbol);
            }
        }

        for import in doc.imports {
            let diagnostic = |message| CompilerError::at(file, import.span, message);
            let path: String = serde_json::from_str(&import.path)
                .map_err(|error| diagnostic(format!("invalid import path: {error}")))?;
            let relative = Path::new(&path);
            if relative.is_absolute()
                || relative.extension().and_then(|ext| ext.to_str()) != Some("xr")
            {
                return Err(diagnostic("imports require a relative .xr path".into()));
            }
            let input = file.parent().unwrap_or(Path::new(".")).join(relative);
            let canonical = input
                .canonicalize()
                .map_err(|error| diagnostic(format!("could not open {path:?}: {error}")))?;
            if self.active.contains(&canonical) {
                return Err(diagnostic(format!("cyclic import of {path:?}")));
            }
            if self
                .declared
                .as_ref()
                .is_some_and(|inputs| !inputs.contains(&canonical))
            {
                return Err(diagnostic(format!(
                    "{path:?} is not declared in xr_ui_library(sources = [...])"
                )));
            }
            if !self.exports.contains_key(&canonical) {
                self.active.insert(canonical.clone());
                let source = fs::read_to_string(&canonical)
                    .map_err(|error| diagnostic(format!("could not read {path:?}: {error}")))?;
                let dependency =
                    parse(&source).map_err(|diagnostics| CompilerError::Diagnostics {
                        filename: input.to_string_lossy().into_owned(),
                        diagnostics,
                    })?;
                let exports = self.resolve(dependency, &input, false)?;
                self.active.remove(&canonical);
                self.exports.insert(canonical.clone(), exports);
            }
            for name in import.names {
                let symbol = self.exports[&canonical].get(&name.name).ok_or_else(|| {
                    diagnostic(format!("{path:?} does not export `{}`", name.name))
                })?;
                let local = name.alias.as_ref().unwrap_or(&name.name);
                if !local_names.insert(semantic::rust_name(local)) {
                    return Err(diagnostic(format!(
                        "duplicate component or import name `{local}`"
                    )));
                }
                names.insert(local.clone(), symbol.clone());
            }
        }

        let asset_prefix = if entry {
            PathBuf::new()
        } else {
            let directory = file.parent().unwrap_or(Path::new("."));
            let actual = directory
                .canonicalize()
                .map_err(|source| CompilerError::Io {
                    path: file.into(),
                    source,
                })?;
            relative_directory(&self.base, &actual)
        };
        for mut record in doc.records {
            record.name = names[&record.name].clone();
            for field in &mut record.fields {
                rewrite_record_type(&mut field.ty, &names);
            }
            self.records.push(record);
        }
        for mut component in doc.components {
            component.name = names[&component.name].clone();
            component.exported &= entry;
            component.filename = Some(file.to_string_lossy().into_owned());
            rewrite_type(&mut component.base, &names, file, component.span)?;
            rewrite_members(&mut component.members, &names, &asset_prefix, file)?;
            self.components.push(component);
        }
        Ok(exports)
    }
}

fn rewrite_record_type(ty: &mut ast::Type, names: &BTreeMap<String, String>) {
    if let ast::Type::Record(name) | ast::Type::RecordList(name) = ty
        && let Some(symbol) = names.get(name.as_ref())
    {
        *name = symbol.as_str().into();
    }
}

fn relative_directory(base: &Path, directory: &Path) -> PathBuf {
    let common = base
        .components()
        .zip(directory.components())
        .take_while(|(a, b)| a == b)
        .count();
    let mut path: PathBuf = std::iter::repeat_n("..", base.components().count() - common).collect();
    path.extend(directory.components().skip(common));
    path
}

fn rewrite_type(
    name: &mut String,
    names: &BTreeMap<String, String>,
    file: &Path,
    span: ast::Span,
) -> Result<(), CompilerError> {
    if let Some(symbol) = names.get(name) {
        *name = symbol.clone();
    } else if !semantic::is_builtin(name) {
        return Err(CompilerError::at(
            file,
            span,
            format!("component or element `{name}` is not in scope"),
        ));
    }
    Ok(())
}

fn rewrite_members(
    members: &mut [ast::Member],
    names: &BTreeMap<String, String>,
    asset_prefix: &Path,
    file: &Path,
) -> Result<(), CompilerError> {
    for member in members {
        match member {
            ast::Member::Property(property) => {
                rewrite_record_type(&mut property.ty, names);
                if let Some(value) = &mut property.value {
                    rewrite_expr(value, asset_prefix, names);
                }
            }
            ast::Member::Binding(binding) => rewrite_expr(&mut binding.value, asset_prefix, names),
            ast::Member::Handler(handler) => {
                for statement in &mut handler.statements {
                    match statement {
                        ast::Statement::Assignment(assignment) => {
                            rewrite_expr(&mut assignment.value, asset_prefix, names)
                        }
                        ast::Statement::Call(call) => {
                            for argument in &mut call.arguments {
                                rewrite_expr(argument, asset_prefix, names);
                            }
                        }
                    }
                }
                if let Some(value) = &mut handler.result {
                    rewrite_expr(value, asset_prefix, names);
                }
            }
            ast::Member::Element(element) => {
                rewrite_type(&mut element.ty, names, file, element.span)?;
                if let Some(condition) = &mut element.condition {
                    rewrite_expr(condition, asset_prefix, names);
                }
                if let Some(repeat) = &mut element.repeat {
                    rewrite_expr(&mut repeat.model, asset_prefix, names);
                }
                rewrite_members(&mut element.members, names, asset_prefix, file)?;
            }
            ast::Member::Callback(callback) => {
                for parameter in &mut callback.parameters {
                    rewrite_record_type(&mut parameter.ty, names);
                }
                if let Some(result) = &mut callback.return_type {
                    rewrite_record_type(result, names);
                }
            }
            ast::Member::Animation(_) => {}
        }
    }
    Ok(())
}

fn rewrite_expr(expr: &mut ast::Expr, prefix: &Path, names: &BTreeMap<String, String>) {
    use ast::ExprKind::*;
    match &mut expr.kind {
        ImageUrl(spelling) => {
            if let Ok(url) = serde_json::from_str::<String>(spelling)
                && !url.is_empty()
                && Path::new(&url).is_relative()
                && !url.contains(':')
            {
                *spelling = serde_json::to_string(&prefix.join(url).to_string_lossy()).unwrap();
            }
        }
        Unary { value, .. } => rewrite_expr(value, prefix, names),
        Binary { left, right, .. } => {
            rewrite_expr(left, prefix, names);
            rewrite_expr(right, prefix, names);
        }
        Conditional { condition, yes, no } => {
            rewrite_expr(condition, prefix, names);
            rewrite_expr(yes, prefix, names);
            rewrite_expr(no, prefix, names);
        }
        Array(items) => {
            for item in items {
                rewrite_expr(item, prefix, names);
            }
        }
        Record { name, fields } => {
            if let Some(symbol) = names.get(name) {
                *name = symbol.clone();
            }
            for field in fields {
                rewrite_expr(&mut field.value, prefix, names);
            }
        }
        Call(call) => {
            for argument in &mut call.arguments {
                rewrite_expr(argument, prefix, names);
            }
        }
        LinearGradient { stops, .. } | RadialGradient { stops } => {
            for stop in stops {
                rewrite_expr(&mut stop.color, prefix, names);
            }
        }
        Literal { .. } | Reference(_) => {}
    }
}
