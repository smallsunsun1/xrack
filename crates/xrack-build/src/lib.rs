//! Compile an XRack UI from a Cargo build script.
use std::{env, io, path::Path, path::PathBuf, process::Command};

#[cfg(feature = "bundled")]
mod bundled;

/// Compile an entry point into the file consumed by `xrack::include_modules!()`.
///
/// The released crate uses its host compiler package by default. Set `XRACKC`
/// to the path of an alternative compiler when working on XRack itself.
pub fn compile(input: impl AsRef<Path>) -> io::Result<()> {
    println!("cargo::rerun-if-env-changed=XRACKC");
    let out = env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("xrack_build::compile must run in a Cargo build script"))?;
    let compiler = if let Some(path) = env::var_os("XRACKC") {
        let path = PathBuf::from(path).canonicalize()?;
        println!("cargo::rerun-if-changed={}", path.display());
        path
    } else {
        bundled_compiler(&out)?
    };
    let status = Command::new(compiler)
        .args(["--cargo", "--cargo-facade"])
        .arg(input.as_ref())
        .arg("-o")
        .arg(out.join("xrack_ui.rs"))
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "xrackc failed to compile {} ({status})",
            input.as_ref().display()
        )))
    }
}

#[cfg(feature = "bundled")]
fn bundled_compiler(out: &Path) -> io::Result<PathBuf> {
    bundled::materialize(out)
}

#[cfg(not(feature = "bundled"))]
fn bundled_compiler(_: &Path) -> io::Result<PathBuf> {
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "this build of xrack-build has no bundled compiler; set XRACKC",
    ))
}
