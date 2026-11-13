use crate::{CompilerError, ast, parse};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
use xrack_ir::modules::{ModuleError, resolve_document};

pub fn parse_document(
    source: &str,
    filename: &str,
    declared_sources: Option<&[PathBuf]>,
) -> Result<ast::Document, CompilerError> {
    parse_document_with_inputs(source, filename, declared_sources).map(|(document, _)| document)
}

pub(crate) fn parse_document_with_inputs(
    source: &str,
    filename: &str,
    declared_sources: Option<&[PathBuf]>,
) -> Result<(ast::Document, BTreeSet<PathBuf>), CompilerError> {
    parse_document_with_overrides(source, filename, declared_sources, &BTreeMap::new())
}

pub(crate) fn parse_document_with_overrides(
    source: &str,
    filename: &str,
    declared_sources: Option<&[PathBuf]>,
    buffers: &BTreeMap<PathBuf, String>,
) -> Result<(ast::Document, BTreeSet<PathBuf>), CompilerError> {
    let file = Path::new(filename);
    let entry = parse(source).map_err(|errors| ModuleError::syntax(file, errors))?;
    let buffers = buffers
        .iter()
        .map(|(file, text)| (file.clone(), text.as_str()))
        .collect();
    let mut inputs = BTreeMap::new();
    let document = resolve_document(entry, file, declared_sources, &buffers, &mut inputs, None)?;
    Ok((document, inputs.into_keys().collect()))
}
