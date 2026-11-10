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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archives", type=Path)
    parser.add_argument("host", choices=bundles.hosts())
    args = parser.parse_args()
    archives = args.archives.resolve()
    root = bundles.ROOT
    version = bundles.workspace_version()
    commit = subprocess.check_output(("git", "rev-parse", "HEAD"), cwd=root, text=True).strip()
    with (root / "rust-toolchain.toml").open("rb") as toolchain:
        channel = tomllib.load(toolchain)["toolchain"]["channel"]
    with tempfile.TemporaryDirectory(prefix="xrack-cargo-consumer-") as directory:
        stage = Path(directory)
        workspace = stage / "sdk"
        bundles.stage_workspace(workspace)
        bundles.install_bundle(workspace, archives, version, args.host, commit)
        app = stage / "app"
        shutil.copytree(root / "examples/cargo-consumer", app)
        env = {**os.environ, "RUSTUP_TOOLCHAIN": channel, "CARGO_TARGET_DIR": str(stage / "build")}
        env.pop("XRACKC", None)
        command = ["cargo", "run", "--release"]
        for name in ("xrack", "xrack-build"):
            path = (workspace / "crates" / name).as_posix()
            command.extend(("--config", f"patch.crates-io.{name}.path={json.dumps(path)}"))
        result = subprocess.run(command, cwd=app, env=env, check=True, text=True, stdout=subprocess.PIPE)
        print(result.stdout, end="")
        if result.stdout.strip() != "count=1, doubled=2":
            raise SystemExit("unexpected Cargo consumer output")


if __name__ == "__main__":
    main()
