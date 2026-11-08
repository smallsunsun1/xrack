//! XRack syntax frontend.
pub use xrack_syntax::ast;

mod antlr;
pub use antlr::parse;

#[cfg(xrack_bazel)]
use xrack_antlr_rust as generated;

// Cargo consumes the same generated sources as Bazel.
#[cfg(not(xrack_bazel))]
mod generated {
    #![allow(clippy::all)]
    include!(concat!(env!("XRACK_ANTLR_DIR"), "/lib.rs"));
}
