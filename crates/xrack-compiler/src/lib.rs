//! Compiler orchestration: ANTLR4 → AST → XR dialect → Rust.
use proc_macro2::TokenStream;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

mod error;
mod modules;
mod package;
pub use error::CompilerError;
pub use modules::parse_document;
pub use package::pack_module;

pub use xrack_codegen as codegen;
pub use xrack_ir as ir;
pub use xrack_ir::semantic;
pub use xrack_mlir as mlir;
pub use xrack_parser::{ast, parse};
use xrack_resources::{
    PrepareMlirError, ResolvedTranslations, prepare_mlir, prepare_module, resolve_assets,
    resolve_translations,
};

fn prepare_error(error: PrepareMlirError) -> CompilerError {
    match error {
        PrepareMlirError::Mlir(message) => CompilerError::Mlir(message),
        PrepareMlirError::Resources(message) => CompilerError::Resources(message),
    }
}

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
    module.verify().map_err(CompilerError::Mlir)?;
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
        .map(|(path, text)| (ir::modules::normalize_source_path(&path), text))
        .collect();
    let (doc, mut inputs) =
        modules::parse_document_with_overrides(source, filename, None, &buffers)?;
    let mut components: Vec<_> = doc
        .components
        .iter()
        .filter(|component| {
            !component.global
                && component
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
        prepare_mlir(source, Path::new(filename)).map_err(prepare_error)?;
    let components = module
        .operations()
        .filter(|op| {
            op.name() == "xr.component"
                && op.attribute("exported") == Some("true")
                && op.attribute("global") != Some("true")
        })
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
    module.verify().map_err(CompilerError::Mlir)?;
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

pub fn compile_live_with_inputs(
    source: &str,
    filename: &str,
    declared_sources: Option<&[PathBuf]>,
    declared_assets: Option<&[PathBuf]>,
) -> Result<(TokenStream, BTreeSet<PathBuf>), CompilerError> {
    let (document, mut inputs) =
        modules::parse_document_with_inputs(source, filename, declared_sources)?;
    let (module, translations) = prepare(&document, filename, declared_assets, &BTreeMap::new())?;
    inputs.extend(translations.inputs);
    inputs.extend(check_output_resources(&module, filename, declared_assets)?);
    let rust = codegen::generate_live(&module, filename, true).map_err(CompilerError::Codegen)?;
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
    let rust = codegen::generate_with_resolved_assets_at(
        module,
        &assets,
        translations.catalog.as_ref(),
        filename,
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
    checked_module(document, filename, declared_assets).map(|_| ())
}

/// Export optimized XR dialect text after checking the resources used at runtime.
pub fn export_mlir(
    document: &ast::Document,
    filename: &str,
    declared_assets: Option<&[PathBuf]>,
) -> Result<String, CompilerError> {
    checked_module(document, filename, declared_assets).map(|module| module.to_mlir())
}

/// Export optimized MLIR with file resources relative to its output location.
pub fn export_mlir_to(
    document: &ast::Document,
    filename: &str,
    declared_assets: Option<&[PathBuf]>,
    output: &Path,
) -> Result<String, CompilerError> {
    fn directory(file: &Path) -> Result<PathBuf, CompilerError> {
        let parent = file
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        parent.canonicalize().map_err(|source| CompilerError::Io {
            path: parent.into(),
            source,
        })
    }

    fn relative_path(from: &Path, to: &Path) -> Option<PathBuf> {
        from.ancestors()
            .take_while(|ancestor| ancestor.has_root())
            .enumerate()
            .find_map(|(levels, ancestor)| {
                let suffix = to.strip_prefix(ancestor).ok()?;
                let mut relative: PathBuf = std::iter::repeat_n("..", levels).collect();
                relative.push(suffix);
                Some(relative)
            })
    }

    fn resource_path(path: &Path) -> Result<String, String> {
        let text = path.to_str().ok_or_else(|| {
            format!(
                "resource path {} cannot be written as UTF-8",
                path.display()
            )
        })?;
        #[cfg(windows)]
        if path.is_relative() {
            return Ok(text.replace('\\', "/"));
        }
        Ok(text.to_owned())
    }

    let module = checked_module(document, filename, declared_assets)?;
    let input_dir = directory(Path::new(filename))?;
    let output_dir = directory(output)?;
    let module = if input_dir == output_dir {
        module
    } else {
        let prefix = relative_path(&output_dir, &input_dir);
        module
            .map_file_resource_paths(|path| {
                let prefix = prefix.as_ref().ok_or_else(|| {
                    format!(
                        "cannot reference resource {path:?} from {}",
                        output.display()
                    )
                })?;
                resource_path(&prefix.join(path))
            })
            .map_err(CompilerError::Resources)?
    };
    let module = module
        .map_media_source_files(|op| {
            let source = Path::new(xrack_resources::visual::media_declaration_file(
                op, filename,
            ));
            let name = source.file_name().ok_or_else(|| {
                format!("media declaration {} has no file name", source.display())
            })?;
            let path = directory(source)
                .map(|parent| parent.join(name))
                .or_else(|_| {
                    std::path::absolute(source).map_err(|source_error| CompilerError::Io {
                        path: source.into(),
                        source: source_error,
                    })
                })
                .map_err(|error| error.to_string())?;
            resource_path(&relative_path(&output_dir, &path).unwrap_or(path))
        })
        .map_err(CompilerError::Resources)?;
    Ok(module.to_mlir())
}

pub fn generate_live_document(
    document: &ast::Document,
    filename: &str,
    declared_assets: Option<&[PathBuf]>,
) -> Result<TokenStream, CompilerError> {
    let module = checked_module(document, filename, declared_assets)?;
    codegen::generate_live(&module, filename, false).map_err(CompilerError::Codegen)
}

fn checked_module(
    document: &ast::Document,
    filename: &str,
    declared_assets: Option<&[PathBuf]>,
) -> Result<mlir::Module, CompilerError> {
    let (module, _) = prepare(document, filename, declared_assets, &BTreeMap::new())?;
    check_output_resources(&module, filename, declared_assets)?;
    Ok(module)
}

pub fn check_resources(
    module: &mlir::Module,
    filename: &str,
    declared_assets: Option<&[PathBuf]>,
) -> Result<(), CompilerError> {
    module.verify().map_err(CompilerError::Mlir)?;
    resolve_translations(
        module,
        Path::new(filename),
        declared_assets,
        &BTreeMap::new(),
    )
    .map_err(CompilerError::Resources)?;
    check_output_resources(module, filename, declared_assets).map(|_| ())
}

fn check_output_resources(
    module: &mlir::Module,
    filename: &str,
    declared_assets: Option<&[PathBuf]>,
) -> Result<BTreeSet<PathBuf>, CompilerError> {
    let assets = resolve_assets(module, Path::new(filename), declared_assets)
        .map_err(CompilerError::Resources)?;
    codegen::check_nine_slices(module, &assets.images).map_err(CompilerError::Resources)?;
    codegen::check_markdown(module).map_err(CompilerError::Codegen)?;
    Ok(assets.inputs)
}

fn prepare(
    doc: &ast::Document,
    filename: &str,
    declared_assets: Option<&[PathBuf]>,
    buffers: &BTreeMap<PathBuf, String>,
) -> Result<(mlir::Module, ResolvedTranslations), CompilerError> {
    prepare_module(
        lower_raw(doc, filename)?,
        Path::new(filename),
        declared_assets,
        buffers,
    )
    .map_err(prepare_error)
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
