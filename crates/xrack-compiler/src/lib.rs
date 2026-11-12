//! Compiler orchestration: ANTLR4 → AST → XR dialect → Rust.
use proc_macro2::TokenStream;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

mod error;
mod modules;
pub use error::CompilerError;
pub use modules::parse_document;

pub use xrack_codegen as codegen;
pub use xrack_ir as ir;
pub use xrack_ir::semantic;
pub use xrack_mlir as mlir;
pub use xrack_parser::{ast, parse};
use xrack_resources::{
    PrepareMlirError, ResolvedTranslations, prepare_mlir, resolve_assets, resolve_translations,
};

pub struct Compilation {
    pub rust: TokenStream,
    pub mlir: String,
}

pub fn compile(source: &str, filename: &str) -> Result<Compilation, CompilerError> {
    let doc = parse_document(source, filename, None)?;
    let (processed, translations) = prepare(&doc, filename, None, &BTreeMap::new())?;
    let mut inputs = BTreeSet::new();
    Ok(Compilation {
        rust: emit_with_inputs(&processed, filename, None, translations, &mut inputs)?,
        mlir: processed.to_mlir(),
    })
}

pub struct Preview {
    pub rust: TokenStream,
    pub components: Vec<String>,
    pub inputs: BTreeSet<PathBuf>,
}

pub struct PreviewModule {
    pub module: mlir::Module,
    pub components: Vec<String>,
    pub inputs: BTreeSet<PathBuf>,
    translations: ResolvedTranslations,
}

pub struct PreviewAssets {
    pub fonts: Vec<Vec<u8>>,
    pub images: BTreeMap<String, codegen::EmbeddedImage>,
    pub inputs: BTreeSet<PathBuf>,
}

pub fn load_preview_assets(
    module: &mlir::Module,
    filename: &str,
) -> Result<PreviewAssets, CompilerError> {
    let assets =
        resolve_assets(module, Path::new(filename), None).map_err(CompilerError::Resources)?;
    codegen::check_nine_slices(module, &assets.images).map_err(CompilerError::Resources)?;
    Ok(PreviewAssets {
        fonts: assets.fonts,
        images: assets.images,
        inputs: assets.inputs,
    })
}

impl PreviewModule {
    pub fn take_translation_catalog(&mut self) -> Option<codegen::EmbeddedTranslations> {
        self.translations.catalog.take()
    }
}

/// Prepare the editor buffer and its open imports for either preview backend.
pub fn prepare_preview(
    source: &str,
    filename: &str,
    buffers: BTreeMap<PathBuf, String>,
) -> Result<PreviewModule, CompilerError> {
    let buffers = buffers
        .into_iter()
        .filter_map(|(path, text)| Some((path.canonicalize().ok()?, text)))
        .collect();
    let (doc, mut inputs) =
        modules::parse_document_with_overrides(source, filename, None, &buffers)?;
    let mut components: Vec<_> = doc
        .components
        .iter()
        .filter(|component| {
            component
                .filename
                .as_deref()
                .is_none_or(|path| path == filename)
        })
        .collect();
    // Default to an exported entry component, retaining source order among peers.
    components.sort_by_key(|component| !component.exported);
    let components = components
        .iter()
        .map(|component| component.name.clone())
        .collect();
    let (module, translations) = prepare(&doc, filename, None, &buffers)?;
    inputs.insert(filename.into());
    inputs.extend(translations.inputs.iter().cloned());
    Ok(PreviewModule {
        module,
        components,
        inputs,
        translations,
    })
}

/// Prepare standalone XR dialect IR and resolve assets relative to its file.
pub fn prepare_mlir_preview(source: &str, filename: &str) -> Result<PreviewModule, CompilerError> {
    let (module, translations) =
        prepare_mlir(source, Path::new(filename)).map_err(|error| match error {
            PrepareMlirError::Mlir(message) => CompilerError::Mlir(message),
            PrepareMlirError::Resources(message) => CompilerError::Resources(message),
        })?;
    let components = module
        .operations()
        .filter(|op| op.name() == "xr.component" && op.attribute("exported") == Some("true"))
        .filter_map(|op| op.attribute("sym_name").map(str::to_owned))
        .collect();
    let mut inputs = BTreeSet::from([filename.into()]);
    inputs.extend(translations.inputs.iter().cloned());
    Ok(PreviewModule {
        module,
        components,
        inputs,
        translations,
    })
}

/// Generate a snapshot host interface for the editor buffer and its open imports.
pub fn compile_preview(
    source: &str,
    filename: &str,
    buffers: BTreeMap<PathBuf, String>,
) -> Result<Preview, CompilerError> {
    let PreviewModule {
        module,
        components,
        mut inputs,
        translations,
    } = prepare_preview(source, filename, buffers)?;
    let mut rust = emit_with_inputs(&module, filename, None, translations, &mut inputs)?;
    rust.extend(codegen::preview_factory(
        components.iter().map(String::as_str),
    ));
    Ok(Preview {
        rust,
        components,
        inputs,
    })
}

pub fn generate_rust(
    module: &mlir::Module,
    filename: &str,
    declared_assets: Option<&[PathBuf]>,
) -> Result<TokenStream, CompilerError> {
    let translations = resolve_translations(
        module,
        Path::new(filename),
        declared_assets,
        &BTreeMap::new(),
    )
    .map_err(CompilerError::Resources)?;
    emit_with_inputs(
        module,
        filename,
        declared_assets,
        translations,
        &mut BTreeSet::new(),
    )
}

pub fn generate_document(
    document: &ast::Document,
    filename: &str,
    declared_assets: Option<&[PathBuf]>,
) -> Result<TokenStream, CompilerError> {
    let (module, translations) = prepare(document, filename, declared_assets, &BTreeMap::new())?;
    emit_with_inputs(
        &module,
        filename,
        declared_assets,
        translations,
        &mut BTreeSet::new(),
    )
}

/// Compile a UI for a Cargo build script and report all source and asset files read.
pub fn compile_with_inputs(
    source: &str,
    filename: &str,
    declared_sources: Option<&[PathBuf]>,
    declared_assets: Option<&[PathBuf]>,
) -> Result<(TokenStream, BTreeSet<PathBuf>), CompilerError> {
    let (document, mut inputs) =
        modules::parse_document_with_inputs(source, filename, declared_sources)?;
    let (module, translations) = prepare(&document, filename, declared_assets, &BTreeMap::new())?;
    let rust = emit_with_inputs(
        &module,
        filename,
        declared_assets,
        translations,
        &mut inputs,
    )?;
    Ok((rust, inputs))
}

fn emit_with_inputs(
    module: &mlir::Module,
    filename: &str,
    declared_assets: Option<&[PathBuf]>,
    translations: ResolvedTranslations,
    inputs: &mut BTreeSet<PathBuf>,
) -> Result<TokenStream, CompilerError> {
    let assets = resolve_assets(module, Path::new(filename), declared_assets)
        .map_err(CompilerError::Resources)?;
    let rust = codegen::generate_with_translations(
        module,
        &assets.images,
        &assets.fonts,
        &assets.paths,
        translations.catalog.as_ref(),
    )
    .map_err(CompilerError::Codegen)?;
    inputs.insert(filename.into());
    inputs.extend(assets.inputs);
    inputs.extend(translations.inputs);
    Ok(rust)
}

pub fn check_document(
    document: &ast::Document,
    filename: &str,
    declared_assets: Option<&[PathBuf]>,
) -> Result<(), CompilerError> {
    let (module, _) = prepare(document, filename, declared_assets, &BTreeMap::new())?;
    check_output_resources(&module, filename, declared_assets)
}

pub fn check_resources(
    module: &mlir::Module,
    filename: &str,
    declared_assets: Option<&[PathBuf]>,
) -> Result<(), CompilerError> {
    resolve_translations(
        module,
        Path::new(filename),
        declared_assets,
        &BTreeMap::new(),
    )
    .map_err(CompilerError::Resources)?;
    check_output_resources(module, filename, declared_assets)
}

fn check_output_resources(
    module: &mlir::Module,
    filename: &str,
    declared_assets: Option<&[PathBuf]>,
) -> Result<(), CompilerError> {
    let assets = resolve_assets(module, Path::new(filename), declared_assets)
        .map_err(CompilerError::Resources)?;
    codegen::check_nine_slices(module, &assets.images).map_err(CompilerError::Resources)?;
    codegen::check_markdown(module).map_err(CompilerError::Codegen)
}

fn prepare(
    doc: &ast::Document,
    filename: &str,
    declared_assets: Option<&[PathBuf]>,
    buffers: &BTreeMap<PathBuf, String>,
) -> Result<(mlir::Module, ResolvedTranslations), CompilerError> {
    let module = lower_raw(doc, filename)?;
    // Validate every call before visual and expression passes remove unused code.
    let translations = resolve_translations(&module, Path::new(filename), declared_assets, buffers)
        .map_err(CompilerError::Resources)?;
    let module = module.optimize().map_err(CompilerError::Mlir)?;
    Ok((module, translations))
}

/// Validate the source and run the native MLIR pass pipeline.
pub fn lower(doc: &ast::Document, filename: &str) -> Result<mlir::Module, CompilerError> {
    lower_raw(doc, filename)?
        .optimize()
        .map_err(CompilerError::Mlir)
}

fn lower_raw(doc: &ast::Document, filename: &str) -> Result<mlir::Module, CompilerError> {
    mlir::lower_module(doc, filename).map_err(|diagnostics| CompilerError::Diagnostics {
        filename: filename.into(),
        diagnostics,
    })
}

pub fn format_diagnostics(filename: &str, diagnostics: &[ast::Diagnostic]) -> String {
    diagnostics
        .iter()
        .map(|d| format!("{filename}:{d}"))
        .collect::<Vec<_>>()
        .join("\n")
}
