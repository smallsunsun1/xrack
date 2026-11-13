use crate::{ast, format_diagnostics};
use std::{io, path::PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CompilerError {
    #[error("{}", format_diagnostics(.filename, .diagnostics))]
    Diagnostics {
        filename: String,
        diagnostics: Vec<ast::Diagnostic>,
    },
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{0}")]
    Resources(String),
    #[error("{0}")]
    Mlir(String),
    #[error("Rust code generation failed: {0}")]
    Codegen(String),
    #[error("could not serialize the compiler output: {0}")]
    Json(#[from] serde_json::Error),
}

impl From<xrack_ir::modules::ModuleError> for CompilerError {
    fn from(error: xrack_ir::modules::ModuleError) -> Self {
        use xrack_ir::modules::ModuleError;
        match error {
            ModuleError::Diagnostics {
                filename,
                diagnostics,
                ..
            } => Self::Diagnostics {
                filename,
                diagnostics,
            },
            ModuleError::Io { path, source } => Self::Io { path, source },
        }
    }
}
