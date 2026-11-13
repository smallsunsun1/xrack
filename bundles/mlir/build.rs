// Copied into each native package alongside the Bazel-generated artifacts.
use lzma_rust2::XzReader;
use std::{env, fs::File, io, path::PathBuf};

fn main() -> io::Result<()> {
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    let filenames = std::fs::read_to_string("native-libraries.txt")?;
    println!("cargo:rerun-if-changed=native-libraries.txt");
    println!("cargo:rustc-link-search=native={}", out.display());
    for filename in filenames.lines() {
        let compressed = format!("{filename}.xz");
        println!("cargo:rerun-if-changed={compressed}");
        io::copy(
            &mut XzReader::new(File::open(compressed)?, false),
            &mut File::create(out.join(filename))?,
        )?;
        let name = filename
            .strip_suffix(".a")
            .and_then(|name| name.strip_prefix("lib"))
            .or_else(|| filename.strip_suffix(".lib"))
            .expect("Bazel static library filename");
        println!("cargo:rustc-link-lib=static={name}");
    }

    println!("cargo:rerun-if-changed=system-libraries.txt");
    for library in std::fs::read_to_string("system-libraries.txt")?.lines() {
        println!("cargo:rustc-link-lib=dylib={library}");
    }
    Ok(())
}
