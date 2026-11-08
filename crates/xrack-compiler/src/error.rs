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

impl CompilerError {
    pub(crate) fn at(file: &std::path::Path, span: ast::Span, message: impl Into<String>) -> Self {
        Self::Diagnostics {
            filename: file.to_string_lossy().into_owned(),
            diagnostics: vec![ast::Diagnostic {
                span,
                message: message.into(),
            }],
        }
    }
}
