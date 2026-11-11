//! Run LLVM FileCheck over optimized XR dialect IR.
use runfiles::Runfiles;
use std::{
    env, fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};
use xrack_compiler::{mlir, parse};

#[test]
fn optimized_mlir_matches_filecheck_cases() {
    let runfiles = Runfiles::create().expect("Bazel runfiles");
    let checker = runfiles
        .rlocation(env::var("XRACK_FILECHECK").unwrap())
        .expect("LLVM FileCheck executable");
    let cases = env::var("XRACK_FILECHECK_CASES").unwrap();
    assert!(!cases.is_empty());
    for location in cases.split_whitespace() {
        let path = runfiles.rlocation(location).expect("FileCheck input");
        let source = fs::read_to_string(&path).unwrap();
        let module = if path.extension().and_then(|ext| ext.to_str()) == Some("xr") {
            let document = parse(&source).unwrap_or_else(|errors| panic!("{location}: {errors:?}"));
            mlir::lower_module(&document, path.to_str().unwrap())
                .unwrap_or_else(|errors| panic!("{location}: {errors:?}"))
        } else {
            mlir::Module::parse(&source).unwrap_or_else(|error| panic!("{location}: {error}"))
        };
        if source
            .lines()
            .any(|line| line.trim_start().starts_with("// RAW:"))
        {
            module
                .verify()
                .unwrap_or_else(|error| panic!("{location}: {error}"));
            check(&checker, &path, "RAW", &module.to_mlir());
        }
        let optimized = module
            .optimize()
            .unwrap_or_else(|error| panic!("{location}: {error}"));
        check(&checker, &path, "CHECK", &optimized.to_mlir());
    }
}

fn check(checker: &Path, path: &Path, prefix: &str, ir: &str) {
    let mut child = Command::new(checker)
        .arg(path)
        .arg(format!("--check-prefix={prefix}"))
        .stdin(Stdio::piped())
        .spawn()
        .expect("launch FileCheck");
    let written = child.stdin.take().unwrap().write_all(ir.as_bytes());
    let status = child.wait().unwrap();
    assert!(status.success(), "{} ({prefix}):\n{ir}", path.display());
    written.unwrap();
}
