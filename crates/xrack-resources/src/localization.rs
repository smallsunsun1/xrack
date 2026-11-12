use crate::{
    EmbeddedTranslations,
    assets::{asset_path, declared_paths, local_path, read_asset},
};
use fluent_syntax::ast::{Entry, Expression, InlineExpression, Pattern, PatternElement};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
use unic_langid::LanguageIdentifier;
use xrack_mlir::{Module, OperationRef};

#[derive(Default)]
pub struct ResolvedTranslations {
    pub catalog: Option<EmbeddedTranslations>,
    pub inputs: BTreeSet<PathBuf>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    default: String,
    locales: BTreeMap<String, String>,
}

pub fn resolve_translations(
    module: &Module,
    filename: &Path,
    declared_assets: Option<&[PathBuf]>,
    buffers: &BTreeMap<PathBuf, String>,
) -> Result<ResolvedTranslations, String> {
    fn collect<'a>(op: OperationRef<'a>, calls: &mut Vec<OperationRef<'a>>) {
        if op.name() == "xr.translate" {
            calls.push(op);
        }
        for child in op.children() {
            collect(child, calls);
        }
    }

    let base = filename.parent().unwrap_or_else(|| Path::new("."));
    let filename = filename.to_string_lossy();
    let mut calls = Vec::new();
    for op in module.operations() {
        collect(op, &mut calls);
    }
    let Some(catalog) = module
        .operations()
        .find(|op| op.name() == "xr.translation_catalog")
    else {
        if let Some(&call) = calls.first() {
            return Err(at(
                call,
                &filename,
                "@tr requires a translation catalog in the entry file",
            ));
        }
        return Ok(ResolvedTranslations::default());
    };

    let declared = declared_paths(declared_assets)?;
    let load = |base: &Path, path: &str, kind| {
        asset_path(base, local_path(path)?, declared.as_ref(), kind)
            .map_err(|error| format!("{path}: {error}"))
    };
    let read = |path: &Path, kind| {
        buffers
            .get(path)
            .cloned()
            .map_or_else(
                || String::from_utf8(read_asset(path, kind)?).map_err(|error| error.to_string()),
                Ok,
            )
            .map_err(|error| format!("{}: {error}", path.display()))
    };
    let manifest_path = load(base, catalog.attr("path")?, "translation catalog")
        .map_err(|error| at(catalog, &filename, &error))?;
    let manifest: Manifest = toml::from_str(&read(&manifest_path, "translation catalog")?)
        .map_err(|error| format!("{}: {error}", manifest_path.display()))?;
    let default = locale_id(&manifest.default)
        .map_err(|error| format!("{}: {error}", manifest_path.display()))?;
    let mut result = ResolvedTranslations::default();
    result.inputs.insert(manifest_path.clone());
    let mut locales = BTreeMap::new();
    for (language, relative) in manifest.locales {
        let language = locale_id(&language)
            .map_err(|error| format!("{}: {error}", manifest_path.display()))?;
        let path = load(
            manifest_path.parent().expect("manifest has a parent"),
            &relative,
            "language pack",
        )
        .map_err(|error| format!("{}: {error}", manifest_path.display()))?;
        let text = read(&path, "language pack")?;
        validate(
            &text,
            &path,
            &language,
            language == default,
            &calls,
            &filename,
        )?;
        if locales.insert(language.clone(), text).is_some() {
            return Err(format!(
                "{}: duplicate language '{language}'",
                manifest_path.display()
            ));
        }
        result.inputs.insert(path);
    }
    if !locales.contains_key(&default) {
        return Err(format!(
            "{}: default language '{default}' has no language pack",
            manifest_path.display()
        ));
    }
    result.catalog = Some(EmbeddedTranslations { default, locales });
    Ok(result)
}

fn locale_id(language: &str) -> Result<String, String> {
    language
        .parse::<LanguageIdentifier>()
        .map(|locale| locale.to_string())
        .map_err(|error| format!("invalid UI locale '{language}': {error}"))
}

fn at(op: OperationRef<'_>, filename: &str, message: &str) -> String {
    let span = op.span();
    format!(
        "{}:{}:{}: {message}",
        op.source_file().unwrap_or(filename),
        span.line,
        span.column
    )
}

type PatternKey<'a> = (bool, &'a str, Option<&'a str>);
type Patterns<'a> = BTreeMap<PatternKey<'a>, &'a Pattern<&'a str>>;

fn validate(
    text: &str,
    path: &Path,
    language: &str,
    default: bool,
    calls: &[OperationRef<'_>],
    filename: &str,
) -> Result<(), String> {
    let resource = fluent_syntax::parser::parse(text).map_err(|(_, errors)| {
        let error = &errors[0];
        let line = text.as_bytes()[..error.pos.start]
            .iter()
            .filter(|&&byte| byte == b'\n')
            .count()
            + 1;
        format!(
            "{}:{line}: invalid Fluent syntax: {}",
            path.display(),
            error.kind
        )
    })?;
    let mut ids = BTreeSet::from(["NUMBER"]);
    let mut patterns = Patterns::new();
    for entry in &resource.body {
        let (term, name, value, attrs) = match entry {
            Entry::Message(message) => (
                false,
                message.id.name,
                message.value.as_ref(),
                &message.attributes,
            ),
            Entry::Term(term) => (true, term.id.name, Some(&term.value), &term.attributes),
            _ => continue,
        };
        if !ids.insert(name) {
            return Err(format!(
                "{}: duplicate or reserved Fluent ID '{name}'",
                path.display()
            ));
        }
        if let Some(value) = value {
            patterns.insert((term, name, None), value);
        }
        for attr in attrs {
            if patterns
                .insert((term, name, Some(attr.id.name)), &attr.value)
                .is_some()
            {
                return Err(format!(
                    "{}: duplicate Fluent attribute '{}.{}'",
                    path.display(),
                    name,
                    attr.id.name
                ));
            }
        }
    }
    let mut requirements = BTreeMap::new();
    for &call in calls {
        let id = call.attr("value")?;
        let key = (false, id, None);
        if !patterns.contains_key(&key) {
            if default {
                return Err(at(
                    call,
                    filename,
                    &format!(
                        "default language '{language}' has no message value for '{id}' in {}",
                        path.display()
                    ),
                ));
            }
            continue;
        }
        let required = match requirements.entry(id) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                let mut check = Variables {
                    patterns: &patterns,
                    active: BTreeSet::new(),
                    visited: BTreeSet::new(),
                    names: BTreeSet::new(),
                };
                check
                    .reference(key, false)
                    .map_err(|error| format!("{}: message '{id}': {error}", path.display()))?;
                entry.insert(check.names)
            }
        };
        let args = call
            .string_array_attribute("arg_names")
            .ok_or("invalid translation argument names")?;
        if let Some(name) = required.iter().find(|name| !args.contains(name)) {
            return Err(at(
                call,
                filename,
                &format!(
                    "translation '{id}' in {} requires argument '{name}'",
                    path.display()
                ),
            ));
        }
    }
    Ok(())
}

struct Variables<'a, 'p> {
    patterns: &'p Patterns<'a>,
    active: BTreeSet<PatternKey<'a>>,
    visited: BTreeSet<(PatternKey<'a>, bool)>,
    names: BTreeSet<&'a str>,
}

impl<'a> Variables<'a, '_> {
    fn reference(&mut self, key: PatternKey<'a>, local: bool) -> Result<(), String> {
        if self.active.contains(&key) || self.active.len() >= 64 {
            return Err(format!(
                "recursive or deeply nested reference to '{}'",
                key.1
            ));
        }
        if !self.visited.insert((key, local)) {
            return Ok(());
        }
        let pattern = self
            .patterns
            .get(&key)
            .ok_or_else(|| format!("unknown Fluent reference '{}'", key.1))?;
        self.active.insert(key);
        self.pattern(pattern, local)?;
        self.active.remove(&key);
        Ok(())
    }

    fn pattern(&mut self, pattern: &'a Pattern<&'a str>, local: bool) -> Result<(), String> {
        for element in &pattern.elements {
            if let PatternElement::Placeable { expression } = element {
                self.expression(expression, local)?;
            }
        }
        Ok(())
    }

    fn expression(&mut self, expr: &'a Expression<&'a str>, local: bool) -> Result<(), String> {
        match expr {
            Expression::Inline(expr) => self.inline(expr, local),
            Expression::Select { selector, variants } => {
                self.inline(selector, local)?;
                for variant in variants {
                    self.pattern(&variant.value, local)?;
                }
                Ok(())
            }
        }
    }

    fn inline(&mut self, expr: &'a InlineExpression<&'a str>, local: bool) -> Result<(), String> {
        use InlineExpression::*;
        let arguments = match expr {
            VariableReference { id } => {
                if !local {
                    self.names.insert(id.name);
                }
                return Ok(());
            }
            Placeable { expression } => return self.expression(expression, local),
            MessageReference { id, attribute } => {
                return self.reference(
                    (false, id.name, attribute.as_ref().map(|id| id.name)),
                    local,
                );
            }
            TermReference {
                id,
                attribute,
                arguments,
            } => {
                // Fluent resolves variables inside a term using its own named arguments.
                self.reference((true, id.name, attribute.as_ref().map(|id| id.name)), true)?;
                arguments.as_ref()
            }
            FunctionReference { id, arguments } if id.name == "NUMBER" => Some(arguments),
            FunctionReference { id, .. } => {
                return Err(format!("unsupported Fluent function '{}'", id.name));
            }
            StringLiteral { .. } | NumberLiteral { .. } => None,
        };
        if let Some(arguments) = arguments {
            for arg in &arguments.positional {
                self.inline(arg, local)?;
            }
            for arg in &arguments.named {
                self.inline(&arg.value, local)?;
            }
        }
        Ok(())
    }
}
