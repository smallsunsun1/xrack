#!/usr/bin/env python3
"""Build and archive xrackc for the current host."""

import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile


ROOT = Path(__file__).resolve().parents[1]
BAZEL_OPTIONS = (
    "--repo_env=CARGO_BAZEL_ISOLATED=false",
    "--lockfile_mode=error",
)


def output(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def cargo_licenses(host):
    metadata = json.loads(subprocess.check_output(
        ("cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform", host),
        cwd=ROOT,
    ))
    packages = {package["id"]: package for package in metadata["packages"]}
    dependencies = {node["id"]: node["deps"] for node in metadata["resolve"]["nodes"]}
    pending = [key for key, package in packages.items() if package["name"] == "xrack-compiler"]
    seen = set()
    while pending:
        key = pending.pop()
        if key in seen:
            continue
        seen.add(key)
        pending.extend(
            dep["pkg"] for dep in dependencies[key]
            if any(kind["kind"] is None for kind in dep["dep_kinds"])
        )
        package = packages[key]
        if package["source"] is None:
            continue
        directory = Path(package["manifest_path"]).parent
        files = {
            file for file in directory.iterdir()
            if file.is_file() and file.name.lower().startswith(
                ("license", "licence", "copying", "notice", "unlicense")
            )
        }
        if package["license_file"]:
            files.add(directory / package["license_file"])
        name = f'{package["name"]}-{package["version"]}'
        # Some crates omit the license from their crates.io archive. Use the
        # original license from the exact source revision in .cargo_vcs_info.json.
        bundled_license = ROOT / "tools" / "licenses" / name / "LICENSE-MIT"
        if not files and bundled_license.is_file():
            files.add(bundled_license)
        if not files and name == "gif-dispose-6.0.0" and package["license"] == "MIT OR Apache-2.0":
            # This crate declares both licenses but ships neither text.
            # Include the declaration and the Apache-2.0 terms.
            files.update((directory / "Cargo.toml.orig", ROOT / "LICENSE-APACHE"))
        if not files:
            raise RuntimeError(f"no license files for {name}")
        for file in sorted(files):
            yield file, f"licenses/cargo/{name}/{file.name}"


def main():
    # Resolve the PATH entry explicitly so Windows can use a Bazelisk batch shim.
    bazel = shutil.which("bazel")
    if bazel is None:
        raise SystemExit("Bazel was not found on PATH")
    bazel = Path(bazel).absolute()
    host = next(line.removeprefix("host: ") for line in output("rustc", "-vV").splitlines()
                if line.startswith("host: "))
    subprocess.run((bazel, "build", *BAZEL_OPTIONS, "//:xrackc"), cwd=ROOT, check=True)
    bin_dir = Path(output(bazel, "info", *BAZEL_OPTIONS, "bazel-bin"))
    external = Path(output(bazel, "info", *BAZEL_OPTIONS, "output_base")) / "external"
    exe = "xrackc.exe" if "windows" in host else "xrackc"
    compiler = bin_dir / "crates" / "xrack-compiler" / exe
    version = output(compiler, "--version").split()[1]
    name = f"xrackc-{version}-{host}"
    dist = ROOT / ".tools" / "dist"
    dist.mkdir(parents=True, exist_ok=True)

    llvm = external / "llvm++llvm+llvm-project"
    rust_docs = Path(output("rustc", "--print", "sysroot")) / "share" / "doc" / "rust"
    files = [
        (compiler, exe),
        (ROOT / "LICENSE-APACHE", "LICENSE-APACHE"),
        (ROOT / "LICENSE-MIT", "LICENSE-MIT"),
        (llvm / "llvm" / "LICENSE.TXT", "licenses/llvm.txt"),
        (llvm / "mlir" / "LICENSE.TXT", "licenses/mlir.txt"),
        (external / "zlib-ng+" / "LICENSE.md", "licenses/zlib-ng.md"),
        (external / "zstd+" / "LICENSE", "licenses/zstd.txt"),
        (rust_docs / "COPYRIGHT-library.html", "licenses/rust-library.html"),
    ]
    files.extend((path, f"licenses/rust/{path.name}") for path in sorted((rust_docs / "licenses").iterdir()))
    files.extend(cargo_licenses(host))
    archive = dist / f"{name}.tar.gz"
    with tempfile.TemporaryDirectory(dir=dist) as staging:
        temporary_archive = Path(staging) / archive.name
        with tarfile.open(temporary_archive, "w:gz", dereference=True) as bundle:
            for source, relative in files:
                bundle.add(source, arcname=f"{name}/{relative}", recursive=False)
        temporary_archive.replace(archive)
    with archive.open("rb") as bundle:
        checksum = hashlib.file_digest(bundle, "sha256").hexdigest()
    archive.with_suffix(".gz.sha256").write_text(f"{checksum}  {archive.name}\n")
    print(archive)


if __name__ == "__main__":
    main()
