#!/usr/bin/env python3
"""Verify XRack's Cargo release or upload a release verified by the workflow."""

import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
from urllib.error import HTTPError
from urllib.request import urlopen

import compiler_bundles as bundles
import mlir_bundles

ROOT = bundles.ROOT
CRATES = (
    "xrack-runtime",
    "xrack-assets",
    "xrack-layout",
    "xrack-renderer-femtovg",
    "xrack-platform-winit",
    "xrack-syntax",
    "xrack-ir",
    "xrack-mlir",
    "xrack-resources",
    "xrack-interpreter",
    "xrack",
    "xrack-build",
)
BOOTSTRAP_CRATES = CRATES[:6]


def output(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def published_version(name, version):
    try:
        with urlopen(f"https://index.crates.io/xr/ac/{name}", timeout=30) as index:
            for line in index:
                entry = json.loads(line)
                if entry["vers"] == version:
                    return entry
    except HTTPError as error:
        if error.code != 404:
            raise
    return None


def published_metadata(name, version, checksum, member):
    package = f"{name}-{version}"
    with urlopen(f"https://static.crates.io/crates/{name}/{package}.crate", timeout=30) as response:
        archive = response.read()
    if hashlib.sha256(archive).hexdigest() != checksum:
        raise RuntimeError(f"{package}: downloaded archive differs from the registry checksum")
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as bundle:
        try:
            with bundle.extractfile(f"{package}/{member}") as record:
                return json.load(record)
        except KeyError:
            return None


def matches_commit(name, version, checksum, commit):
    metadata = published_metadata(name, version, checksum, ".cargo_vcs_info.json")
    recorded = (metadata or {}).get("git", {})
    return recorded.get("sha1") == commit and not recorded.get("dirty", False)


def pending_crates(version, commit):
    metadata = json.loads(output("cargo", "metadata", "--locked", "--format-version", "1", "--no-deps"))
    versions = {package["name"]: package["version"] for package in metadata["packages"]
                if package["publish"] != []}
    for name in CRATES:
        if versions.get(name) != version:
            raise RuntimeError(f"{name}: release version {version} does not match the manifest")
    pending = []
    for name in CRATES:
        published = published_version(name, version)
        if published is None:
            pending.append(name)
        elif published["yanked"] or not matches_commit(name, version, published["cksum"], commit):
            raise RuntimeError(f"{name} {version} already exists with a different release state")
        else:
            print(f"{name} {version} already published from {commit[:7]}", flush=True)
    return pending


def pending_bundles(workspace, archives, version, commit):
    pending = []

    def add(name, identity):
        published = published_version(name, version)
        if published is None:
            pending.append(name)
            return
        recorded = published_metadata(name, version, published["cksum"], "release.json") or {}
        if published["yanked"] or any(recorded.get(key) != value for key, value in identity.items()):
            raise RuntimeError(f"{name} {version} already exists with a different release state")
        print(f"{name} {version} already published from {commit[:7]}", flush=True)

    for host in bundles.hosts():
        provenance = bundles.install_bundle(workspace, archives, version, host, commit)
        add(bundles.crate_name(host), {
            key: provenance[key] for key in ("version", "host", "source_commit", "compiler_sha256")
        })
        archive, _ = bundles.verified_archive(archives, version, host)
        native = {
            "version": version, "host": host, "source_commit": commit,
            "artifacts": mlir_bundles.stage_bundle(workspace, archive, host),
        }
        (workspace / "bundles/mlir" / host / "release.json").write_text(
            json.dumps(native, sort_keys=True) + "\n"
        )
        add(f"xrack-mlir-bin-{host}", native)

    parser = workspace / "crates/xrack-parser"
    identity = {
        "version": version, "source_commit": commit,
        "generated": {
            file.name: hashlib.sha256(file.read_bytes()).hexdigest()
            for file in sorted((parser / "generated").iterdir())
        },
    }
    (parser / "release.json").write_text(json.dumps(identity, sort_keys=True) + "\n")
    add("xrack-parser", identity)
    return pending


def cargo(workspace, target_dir, mode, packages):
    if packages:
        command = ["cargo", *mode, "--registry", "crates-io", "--locked", "--target-dir", str(target_dir)]
        for name in packages:
            command.extend(("-p", name))
        subprocess.run(command, cwd=workspace, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version")
    parser.add_argument("--publish", action="store_true", help="upload after the workflow verification job")
    parser.add_argument("--archives", type=Path, help="directory with all Bazel compiler archives and checksums")
    args = parser.parse_args()
    if output("git", "status", "--porcelain"):
        parser.error("the release checkout must be clean")
    if args.publish and args.archives is None:
        parser.error("--publish requires --archives")
    commit = output("git", "rev-parse", "HEAD")
    pending = pending_crates(args.version, commit)
    # Cargo's temporary package registry caches by crate version and target directory.
    # A fresh directory keeps a previous dry-run of the same version out of this release.
    with tempfile.TemporaryDirectory(prefix="xrack-release-") as directory:
        stage = Path(directory)
        if args.archives is None:
            # Before the compiler build, verify the packages that have no host payload dependency.
            cargo(ROOT, stage / "build", ["publish", "--dry-run"], [
                name for name in pending if name in BOOTSTRAP_CRATES
            ])
            return
        workspace = stage / "workspace"
        bundles.stage_workspace(workspace)
        payloads = pending_bundles(workspace, args.archives.resolve(), args.version, commit)
        if not args.publish:
            # The platform matrix has already built and run the Cargo consumer.
            # Verify the normalized package graph before providing the registry token.
            packages = [*payloads, *pending]
            cargo(workspace, stage / "build", ["package", "--no-verify", "--allow-dirty"], packages)
            for name in packages:
                artifact = stage / "build" / "package" / f"{name}-{args.version}.crate"
                if artifact.stat().st_size > 10_000_000:
                    raise RuntimeError(f"{artifact.name} exceeds the crates.io package size limit")
            return
        if (payloads or pending) and not os.environ.get("CARGO_REGISTRY_TOKEN"):
            parser.error("configure CARGO_REGISTRY_TOKEN in the crates-io environment")
        cargo(ROOT, stage / "bootstrap", ["publish", "--no-verify"], [
            name for name in pending if name in BOOTSTRAP_CRATES
        ])
        cargo(workspace, stage / "bundles", ["publish", "--no-verify", "--allow-dirty"], payloads)
        # Source crates are packaged from the clean tag and resolve published payloads.
        cargo(ROOT, stage / "source", ["publish", "--no-verify"], [
            name for name in pending if name not in BOOTSTRAP_CRATES
        ])


if __name__ == "__main__":
    main()
