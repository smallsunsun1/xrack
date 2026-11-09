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
from urllib.error import HTTPError
from urllib.request import urlopen


ROOT = Path(__file__).resolve().parents[1]
CRATES = (
    "xrack-runtime",
    "xrack-assets",
    "xrack-layout",
    "xrack-renderer-femtovg",
    "xrack-platform-winit",
)


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


def matches_commit(name, version, checksum, commit):
    package = f"{name}-{version}"
    with urlopen(f"https://static.crates.io/crates/{name}/{package}.crate", timeout=30) as response:
        archive = response.read()
    if hashlib.sha256(archive).hexdigest() != checksum:
        raise RuntimeError(f"{package}: downloaded archive differs from the registry checksum")
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as bundle:
        try:
            with bundle.extractfile(f"{package}/.cargo_vcs_info.json") as vcs:
                recorded = json.load(vcs).get("git", {})
        except KeyError:
            return False
    return recorded.get("sha1") == commit and not recorded.get("dirty", False)


def pending_crates(version):
    metadata = json.loads(output("cargo", "metadata", "--locked", "--format-version", "1", "--no-deps"))
    versions = {package["name"]: package["version"] for package in metadata["packages"]
                if package["publish"] != []}
    for name in CRATES:
        if versions.get(name) != version:
            raise RuntimeError(f"{name}: release version {version} does not match the manifest")
    commit = output("git", "rev-parse", "HEAD")
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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version")
    parser.add_argument("--publish", action="store_true", help="upload after the workflow verification job")
    args = parser.parse_args()
    if output("git", "status", "--porcelain"):
        parser.error("the release checkout must be clean")
    pending = pending_crates(args.version)
    if not pending:
        return
    if args.publish and not os.environ.get("CARGO_REGISTRY_TOKEN"):
        parser.error("configure CARGO_REGISTRY_TOKEN in the crates-io environment")
    # Build verification runs in the dry-run job before the upload job receives the token.
    mode = "--no-verify" if args.publish else "--dry-run"
    command = ["cargo", "publish", "--registry", "crates-io", "--locked", mode]
    for name in pending:
        command.extend(("-p", name))
    subprocess.run(command, cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
