#!/usr/bin/env python3
"""Package native XRack VSIX files and publish a complete Marketplace release."""

import argparse
import json
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import subprocess
import tarfile
import tempfile
import tomllib
import xml.etree.ElementTree as ET
import zipfile

from compiler_bundles import ROOT, archive_members, verified_archive, workspace_version
from package_xrackc import BAZEL_OPTIONS, cargo_licenses

EXTENSION = ROOT / "crates" / "xrack-lsp" / "vscode"
VSCE = "@vscode/vsce@4.1.0"
TARGETS = {
    "aarch64-apple-darwin": "darwin-arm64",
    "aarch64-unknown-linux-gnu": "linux-arm64",
    "x86_64-unknown-linux-gnu": "linux-x64",
    "aarch64-pc-windows-msvc": "win32-arm64",
    "x86_64-pc-windows-msvc": "win32-x64",
}
RUNTIME_CRATES = (
    "xrack-runtime",
    "xrack-assets",
    "xrack-layout",
    "xrack-renderer-femtovg",
)
EXTENSION_FILES = (
    "README.md",
    "package.json",
    "package-lock.json",
    "extension.js",
    "language-configuration.json",
    "preview.js",
    "preview-host.rs",
    "preview-process.js",
    "preview-queue.js",
)
SAMPLE = "export component PreviewProbe inherits Window { width: 120px; height: 80px; }"
SERVER_TARGETS = {
    "xrack-lsp": "crates/xrack-lsp",
    "xrack-preview": "crates/xrack-preview",
}


def tool(name):
    path = shutil.which(name)
    if path is None:
        raise ValueError(f"{name} is required on PATH")
    return Path(path).absolute()


def manifest():
    return json.loads((EXTENSION / "package.json").read_text(encoding="utf-8"))


def check_tag(tag, extension):
    version = extension["version"]
    if not re.fullmatch(r"(0|[1-9][0-9]*)(\.(0|[1-9][0-9]*)){2}", version):
        raise ValueError(f"expected a stable VS Code extension version, got {version!r}")
    if tag != f"vscode-v{version}":
        raise ValueError(f"expected vscode-v{version} for this extension, got {tag!r}")


def vsix_name(extension, target):
    return f'{extension["name"]}-{extension["version"]}-{target}.vsix'


def stage_preview_sdk(destination):
    destination.mkdir()
    members = [f"crates/{crate}" for crate in RUNTIME_CRATES]
    source = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    text = re.sub(r"(?m)^members\s*=\s*\[[^\]]*\]", f"members = {json.dumps(members)}", source, count=1)
    if tomllib.loads(text)["workspace"]["members"] != members:
        raise ValueError("could not set the preview SDK workspace members")
    (destination / "Cargo.toml").write_text(text, encoding="utf-8")
    shutil.copyfile(ROOT / "rust-toolchain.toml", destination / "rust-toolchain.toml")
    for member in members:
        crate = destination / member
        shutil.copytree(ROOT / member / "src", crate / "src")
        shutil.copyfile(ROOT / member / "Cargo.toml", crate / "Cargo.toml")
        for license_name in ("LICENSE-APACHE", "LICENSE-MIT"):
            shutil.copyfile(ROOT / license_name, crate / license_name)


def stage_servers(destination, archives, host, suffix):
    destination.mkdir()
    compiler = "xrackc" + suffix
    archive, _ = verified_archive(archives, workspace_version(), host)
    with tarfile.open(archive, "r:gz") as bundle:
        for relative, entry in archive_members(bundle, archive):
            if relative == PurePosixPath(compiler) or relative.parts[:1] == ("licenses",):
                output = destination.joinpath(*relative.parts)
                output.parent.mkdir(parents=True, exist_ok=True)
                with bundle.extractfile(entry) as source, output.open("xb") as target:
                    shutil.copyfileobj(source, target)
    executable = destination / compiler
    if not executable.is_file() or executable.stat().st_size == 0:
        raise ValueError(f"{archive}: missing {compiler}")
    executable.chmod(0o755)
    for source, relative in cargo_licenses(host, roots=("xrack-lsp", "xrack-preview")):
        output = destination / relative
        if not output.exists():
            output.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, output)

    bazel = tool("bazel")
    targets = [f"//{directory}:{name}" for name, directory in SERVER_TARGETS.items()]
    subprocess.run((bazel, "build", *BAZEL_OPTIONS, *targets), cwd=ROOT, check=True)
    bin_dir = subprocess.check_output((bazel, "info", *BAZEL_OPTIONS, "bazel-bin"), cwd=ROOT, text=True).strip()
    for name, directory in SERVER_TARGETS.items():
        server = destination / (name + suffix)
        shutil.copyfile(Path(bin_dir) / directory / server.name, server)
        server.chmod(0o755)


def verify_vsix(package, extension, target):
    with zipfile.ZipFile(package) as archive:
        if archive.testzip() is not None:
            raise ValueError(f"{package}: corrupt VSIX")
        metadata = ET.fromstring(archive.read("extension.vsixmanifest"))
        identity = metadata.find("{*}Metadata/{*}Identity")
        expected = {
            "Id": extension["name"],
            "Publisher": extension["publisher"],
            "Version": extension["version"],
            "TargetPlatform": target,
        }
        if identity is None or any(identity.get(key) != value for key, value in expected.items()):
            raise ValueError(f"{package}: VSIX identity does not match the release target")
        packed_manifest = json.loads(archive.read("extension/package.json"))
        if any(packed_manifest.get(key) != extension[key] for key in ("name", "publisher", "version")):
            raise ValueError(f"{package}: extension manifest does not match the release")
        suffix = ".exe" if target.startswith("win32-") else ""
        for name in ("xrackc", *SERVER_TARGETS):
            binary = archive.getinfo("extension/server/" + name + suffix)
            mode = binary.external_attr >> 16
            if binary.file_size == 0 or (not suffix and not (mode & stat.S_IXUSR)):
                raise ValueError(f"{package}: invalid executable {binary.filename}")
        for path in (
            "extension/" + extension["icon"],
            "extension/preview-host.rs",
            "extension/preview-sdk/Cargo.toml",
            *(f"extension/preview-sdk/crates/{crate}/src/lib.rs" for crate in RUNTIME_CRATES),
        ):
            if archive.getinfo(path).file_size == 0:
                raise ValueError(f"{package}: empty file {path}")


def lsp_message(method, **fields):
    body = json.dumps({"jsonrpc": "2.0", "method": method, **fields}).encode()
    return f"Content-Length: {len(body)}\r\n\r\n".encode() + body


def smoke_test(package, directory, suffix):
    with zipfile.ZipFile(package) as archive:
        compiler, lsp, preview = (
            Path(archive.extract(f"extension/server/{name}{suffix}", directory))
            for name in ("xrackc", *SERVER_TARGETS)
        )
    compiler.chmod(0o755)
    lsp.chmod(0o755)
    preview.chmod(0o755)
    compiled = json.loads(subprocess.run(
        (compiler, directory / "probe.xr", "--emit", "preview"),
        input=json.dumps({"text": SAMPLE}).encode(),
        stdout=subprocess.PIPE,
        check=True,
        timeout=60,
    ).stdout)
    if compiled["components"] != ["PreviewProbe"] or "__xrack_preview_component" not in compiled["rust"]:
        raise ValueError(f"{package}: preview compiler smoke test failed")
    request = {"type": "load", "filename": str(directory / "probe.xr"), "text": SAMPLE}
    frame = subprocess.run(
        (preview, "--check"), input=(json.dumps(request) + "\n").encode(),
        stdout=subprocess.PIPE, check=True, timeout=60,
    ).stdout
    size = int.from_bytes(frame[:4], "big")
    response = json.loads(frame[4:4 + size])
    if response["error"] or response["component"] != "PreviewProbe":
        raise ValueError(f"{package}: interpreter smoke test failed: {response}")
    messages = (
        lsp_message("initialize", id=1, params={})
        + lsp_message("shutdown", id=2)
        + lsp_message("exit")
    )
    output = subprocess.run(
        (lsp,), input=messages, stdout=subprocess.PIPE, check=True, timeout=30
    ).stdout
    header, _, payload = output.partition(b"\r\n\r\n")
    size = int(header.split(b":", 1)[1])
    capabilities = json.loads(payload[:size])["result"]["capabilities"]
    if not capabilities.get("definitionProvider") or not capabilities.get("semanticTokensProvider"):
        raise ValueError(f"{package}: language server smoke test failed")


def package_extension(host, archives, output, extension):
    npm = tool("npm")
    output.mkdir(parents=True, exist_ok=True)
    target = TARGETS[host]
    suffix = ".exe" if target.startswith("win32-") else ""
    with tempfile.TemporaryDirectory(prefix="xrack-vsix-", dir=output) as temporary:
        directory = Path(temporary)
        stage = directory / "extension"
        stage.mkdir()
        for name in EXTENSION_FILES:
            shutil.copyfile(EXTENSION / name, stage / name)
        for name in ("media", "syntaxes"):
            shutil.copytree(EXTENSION / name, stage / name)
        shutil.copyfile(ROOT / "LICENSE-MIT", stage / "LICENSE")
        shutil.copyfile(ROOT / "LICENSE-APACHE", stage / "LICENSE-APACHE")
        stage_preview_sdk(stage / "preview-sdk")
        stage_servers(stage / "server", archives, host, suffix)
        subprocess.run(
            (npm, "ci", "--omit=dev", "--ignore-scripts", "--no-audit", "--no-fund"),
            cwd=stage,
            check=True,
        )
        package = directory / vsix_name(extension, target)
        subprocess.run(
            (npm, "exec", "--yes", "--package", VSCE, "--", "vsce", "package", "--target", target, "--out", package),
            cwd=stage,
            check=True,
        )
        verify_vsix(package, extension, target)
        smoke_test(package, directory / "smoke", suffix)
        package.replace(output / package.name)
    print(output / package.name, flush=True)


def release_packages(directory, extension):
    expected = {vsix_name(extension, target): target for target in TARGETS.values()}
    found = {path.name for path in directory.glob("*.vsix")}
    if found != expected.keys():
        raise ValueError(f"VSIX set mismatch. Missing {sorted(expected.keys() - found)}; unexpected {sorted(found - expected.keys())}")
    for name, target in expected.items():
        verify_vsix(directory / name, extension, target)
    return [directory / name for name in expected]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    package = commands.add_parser("package")
    package.add_argument("--host", required=True, choices=TARGETS)
    package.add_argument("--archives", required=True, type=Path)
    package.add_argument("--output", required=True, type=Path)
    check = commands.add_parser("check-tag")
    check.add_argument("tag")
    publish = commands.add_parser("publish")
    publish.add_argument("tag")
    publish.add_argument("--packages", required=True, type=Path)
    publish.add_argument("--skip-duplicate", action="store_true")
    args = parser.parse_args()
    extension = manifest()
    if args.command == "package":
        package_extension(args.host, args.archives.resolve(), args.output.resolve(), extension)
    else:
        check_tag(args.tag, extension)
        if args.command == "publish":
            packages = release_packages(args.packages.resolve(), extension)
            retry = ("--skip-duplicate",) if args.skip_duplicate else ()
            subprocess.run(
                (tool("npm"), "exec", "--yes", "--package", VSCE, "--", "vsce", "publish",
                 "--oidc", *retry, "--packagePath", *packages),
                cwd=ROOT,
                check=True,
            )


if __name__ == "__main__":
    main()
