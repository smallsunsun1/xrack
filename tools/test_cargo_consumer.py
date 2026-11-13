#!/usr/bin/env python3
"""Run the standalone Cargo example using a staged host compiler package."""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib

import compiler_bundles as bundles
import mlir_bundles


def check_interpreter(app, compiler, env):
    (app / "src").mkdir(parents=True)
    (app / "Cargo.toml").write_text(f"""[package]
name = "xrack-mlir-consumer"
version = "0.0.0"
edition = "2024"

[dependencies]
xrack = {{ version = "={bundles.workspace_version()}", features = ["interpreter"] }}
""")
    (app / "src/main.rs").write_text("""fn main() {
    use xrack::interpreter::{HostValue, load_mlir_file};
    let program = load_mlir_file("counter.mlir").unwrap();
    let ui = program.create("Counter").unwrap();
    ui.set("count", HostValue::Int(21)).unwrap();
    assert_eq!(ui.get("doubled").unwrap(), HostValue::Int(42));
    assert_eq!(ui.try_render().unwrap().kind, "Window");
    println!("interpreted count=21, doubled=42");
}
""")
    source = app / "counter.xr"
    source.write_text("""export component Counter inherits Window {
    in-out property <int> count: 0;
    out property <int> doubled: count * 2;
    Text { text: "" + root.doubled; }
}
""")
    subprocess.run(
        (compiler, source, "--emit", "mlir", "-o", app / "counter.mlir"),
        cwd=app, env=env, check=True,
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archives", type=Path)
    parser.add_argument("host", choices=bundles.hosts())
    parser.add_argument("--interpreter", action="store_true")
    args = parser.parse_args()
    archives = args.archives.resolve()
    root = bundles.ROOT
    version = bundles.workspace_version()
    with (root / "rust-toolchain.toml").open("rb") as toolchain:
        channel = tomllib.load(toolchain)["toolchain"]["channel"]
    with tempfile.TemporaryDirectory(prefix="xrack-cargo-consumer-") as directory:
        stage = Path(directory)
        workspace = stage / "sdk"
        bundles.stage_workspace(workspace)
        app = stage / "app"
        env = {
            **os.environ,
            "RUSTUP_TOOLCHAIN": channel,
            "CARGO_TARGET_DIR": os.environ.get("CARGO_TARGET_DIR", str(stage / "build")),
        }
        for key in ("XRACKC", "XRACK_ANTLR_DIR", "XRACK_MLIR_BINDINGS", "OUT_DIR"):
            env.pop(key, None)
        if args.interpreter:
            archive, _ = bundles.verified_archive(archives, version, args.host)
            if "windows" in args.host:
                commit = subprocess.check_output(("git", "rev-parse", "HEAD"), cwd=root, text=True).strip()
                bundles.install_bundle(workspace, archives, version, args.host, commit)
            mlir_bundles.stage_bundle(workspace, archive, args.host)
            compiler = bundles.extract_compiler(archive, stage)
            check_interpreter(app, compiler, env)
            expected = "interpreted count=21, doubled=42"
        else:
            commit = subprocess.check_output(("git", "rev-parse", "HEAD"), cwd=root, text=True).strip()
            bundles.install_bundle(workspace, archives, version, args.host, commit)
            shutil.copytree(root / "examples/cargo-consumer", app)
            expected = "count=1, doubled=2"
        command = ["cargo", "run", "--release"]
        # Patch the standalone registry graph to the exact staged release sources.
        patches = {path.parent.name: path.parent for path in (workspace / "crates").glob("*/Cargo.toml")
                   if path.parent.name not in ("xrack-compiler", "xrack-codegen", "xrack-lsp", "xrack-preview")}
        if args.interpreter:
            patches.pop("xrack-build")
        for host in bundles.hosts():
            if not args.interpreter or "windows" in host:
                patches[bundles.crate_name(host)] = workspace / "bundles/compiler" / host
            patches[f"xrack-mlir-bin-{host}"] = workspace / "bundles/mlir" / host
        for name, directory in patches.items():
            path = directory.as_posix()
            command.extend(("--config", f"patch.crates-io.{name}.path={json.dumps(path)}"))
        if args.interpreter:
            names = [f"xrack-mlir-bin-{args.host}"]
            if "windows" in args.host:
                names.append(bundles.crate_name(args.host))
            packages = ["cargo", "package", "--locked", "--no-verify", "--allow-dirty"]
            for name in names:
                packages.extend(("-p", name))
            subprocess.run(packages, cwd=workspace, env=env, check=True)
            for name in names:
                artifact = Path(env["CARGO_TARGET_DIR"]) / "package" / f"{name}-{version}.crate"
                if artifact.stat().st_size > 10_000_000:
                    raise ValueError(f"{artifact.name} exceeds the crates.io package size limit")
        result = subprocess.run(command, cwd=app, env=env, check=True, text=True, stdout=subprocess.PIPE)
        print(result.stdout, end="")
        if result.stdout.strip() != expected:
            raise SystemExit("unexpected Cargo consumer output")


if __name__ == "__main__":
    main()
