use std::{env, fs, io, path::PathBuf};

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-env-changed=XRACK_ANTLR_DIR");
    let source = env::var_os("XRACK_ANTLR_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("generated"));
    println!("cargo:rerun-if-changed={}", source.display());
    let destination =
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory")).join("antlr");
    fs::create_dir_all(&destination)?;
    for name in [
        "lib.rs",
        "xracklexer.rs",
        "xracklistener.rs",
        "xrackparser.rs",
    ] {
        fs::copy(source.join(name), destination.join(name)).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("{}: {error}", source.join(name).display()),
            )
        })?;
    }
    Ok(())
}
