use crate::semantic;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
};
use xrack_parser::{ast, parse};

#[derive(Debug)]
pub enum ModuleError {
    Diagnostics {
        filename: String,
        diagnostics: Vec<ast::Diagnostic>,
        syntax: bool,
    },
    Io {
        path: PathBuf,
        source: io::Error,
    },
}

impl ModuleError {
    fn at(file: &Path, span: ast::Span, message: impl Into<String>) -> Self {
        Self::Diagnostics {
            filename: file.to_string_lossy().into_owned(),
            diagnostics: vec![ast::Diagnostic {
                span,
                message: message.into(),
            }],
            syntax: false,
        }
    }

    pub fn syntax(file: &Path, diagnostics: Vec<ast::Diagnostic>) -> Self {
        Self::Diagnostics {
            filename: file.to_string_lossy().into_owned(),
            diagnostics,
            syntax: true,
        }
    }
}

/// Resolve imports with optional editor buffers and source provenance.
/// `inputs` also records attempted reads so failed imports can be invalidated.
pub fn resolve_document(
    entry: ast::Document,
    file: &Path,
    declared_sources: Option<&[PathBuf]>,
    buffers: &BTreeMap<PathBuf, &str>,
    inputs: &mut BTreeMap<PathBuf, ast::Span>,
    source_files: Option<&mut Vec<PathBuf>>,
) -> Result<ast::Document, ModuleError> {
    resolve(
        entry,
        file,
        declared_sources,
        buffers,
        inputs,
        source_files,
        None,
    )
}

/// Resolve editor imports against a single set of parsed source snapshots.
pub fn resolve_indexed_document(
    entry: ast::Document,
    file: &Path,
    parsed: &BTreeMap<PathBuf, &ast::Document>,
    inputs: &mut BTreeMap<PathBuf, ast::Span>,
    source_files: &mut Vec<PathBuf>,
) -> Result<ast::Document, ModuleError> {
    resolve(
        entry,
        file,
        None,
        &BTreeMap::new(),
        inputs,
        Some(source_files),
        Some(parsed),
    )
}

fn resolve(
    entry: ast::Document,
    file: &Path,
    declared_sources: Option<&[PathBuf]>,
    buffers: &BTreeMap<PathBuf, &str>,
    inputs: &mut BTreeMap<PathBuf, ast::Span>,
    source_files: Option<&mut Vec<PathBuf>>,
    parsed: Option<&BTreeMap<PathBuf, &ast::Document>>,
) -> Result<ast::Document, ModuleError> {
    if let Some(extra) = entry.translations.get(1) {
        return Err(ModuleError::at(
            file,
            extra.span,
            "only one translation catalog can be declared",
        ));
    }
    if entry.imports.is_empty() {
        return Ok(entry);
    }
    let directory = file
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let base = if source_files.is_some() {
        normalize_source_path(directory)
    } else {
        directory.canonicalize().map_err(|source| ModuleError::Io {
            path: file.into(),
            source,
        })?
    };
    let declared = declared_sources
        .map(|sources| {
            sources
                .iter()
                .map(|path| {
                    path.canonicalize().map_err(|source| ModuleError::Io {
                        path: path.clone(),
                        source,
                    })
                })
                .collect::<Result<BTreeSet<_>, _>>()
        })
        .transpose()?;
    let mut loader = Loader {
        buffers,
        inputs,
        source_files,
        parsed,
        base,
        declared,
        active: BTreeSet::new(),
        exports: BTreeMap::new(),
        used_names: BTreeSet::new(),
        next_module: 0,
        components: Vec::new(),
        records: Vec::new(),
        fonts: Vec::new(),
        global_symbols: BTreeSet::new(),
    };
    loader.active.insert(normalize_source_path(file));
    let translations = entry.translations.clone();
    loader.resolve(entry, file, None)?;
    Ok(ast::Document {
        imports: Vec::new(),
        fonts: loader.fonts,
        translations,
        records: loader.records,
        components: loader.components,
    })
}

/// Canonicalize existing paths and preserve paths of new editor files.
pub fn normalize_source_path(path: &Path) -> PathBuf {
    if let Ok(path) = path.canonicalize() {
        return path;
    }
    if let Some((parent, filename)) = path.parent().zip(path.file_name()) {
        let parent = if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        };
        return normalize_source_path(parent).join(filename);
    }
    path.to_owned()
}

struct Loader<'a> {
    buffers: &'a BTreeMap<PathBuf, &'a str>,
    inputs: &'a mut BTreeMap<PathBuf, ast::Span>,
    source_files: Option<&'a mut Vec<PathBuf>>,
    parsed: Option<&'a BTreeMap<PathBuf, &'a ast::Document>>,
    base: PathBuf,
    declared: Option<BTreeSet<PathBuf>>,
    active: BTreeSet<PathBuf>,
    exports: BTreeMap<PathBuf, BTreeMap<String, String>>,
    used_names: BTreeSet<String>,
    next_module: usize,
    components: Vec<ast::Component>,
    records: Vec<ast::RecordDef>,
    fonts: Vec<ast::FontImport>,
    global_symbols: BTreeSet<String>,
}

impl Loader<'_> {
    fn resolve(
        &mut self,
        doc: ast::Document,
        file: &Path,
        entry_import: Option<ast::Span>,
    ) -> Result<BTreeMap<String, String>, ModuleError> {
        let entry = entry_import.is_none();
        if !entry && let Some(declaration) = doc.translations.first() {
            return Err(ModuleError::at(
                file,
                declaration.span,
                "declare the translation catalog in the entry file",
            ));
        }
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
                return Err(ModuleError::at(
                    file,
                    span,
                    format!("{kind} name `{name}` conflicts with a built-in element"),
                ));
            }
            if !local_names.insert(semantic::rust_name(name)) {
                return Err(ModuleError::at(
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
            let diagnostic = |message| ModuleError::at(file, import.span, message);
            let path: String = serde_json::from_str(&import.path)
                .map_err(|error| diagnostic(format!("invalid import path: {error}")))?;
            let relative = Path::new(&path);
            if relative.is_absolute()
                || relative.extension().and_then(|ext| ext.to_str()) != Some("xr")
            {
                return Err(diagnostic("imports require a relative .xr path".into()));
            }
            let input = file.parent().unwrap_or(Path::new(".")).join(relative);
            let canonical = normalize_source_path(&input);
            let entry_import = entry_import.unwrap_or(import.span);
            self.inputs.entry(canonical.clone()).or_insert(entry_import);
            if self.parsed.is_none() && !self.buffers.contains_key(&canonical) {
                input
                    .canonicalize()
                    .map_err(|error| diagnostic(format!("could not open {path:?}: {error}")))?;
            }
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
                let dependency = if let Some(parsed) = self.parsed {
                    parsed
                        .get(&canonical)
                        .map(|document| (*document).clone())
                        .ok_or_else(|| {
                            diagnostic(format!("{path:?} has no valid indexed source"))
                        })?
                } else {
                    let disk;
                    let source = if let Some(source) = self.buffers.get(&canonical) {
                        *source
                    } else {
                        disk = fs::read_to_string(&canonical).map_err(|error| {
                            diagnostic(format!("could not read {path:?}: {error}"))
                        })?;
                        &disk
                    };
                    parse(source).map_err(|diagnostics| ModuleError::syntax(&input, diagnostics))?
                };
                let exports = self.resolve(dependency, &input, Some(entry_import))?;
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

        let asset_prefix = if entry || self.source_files.is_some() {
            PathBuf::new()
        } else {
            let directory = file.parent().unwrap_or(Path::new("."));
            let actual = directory.canonicalize().map_err(|source| ModuleError::Io {
                path: file.into(),
                source,
            })?;
            relative_directory(&self.base, &actual)
        };
        let source = self.source_files.as_mut().map_or(0, |files| {
            files.push(normalize_source_path(file));
            files.len()
        });
        for mut font in doc.fonts {
            rebase_asset(&mut font.path, &asset_prefix);
            font.filename = Some(file.to_string_lossy().into_owned());
            self.fonts.push(font);
        }
        for mut record in doc.records {
            record.span.source = source;
            record.name = names[&record.name].clone();
            for field in &mut record.fields {
                field.span.source = source;
                field.type_span.source = source;
                if let ast::Type::Record(name) = &field.ty
                    && !names.contains_key(name.as_ref())
                {
                    return Err(ModuleError::at(
                        file,
                        field.span,
                        format!("unknown record type `{name}`"),
                    ));
                }
                rewrite_record_type(&mut field.ty, &names);
            }
            self.records.push(record);
        }
        for component in &doc.components {
            if component.global {
                self.global_symbols.insert(names[&component.name].clone());
            }
        }
        let globals: BTreeMap<_, _> = names
            .iter()
            .filter(|(_, resolved)| self.global_symbols.contains(*resolved))
            .map(|(name, resolved)| (name.clone(), resolved.clone()))
            .collect();
        for mut component in doc.components {
            component.span.source = source;
            component.base_span.source = source;
            component.name = names[&component.name].clone();
            component.exported &= entry;
            component.filename = Some(file.to_string_lossy().into_owned());
            rewrite_type(&mut component.base, &names, file, component.base_span)?;
            rewrite_members(&mut component.members, &names, &asset_prefix, file, source)?;
            rewrite_global_members(&mut component.members, &globals);
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
) -> Result<(), ModuleError> {
    if let Some(symbol) = names.get(name) {
        *name = symbol.clone();
    } else if !semantic::is_builtin(name) {
        return Err(ModuleError::at(
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
    source: usize,
) -> Result<(), ModuleError> {
    for member in members {
        match member {
            ast::Member::Property(property) => {
                property.span.source = source;
                property.type_span.source = source;
                rewrite_record_type(&mut property.ty, names);
                if let Some(value) = &mut property.value {
                    rewrite_expr(value, asset_prefix, names, source);
                }
            }
            ast::Member::Binding(binding) => {
                binding.span.source = source;
                rewrite_expr(&mut binding.value, asset_prefix, names, source);
            }
            ast::Member::Handler(handler) => {
                handler.span.source = source;
                for span in &mut handler.parameter_spans {
                    span.source = source;
                }
                for statement in &mut handler.statements {
                    match statement {
                        ast::Statement::Assignment(assignment) => {
                            assignment.span.source = source;
                            rewrite_expr(&mut assignment.value, asset_prefix, names, source)
                        }
                        ast::Statement::Call(call) => {
                            call.span.source = source;
                            for argument in &mut call.arguments {
                                rewrite_expr(argument, asset_prefix, names, source);
                            }
                        }
                    }
                }
                if let Some(value) = &mut handler.result {
                    rewrite_expr(value, asset_prefix, names, source);
                }
            }
            ast::Member::Element(element) => {
                element.span.source = source;
                element.type_span.source = source;
                rewrite_type(&mut element.ty, names, file, element.type_span)?;
                if let Some(condition) = &mut element.condition {
                    rewrite_expr(condition, asset_prefix, names, source);
                }
                if let Some(repeat) = &mut element.repeat {
                    repeat.item_span.source = source;
                    repeat.index_span.source = source;
                    rewrite_expr(&mut repeat.model, asset_prefix, names, source);
                }
                rewrite_members(&mut element.members, names, asset_prefix, file, source)?;
            }
            ast::Member::Callback(callback) => {
                callback.span.source = source;
                for span in &mut callback.parameter_type_spans {
                    span.source = source;
                }
                if let Some(span) = &mut callback.result_type_span {
                    span.source = source;
                }
                if let Some(span) = &mut callback.alias_span {
                    span.source = source;
                }
                for parameter in &mut callback.parameters {
                    rewrite_record_type(&mut parameter.ty, names);
                }
                if let Some(result) = &mut callback.return_type {
                    rewrite_record_type(result, names);
                }
            }
            ast::Member::Children(span) => span.source = source,
            ast::Member::Animation(animation) => {
                rewrite_animation(animation, asset_prefix, names, source);
            }
            ast::Member::States(block) => {
                block.span.source = source;
                for state in &mut block.states {
                    state.span.source = source;
                    if let Some(condition) = &mut state.when {
                        rewrite_expr(condition, asset_prefix, names, source);
                    }
                    for change in &mut state.changes {
                        change.span.source = source;
                        rewrite_expr(&mut change.value, asset_prefix, names, source);
                    }
                    for animation in state
                        .transitions
                        .iter_mut()
                        .flat_map(|transition| &mut transition.animations)
                    {
                        rewrite_animation(animation, asset_prefix, names, source);
                    }
                }
            }
        }
    }
    Ok(())
}

fn rewrite_animation(
    animation: &mut ast::Animation,
    prefix: &Path,
    names: &BTreeMap<String, String>,
    source: usize,
) {
    animation.span.source = source;
    for option in &mut animation.options {
        option.span.source = source;
        if let ast::AnimationValue::Expression(value) = &mut option.value {
            rewrite_expr(value, prefix, names, source);
        }
    }
}

fn rebase_asset(spelling: &mut String, prefix: &Path) {
    if let Ok(url) = serde_json::from_str::<String>(spelling)
        && !url.is_empty()
        && Path::new(&url).is_relative()
        && !url.contains(':')
    {
        let path = prefix.join(url);
        let path = path.to_string_lossy();
        #[cfg(windows)]
        let path = path.replace('\\', "/");
        *spelling = serde_json::to_string(&path).unwrap();
    }
}

fn rewrite_expr(
    expr: &mut ast::Expr,
    prefix: &Path,
    names: &BTreeMap<String, String>,
    source: usize,
) {
    expr.span.source = source;
    use ast::ExprKind::*;
    match &mut expr.kind {
        ImageUrl(spelling) | SlicedImageUrl { path: spelling, .. } => {
            rebase_asset(spelling, prefix)
        }
        Unary { value, .. } => rewrite_expr(value, prefix, names, source),
        Binary { left, right, .. } => {
            rewrite_expr(left, prefix, names, source);
            rewrite_expr(right, prefix, names, source);
        }
        Conditional { condition, yes, no } => {
            rewrite_expr(condition, prefix, names, source);
            rewrite_expr(yes, prefix, names, source);
            rewrite_expr(no, prefix, names, source);
        }
        Array(items) => {
            for item in items {
                rewrite_expr(item, prefix, names, source);
            }
        }
        Record { name, fields } => {
            if let Some(symbol) = names.get(name) {
                *name = symbol.clone();
            }
            for field in fields {
                field.span.source = source;
                rewrite_expr(&mut field.value, prefix, names, source);
            }
        }
        Translation { arguments, .. } => {
            for argument in arguments {
                rewrite_expr(&mut argument.value, prefix, names, source);
            }
        }
        Call(call) => {
            call.span.source = source;
            for argument in &mut call.arguments {
                rewrite_expr(argument, prefix, names, source);
            }
        }
        LinearGradient { stops, .. } | RadialGradient { stops } => {
            for stop in stops {
                stop.span.source = source;
                rewrite_expr(&mut stop.color, prefix, names, source);
            }
        }
        Literal { .. } | Reference(_) | Markdown(_) => {}
    }
}

fn rewrite_global_reference(reference: &mut String, globals: &BTreeMap<String, String>) {
    if let Some((receiver, rest)) = reference.split_once('.')
        && let Some(symbol) = globals.get(receiver)
        && symbol != receiver
    {
        *reference = format!("{receiver}@{symbol}.{rest}");
    }
}

fn rewrite_global_expr(expr: &mut ast::Expr, globals: &BTreeMap<String, String>) {
    use ast::ExprKind::*;
    match &mut expr.kind {
        Reference(reference) => rewrite_global_reference(reference, globals),
        Call(call) => {
            rewrite_global_reference(&mut call.callee, globals);
            for value in &mut call.arguments {
                rewrite_global_expr(value, globals);
            }
        }
        Unary { value, .. } => rewrite_global_expr(value, globals),
        Binary { left, right, .. } => {
            rewrite_global_expr(left, globals);
            rewrite_global_expr(right, globals);
        }
        Conditional { condition, yes, no } => {
            for value in [condition, yes, no] {
                rewrite_global_expr(value, globals);
            }
        }
        Array(values) => {
            for value in values {
                rewrite_global_expr(value, globals);
            }
        }
        Record { fields, .. }
        | Translation {
            arguments: fields, ..
        } => {
            for field in fields {
                rewrite_global_expr(&mut field.value, globals);
            }
        }
        LinearGradient { stops, .. } | RadialGradient { stops } => {
            for stop in stops {
                rewrite_global_expr(&mut stop.color, globals);
            }
        }
        _ => {}
    }
}

fn rewrite_global_members(members: &mut [ast::Member], globals: &BTreeMap<String, String>) {
    for member in members {
        match member {
            ast::Member::Property(property) => {
                if let Some(value) = &mut property.value {
                    rewrite_global_expr(value, globals);
                }
            }
            ast::Member::Binding(binding) => rewrite_global_expr(&mut binding.value, globals),
            ast::Member::Callback(callback) => {
                if let Some(alias) = &mut callback.alias {
                    rewrite_global_reference(alias, globals);
                }
            }
            ast::Member::Handler(handler) => {
                for statement in &mut handler.statements {
                    match statement {
                        ast::Statement::Assignment(assignment) => {
                            rewrite_global_reference(&mut assignment.target, globals);
                            rewrite_global_expr(&mut assignment.value, globals);
                        }
                        ast::Statement::Call(call) => {
                            rewrite_global_reference(&mut call.callee, globals);
                            for value in &mut call.arguments {
                                rewrite_global_expr(value, globals);
                            }
                        }
                    }
                }
                if let Some(value) = &mut handler.result {
                    rewrite_global_expr(value, globals);
                }
            }
            ast::Member::Element(element) => {
                if let Some(value) = &mut element.condition {
                    rewrite_global_expr(value, globals);
                }
                if let Some(repeat) = &mut element.repeat {
                    rewrite_global_expr(&mut repeat.model, globals);
                }
                rewrite_global_members(&mut element.members, globals);
            }
            ast::Member::Animation(animation) => {
                for option in &mut animation.options {
                    if let ast::AnimationValue::Expression(value) = &mut option.value {
                        rewrite_global_expr(value, globals);
                    }
                }
            }
            ast::Member::States(states) => {
                for state in &mut states.states {
                    if let Some(value) = &mut state.when {
                        rewrite_global_expr(value, globals);
                    }
                    for change in &mut state.changes {
                        rewrite_global_expr(&mut change.value, globals);
                    }
                    for animation in state
                        .transitions
                        .iter_mut()
                        .flat_map(|transition| &mut transition.animations)
                    {
                        for option in &mut animation.options {
                            if let ast::AnimationValue::Expression(value) = &mut option.value {
                                rewrite_global_expr(value, globals);
                            }
                        }
                    }
                }
            }
            ast::Member::Children(_) => {}
        }
    }
}
