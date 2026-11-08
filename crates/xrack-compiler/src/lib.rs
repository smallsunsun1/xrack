//! Compiler orchestration: ANTLR4 C++ → AST → XR dialect → Rust.
use proc_macro2::TokenStream;
use std::path::PathBuf;

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
    let images =
        resources::resolve(module, filename, declared_assets).map_err(CompilerError::Resources)?;
    codegen::generate_with_images(module, &images).map_err(CompilerError::Codegen)
}

pub fn check_resources(
    module: &mlir::Module,
    filename: &str,
    declared_assets: Option<&[PathBuf]>,
) -> Result<(), CompilerError> {
    resources::resolve(module, filename, declared_assets)
        .map(|_| ())
        .map_err(CompilerError::Resources)
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
