"""Stage the replaceable Bazel media library and its corresponding sources."""

import hashlib
import lzma
from pathlib import Path
import shutil
import tarfile

import compiler_bundles as bundles

ROOT = bundles.ROOT
TARGETS = ("//crates/xrack-media:native_payload", "//crates/xrack-media:ffmpeg_bindings")
BINDINGS = "ffmpeg_bindings.rs"


def library_name(host):
    if "windows" in host:
        return "xrack_media_ffmpeg.dll"
    if "darwin" in host:
        return "libxrack_media_ffmpeg.dylib"
    return "libxrack_media_ffmpeg.so"


def native_inputs(bin_dir, external, host, directory):
    native = bin_dir / "crates/xrack-media"
    library = native / library_name(host)
    payload = directory / "media-library.xz"
    with library.open("rb") as source, lzma.open(payload, "wb", preset=6) as target:
        shutil.copyfileobj(source, target)
    return [
        (payload, "media/library.xz"),
        (native / BINDINGS, "media/" + BINDINGS),
        (external / "ffmpeg+" / "COPYING.LGPLv2.1", "licenses/media/FFmpeg-LGPL-2.1.txt"),
        (external / "+http_archive+miniaudio" / "LICENSE", "licenses/media/miniaudio.txt"),
        (external / "zlib+" / "LICENSE", "licenses/media/zlib.txt"),
        (external / "bzip2+" / "LICENSE", "licenses/media/bzip2.txt"),
        (external / "openssl+" / "LICENSE.txt", "licenses/media/OpenSSL.txt"),
    ]


def stage_bundle(workspace, archive, host):
    package = workspace / "bundles/media" / host
    destination = package
    digests = {}
    with tarfile.open(archive, "r:gz") as bundle:
        for relative, entry in bundles.archive_members(bundle, archive):
            if relative.as_posix() in ("media/library.xz", f"media/{BINDINGS}"):
                artifact = destination / relative.name
                with bundle.extractfile(entry) as source, artifact.open("xb") as target:
                    shutil.copyfileobj(source, target)
                with artifact.open("rb") as source:
                    digests[relative.name] = hashlib.file_digest(source, "sha256").hexdigest()
            elif relative.parts[:2] == ("licenses", "media"):
                license_path = package.joinpath(*relative.parts)
                license_path.parent.mkdir(parents=True, exist_ok=True)
                data = bundle.extractfile(entry).read()
                if license_path.exists() and license_path.read_bytes() != data:
                    raise ValueError(f"{archive}: inconsistent media license {relative}")
                license_path.write_bytes(data)
    if set(digests) != {"library.xz", BINDINGS}:
        raise ValueError(f"{archive}: incomplete media payload for {host}")
    manifest = package / "Cargo.toml"
    text = manifest.read_text()
    if 'publish = false' not in text or 'path = "../stub.rs"' not in text:
        raise ValueError(f"{manifest}: expected a media payload placeholder")
    manifest.write_text(text.replace("publish = false\n", "").replace("../stub.rs", "src/lib.rs"))
    source = package / "src"
    source.mkdir()
    with lzma.open(package / "library.xz", "rb") as library:
        digest = hashlib.file_digest(library, "sha256").hexdigest()
    (source / "lib.rs").write_text(
        '#![allow(nonstandard_style, dead_code, unnecessary_transmutes)]\n'
        'include!(concat!(env!("CARGO_MANIFEST_DIR"), "/ffmpeg_bindings.rs"));\n'
        'pub const LIBRARY: &[u8] = include_bytes!("../library.xz");\n'
        f'pub const LIBRARY_SHA256: &str = "{digest}";\n'
    )
    return digests


def source_asset(version, host):
    return f"xrack-media-source-{version}-{host}.tar.gz"


def source_notice(version):
    return (
        "XRack uses FFmpeg under LGPL 2.1 or later. Corresponding source files "
        "and build inputs for each platform are published with this release:\n\n"
        + "".join(
            f"- https://github.com/smallsunsun1/xrack/releases/download/v{version}/"
            f"{source_asset(version, host)}\n" for host in bundles.hosts()
        )
        + "\nReplace the native library using an absolute XRACK_MEDIA_LIBRARY path.\n"
    )


def install_library(archive, host, directory):
    """Extract exactly the native library packaged with this release."""
    output = directory / library_name(host)
    with tarfile.open(archive, "r:gz") as bundle:
        for relative, entry in bundles.archive_members(bundle, archive):
            if relative.as_posix() == "media/library.xz":
                with bundle.extractfile(entry) as source, lzma.open(source, "rb") as raw, output.open("xb") as target:
                    shutil.copyfileobj(raw, target)
                return output
    raise ValueError(f"{archive}: missing native media library")


def source_archive(external, bin_dir, destination):
    """Archive the exact patched Bazel source tree and native configuration."""
    ffmpeg = external / "ffmpeg+"
    miniaudio = external / "+http_archive+miniaudio"
    native = bin_dir / "external/ffmpeg+"
    with tarfile.open(destination, "w:gz", dereference=True) as bundle:
        for path in sorted(ffmpeg.iterdir()):
            if not path.name.startswith((".", "bazel-")):
                bundle.add(path, arcname="ffmpeg/" + path.name)
        for name in ("miniaudio.h", "LICENSE"):
            bundle.add(miniaudio / name, arcname="miniaudio/" + name)
        for name in ("config.h", "config_components.h", "libavutil/avconfig.h"):
            bundle.add(native / name, arcname="build-config/" + name)
        for name in ("BUILD.bazel", "ffmpeg.h", "audio_device.c", "native_api.c", "native_api.h"):
            bundle.add(ROOT / "crates/xrack-media" / name, arcname="xrack-media/" + name)
        for name in (".bazelrc", ".bazelversion", "MODULE.bazel", "MODULE.bazel.lock"):
            bundle.add(ROOT / name, arcname="xrack-build/" + name)
        bundle.add(ROOT / "bazel/ffmpeg_hls_window.patch", arcname="xrack-build/bazel/ffmpeg_hls_window.patch")
        for name in ("zlib+", "bzip2+", "openssl+"):
            directory = external / name
            for path in sorted(directory.iterdir()):
                if not path.name.startswith((".", "bazel-")):
                    bundle.add(path, arcname=f"{name[:-1]}/{path.name}")
