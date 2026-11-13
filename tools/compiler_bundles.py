"""Prepare the host Cargo payloads from Bazel release artifacts."""

import gzip
import hashlib
import json
from pathlib import Path, PurePosixPath
import shutil
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def hosts():
    return sorted(path.parent.name for path in (ROOT / "bundles/compiler").glob("*/Cargo.toml"))


def crate_name(host):
    return f"xrackc-bin-{host}"


def workspace_version():
    with (ROOT / "Cargo.toml").open("rb") as manifest:
        return tomllib.load(manifest)["workspace"]["package"]["version"]


def stage_workspace(destination):
    destination.mkdir()
    for name in ("Cargo.toml", "Cargo.lock", "README.md", "LICENSE-APACHE", "LICENSE-MIT"):
        shutil.copyfile(ROOT / name, destination / name)
    for name in ("crates", "bundles"):
        shutil.copytree(ROOT / name, destination / name)


def verified_archive(archives, version, host):
    stem = f"xrackc-{version}-{host}"
    archive = archives / f"{stem}.tar.gz"
    checksum = archive.with_suffix(".gz.sha256").read_text().split()
    with archive.open("rb") as bundle:
        digest = hashlib.file_digest(bundle, "sha256").hexdigest()
    if checksum != [digest, archive.name]:
        raise ValueError(f"{archive}: invalid release checksum")
    return archive, digest


def archive_members(bundle, archive):
    stem = archive.name.removesuffix(".tar.gz")
    for entry in bundle:
        path = PurePosixPath(entry.name)
        if (
            path.parts[:1] != (stem,)
            or len(path.parts) < 2
            or ".." in path.parts
            or "\\" in entry.name
            or ":" in entry.name
            or not entry.isfile()
        ):
            raise ValueError(f"{archive}: invalid member {entry.name}")
        yield path.relative_to(stem), entry


def extract_compiler(archive, destination):
    executable = "xrackc.exe" if "windows" in archive.name else "xrackc"
    with tarfile.open(archive, "r:gz") as bundle:
        for relative, entry in archive_members(bundle, archive):
            if relative == PurePosixPath(executable):
                output = destination / executable
                with bundle.extractfile(entry) as source, output.open("xb") as target:
                    shutil.copyfileobj(source, target)
                output.chmod(0o755)
                return output
    raise ValueError(f"{archive}: missing {executable}")


def install_bundle(workspace, archives, version, host, source_commit):
    archive, digest = verified_archive(archives, version, host)
    package = workspace / "bundles/compiler" / host
    executable = "xrackc.exe" if "windows" in host else "xrackc"
    compiler_digest = hashlib.sha256()
    found = False
    with tarfile.open(archive, "r:gz") as bundle:
        for relative, entry in archive_members(bundle, archive):
            if relative == PurePosixPath(executable):
                if found or entry.size == 0:
                    raise ValueError(f"{archive}: invalid compiler entry")
                with bundle.extractfile(entry) as source, (package / "xrackc.gz").open("wb") as output:
                    with gzip.GzipFile(filename="", mode="wb", fileobj=output, mtime=0) as compressed:
                        for chunk in iter(lambda: source.read(1024 * 1024), b""):
                            compiler_digest.update(chunk)
                            compressed.write(chunk)
                found = True
            elif relative.parts[:1] == ("licenses",) or relative.name in ("LICENSE-APACHE", "LICENSE-MIT"):
                destination = package.joinpath(*relative.parts)
                destination.parent.mkdir(parents=True, exist_ok=True)
                with bundle.extractfile(entry) as source, destination.open("wb") as output:
                    shutil.copyfileobj(source, output)
    if not found:
        raise ValueError(f"{archive}: missing {executable}")

    manifest = package / "Cargo.toml"
    text = manifest.read_text()
    if 'publish = false' not in text or 'path = "../stub.rs"' not in text:
        raise ValueError(f"{manifest}: expected a compiler placeholder")
    manifest.write_text(text.replace("publish = false\n", "").replace('../stub.rs', 'src/lib.rs'))
    (package / "src").mkdir()
    (package / "src/lib.rs").write_text(
        'pub const COMPILER: &[u8] = include_bytes!("../xrackc.gz");\n'
    )
    provenance = {
        "version": version,
        "host": host,
        "source_commit": source_commit,
        "archive_sha256": digest,
        "compiler_sha256": compiler_digest.hexdigest(),
    }
    (package / "release.json").write_text(json.dumps(provenance, sort_keys=True) + "\n")
    return provenance
