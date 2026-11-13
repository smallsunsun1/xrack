//! XRack syntax frontend.
pub use xrack_syntax::ast;

mod antlr;
pub use antlr::{FormatToken, format_tokens, parse, same_tokens};

#[cfg(xrack_bazel)]
use xrack_antlr_rust as generated;

#[cfg(not(xrack_bazel))]
mod generated {
    #![allow(clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/antlr/lib.rs"));
}
