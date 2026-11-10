//! Compiler orchestration: ANTLR4 → AST → XR dialect → Rust.
use proc_macro2::TokenStream;
use std::{collections::BTreeSet, path::PathBuf};

mod animated_image;
mod error;
mod modules;
mod resources;
pub use error::CompilerError;
pub use modules::parse_document;

pub use xrack_codegen as codegen;
pub use xrack_ir as ir;
pub use xrack_ir::semantic;
pub use xrack_mlir as mlir;
pub use xrack_parser::{ast, parse};

pub struct Compilation {
    pub rust: TokenStream,
    pub mlir: String,
}

pub fn compile(source: &str, filename: &str) -> Result<Compilation, CompilerError> {
    let doc = parse_document(source, filename, None)?;
    let processed = lower(&doc, filename)?;
    Ok(Compilation {
        rust: generate_rust(&processed, filename, None)?,
        mlir: processed.to_mlir(),
    })
}

pub fn generate_rust(
    module: &mlir::Module,
    filename: &str,
    declared_assets: Option<&[PathBuf]>,
) -> Result<TokenStream, CompilerError> {
    let assets =
        resources::resolve(module, filename, declared_assets).map_err(CompilerError::Resources)?;
    emit_rust(module, &assets)
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
    let module = lower(&document, filename)?;
    let assets =
        resources::resolve(&module, filename, declared_assets).map_err(CompilerError::Resources)?;
    let rust = emit_rust(&module, &assets)?;
    inputs.insert(filename.into());
    inputs.extend(assets.inputs);
    Ok((rust, inputs))
}

fn emit_rust(
    module: &mlir::Module,
    assets: &resources::Resolved,
) -> Result<TokenStream, CompilerError> {
    codegen::generate_with_resource_plans(module, &assets.images, &assets.fonts, &assets.paths)
        .map_err(CompilerError::Codegen)
}

pub fn check_resources(
    module: &mlir::Module,
    filename: &str,
    declared_assets: Option<&[PathBuf]>,
) -> Result<(), CompilerError> {
    let assets =
        resources::resolve(module, filename, declared_assets).map_err(CompilerError::Resources)?;
    codegen::check_nine_slices(module, &assets.images).map_err(CompilerError::Resources)?;
    codegen::check_markdown(module).map_err(CompilerError::Codegen)
}

/// Validate the source and run the native MLIR pass pipeline.
pub fn lower(doc: &ast::Document, filename: &str) -> Result<mlir::Module, CompilerError> {
    mlir::lower_module(doc, filename)
        .map_err(|diagnostics| CompilerError::Diagnostics {
            filename: filename.into(),
            diagnostics,
        })?
        .optimize()
        .map_err(CompilerError::Mlir)
}

pub fn format_diagnostics(filename: &str, diagnostics: &[ast::Diagnostic]) -> String {
    diagnostics
        .iter()
        .map(|d| format!("{filename}:{d}"))
        .collect::<Vec<_>>()
        .join("\n")
}
