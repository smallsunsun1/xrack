#!/usr/bin/env python3
"""Stage the Bazel-patched FemtoVG sources for Cargo packaging."""

import argparse
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import tarfile
import tomllib
from urllib.request import urlopen

import compiler_bundles as bundles


PACKAGE = "xrack-femtovg"
RENDERER = "xrack-renderer-femtovg"
RUNTIME_PACKAGES = ("xrack-runtime", "xrack-assets", "xrack-layout", PACKAGE, RENDERER)


def patch_configuration() -> tuple[str, str, list[Path]]:
    module = (bundles.ROOT / "MODULE.bazel").read_text(encoding="utf-8")
    annotations = re.findall(
        r"^crate\.annotation\(\n(.*?)^\)", module, re.MULTILINE | re.DOTALL
    )
    femtovg = [block for block in annotations if 'crate = "femtovg",' in block]
    if len(femtovg) != 1:
        raise ValueError("Expected one FemtoVG annotation in MODULE.bazel")
    block = femtovg[0]
    version = re.search(r'\bversion = "([^"]+)"', block)
    self_cell = re.search(r"@crates__self_cell-([0-9.]+)//:self_cell", block)
    labels = re.findall(
        r'^\s*"//bazel:(femtovg_[a-z0-9_]+\.patch)",\s*$', block, re.MULTILINE
    )
    if (
        version is None
        or self_cell is None
        or not labels
        or 'patch_args = ["-p1"]' not in block
    ):
        raise ValueError("Unsupported FemtoVG annotation in MODULE.bazel")
    # rules_rust stores the crate annotation's patches in a BTreeSet.
    return (
        version[1],
        self_cell[1],
        [bundles.ROOT / "bazel" / name for name in sorted(labels)],
    )


def upstream(workspace: Path, archive: Path | None) -> tuple[str, str, bytes]:
    packages = tomllib.loads((workspace / "Cargo.lock").read_text(encoding="utf-8"))[
        "package"
    ]
    entries = [entry for entry in packages if entry["name"] == "femtovg"]
    if (
        len(entries) != 1
        or entries[0].get("source")
        != "registry+https://github.com/rust-lang/crates.io-index"
    ):
        raise ValueError(
            "Cargo.lock must contain exactly one crates.io FemtoVG version"
        )
    version = entries[0]["version"]
    checksum = entries[0]["checksum"]
    if archive is None:
        url = f"https://static.crates.io/crates/femtovg/femtovg-{version}.crate"
        with urlopen(url, timeout=30) as source:
            contents = source.read()
    else:
        contents = archive.read_bytes()
    if hashlib.sha256(contents).hexdigest() != checksum:
        raise ValueError("FemtoVG archive differs from the Cargo.lock checksum")
    return version, checksum, contents


def extract_sources(destination: Path, version: str, contents: bytes) -> None:
    prefix = f"femtovg-{version}"
    top_level = {"Cargo.toml", "build.rs", "README.md", "LICENSE-MIT", "LICENSE-APACHE"}
    with tarfile.open(fileobj=io.BytesIO(contents), mode="r:gz") as archive:
        for entry in archive:
            path = PurePosixPath(entry.name)
            if path.parts[:1] != (prefix,) or ".." in path.parts or "\\" in entry.name:
                raise ValueError(f"Invalid FemtoVG archive path {entry.name}")
            relative = path.relative_to(prefix)
            if not relative.parts or relative.parts[0] not in top_level | {"src"}:
                continue
            if entry.isdir():
                continue
            source = archive.extractfile(entry) if entry.isfile() else None
            if source is None:
                raise ValueError(f"Expected a regular file for {entry.name}")
            target = destination / relative.as_posix()
            target.parent.mkdir(parents=True, exist_ok=True)
            with source, target.open("xb") as output:
                shutil.copyfileobj(source, output)


def package_manifest(path: Path, version: str, self_cell: str) -> None:
    manifest = path.read_text(encoding="utf-8")
    sections = re.split(r"(?=^\[)", manifest, flags=re.MULTILINE)
    manifest = "".join(
        section
        for section in sections
        if not (
            section.startswith(
                (
                    "[[example]]",
                    "[[test]]",
                    "[[bench]]",
                    "[dev-dependencies.",
                    "[profile.",
                )
            )
            or section.startswith("[target.")
            and ".dev-dependencies." in section.splitlines()[0]
        )
    )
    manifest = manifest.replace('name = "femtovg"', f'name = "{PACKAGE}"', 1)
    manifest = re.sub(
        r'^version = "[^"]+"$',
        f'version = "{version}"',
        manifest,
        count=1,
        flags=re.MULTILINE,
    )
    manifest = re.sub(
        r"^exclude = \[.*?^\]\n", "", manifest, flags=re.MULTILINE | re.DOTALL
    )
    manifest = manifest.replace(
        "[package]\n",
        "[package]\n"
        + (
            'include = ["src/**", "build.rs", "README.md", "LICENSE-*", "release.json"]\n'
        ),
        1,
    )
    manifest += f'\n[dependencies.self_cell]\nversion = "={self_cell}"\n'
    parsed = tomllib.loads(manifest)
    if parsed["package"]["name"] != PACKAGE or parsed["package"]["version"] != version:
        raise ValueError("Unexpected FemtoVG manifest layout")
    path.write_text(manifest, encoding="utf-8")


def stage_renderer(
    workspace: Path, archive: Path | None = None, *, source_commit: str | None = None
) -> Path:
    manifest_path = workspace / "Cargo.toml"
    manifest = manifest_path.read_text(encoding="utf-8")
    workspace_spec = tomllib.loads(manifest)["workspace"]
    version: str = workspace_spec["package"]["version"]
    upstream_version, checksum, contents = upstream(workspace, archive)
    patched_version, self_cell, patch_files = patch_configuration()
    if upstream_version != patched_version:
        raise ValueError("Bazel and Cargo use different upstream FemtoVG versions")
    package = workspace / "bundles" / "femtovg"
    package.mkdir()
    extract_sources(package, upstream_version, contents)
    subprocess.run(
        ["git", "apply", "--whitespace=nowarn", *map(str, patch_files)],
        cwd=package,
        check=True,
    )
    package_manifest(package / "Cargo.toml", version, self_cell)
    release = {
        "version": version,
        "upstream_version": upstream_version,
        "upstream_sha256": checksum,
        "patches": {
            path.name: hashlib.sha256(path.read_bytes()).hexdigest()
            for path in patch_files
        },
    }
    if source_commit is not None:
        release["source_commit"] = source_commit
    release_json = json.dumps(release, sort_keys=True) + "\n"
    (package / "release.json").write_text(release_json, encoding="utf-8")

    dependency = (
        f'femtovg = {{ path = "bundles/femtovg", package = "{PACKAGE}", version = "={version}", '
        'default-features = false, features = ["wgpu", "textlayout"] }'
    )
    manifest, replacements = re.subn(
        r"^femtovg = \{[^\n]+\}$", dependency, manifest, flags=re.MULTILINE
    )
    if replacements != 1:
        raise ValueError("Expected one FemtoVG workspace dependency")
    manifest_path.write_text(manifest, encoding="utf-8")
    renderer = workspace / "crates" / RENDERER
    with (renderer / "build.rs").open("x", encoding="utf-8") as script:
        script.write(
            'fn main() {\n    println!("cargo::rustc-cfg=xrack_femtovg_glyph_samples");\n}\n'
        )
    if source_commit is not None:
        with (renderer / "release.json").open("x", encoding="utf-8") as record:
            record.write(release_json)
    return package


def update_lock(
    workspace: Path, *, offline: bool = False, env: dict[str, str] | None = None
) -> None:
    command = ["cargo", "update", "--workspace"]
    if offline:
        command.append("--offline")
    subprocess.run(command, cwd=workspace, env=env, check=True)


def benchmark_renderer(workspace: Path, *, offline: bool) -> None:
    """Run the Bazel transcript fixture against the staged Cargo packages."""
    benchmark = workspace / ".tools" / "transcript-benchmark"
    benchmark.mkdir(parents=True)
    for source in ("transcript_layout_benchmark.rs", "gpu_readback.rs"):
        shutil.copyfile(bundles.ROOT / "tests" / source, benchmark / source)
    # Resolve the standalone test from the same locked dependencies as the SDK.
    shutil.copyfile(workspace / "Cargo.lock", benchmark / "Cargo.lock")

    dependencies = tomllib.loads(
        (workspace / "Cargo.toml").read_text(encoding="utf-8")
    )["workspace"]["dependencies"]
    wgpu = dependencies["wgpu"]
    wgpu_declaration = (
        f"version = {json.dumps(wgpu['version'])}, "
        f"default-features = {json.dumps(wgpu['default-features'])}, "
        f"features = {json.dumps(wgpu['features'])}"
    )
    (benchmark / "Cargo.toml").write_text(
        f"""[workspace]

[package]
name = "xrack-transcript-benchmark"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
xrack-layout = {{ path = "../../crates/xrack-layout" }}
xrack-renderer-femtovg = {{ path = "../../crates/xrack-renderer-femtovg" }}
xrack-runtime = {{ path = "../../crates/xrack-runtime" }}
pollster = {json.dumps(dependencies["pollster"])}
wgpu = {{ {wgpu_declaration} }}

[lib]
name = "gpu_readback"
path = "gpu_readback.rs"

[[test]]
name = "transcript_layout_benchmark"
path = "transcript_layout_benchmark.rs"
""",
        encoding="utf-8",
    )
    env = dict(os.environ)
    env.setdefault("CARGO_TARGET_DIR", str(workspace / ".tools" / "cargo-build"))
    command = ["cargo", "test", "--release", "--test", "transcript_layout_benchmark"]
    if offline:
        command.append("--offline")
    command.extend(["--", "--nocapture"])
    print(f"Running the transcript benchmark with staged {PACKAGE}", flush=True)
    subprocess.run(command, cwd=benchmark, env=env, check=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "workspace", type=Path, help="new directory for the staged workspace"
    )
    parser.add_argument(
        "--upstream-archive", type=Path, help="local copy of the locked upstream crate"
    )
    parser.add_argument(
        "--offline",
        action="store_true",
        help="resolve the staged Cargo.lock from cache",
    )
    parser.add_argument(
        "--package", action="store_true", help="build the renderer dependency archives"
    )
    parser.add_argument(
        "--benchmark",
        action="store_true",
        help="run the GPU transcript benchmark with the staged renderer",
    )
    args = parser.parse_args()
    workspace: Path = args.workspace.resolve()
    bundles.stage_workspace(workspace)
    stage_renderer(workspace, args.upstream_archive)
    update_lock(workspace, offline=args.offline)
    if args.package:
        command = [
            "cargo",
            "package",
            "--locked",
            "--no-verify",
            "--allow-dirty",
            "--target-dir",
            str(workspace / ".tools" / "cargo-build"),
        ]
        if args.offline:
            command.append("--offline")
        for name in RUNTIME_PACKAGES:
            command.extend(("-p", name))
        subprocess.run(command, cwd=workspace, check=True)
    if args.benchmark:
        benchmark_renderer(workspace, offline=args.offline)


if __name__ == "__main__":
    main()
