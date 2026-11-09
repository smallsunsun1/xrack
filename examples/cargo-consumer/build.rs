use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo::rerun-if-env-changed=XRACKC");
    let compiler =
        PathBuf::from(env::var_os("XRACKC").expect("set XRACKC to the xrackc executable"));
    let compiler = compiler
        .canonicalize()
        .expect("locate the xrackc executable");
    println!("cargo::rerun-if-changed={}", compiler.display());

    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("ui.rs");
    let status = Command::new(&compiler)
        .arg("--cargo")
        .arg("ui/counter.xr")
        .arg("-o")
        .arg(output)
        .status()
        .expect("run xrackc");
    assert!(status.success(), "xrackc failed with {status}");
}
