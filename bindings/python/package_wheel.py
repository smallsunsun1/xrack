"""Build, test, and validate native XRack Python distributions."""

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
import venv
import zipfile
from email import policy
from email.parser import BytesParser
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MATRIX: dict[str, tuple[str, str]] = {
    "aarch64-apple-darwin": ("macosx_13_0_arm64", "macosx_13_0_arm64"),
    "x86_64-apple-darwin": ("macosx_13_0_x86_64", "macosx_13_0_x86_64"),
    "aarch64-unknown-linux-gnu": ("linux_aarch64", "manylinux_2_39_aarch64"),
    "x86_64-unknown-linux-gnu": ("linux_x86_64", "manylinux_2_39_x86_64"),
    "x86_64-pc-windows-msvc": ("win_amd64", "win_amd64"),
    "aarch64-pc-windows-msvc": ("win_arm64", "win_arm64"),
}
ABI = "cp311-abi3"
BAZEL_OPTIONS = ["--lockfile_mode=update", "--repo_env=CARGO_BAZEL_ISOLATED=false"]


def version() -> str:
    with (ROOT / "Cargo.toml").open("rb") as manifest:
        return str(tomllib.load(manifest)["workspace"]["package"]["version"])


def verify_wheel(wheel: Path, release_version: str, platform: str) -> None:
    dist_info = f"xrack-{release_version}.dist-info"
    filename_prefix = f"xrack-{release_version}-{ABI}-"
    if not wheel.name.startswith(filename_prefix):
        raise ValueError(f"{wheel.name} does not target {ABI} for XRack {release_version}")
    filename_platforms = wheel.stem.removeprefix(filename_prefix).split(".")
    if platform not in filename_platforms:
        raise ValueError(f"{wheel.name} does not include the {platform} platform tag")
    parser = BytesParser(policy=policy.default)
    with zipfile.ZipFile(wheel) as archive:
        if bad_file := archive.testzip():
            raise ValueError(f"Corrupt wheel entry {bad_file!r}")
        metadata = parser.parsebytes(archive.read(f"{dist_info}/METADATA"))
        if metadata["Name"] != "xrack" or metadata["Version"] != release_version:
            raise ValueError(f"Unexpected metadata in {wheel.name}")
        wheel_info = parser.parsebytes(archive.read(f"{dist_info}/WHEEL"))
        if wheel_info[
            "Root-Is-Purelib"
        ] != "false" or f"{ABI}-{platform}" not in wheel_info.get_all("Tag", []):
            raise ValueError(f"Missing native platform tag in {wheel.name}")
        ext = "pyd" if platform.startswith("win_") else "so"
        for name in [
            "__init__.py",
            "values.py",
            "stubgen.py",
            "py.typed",
            "_native.pyi",
            f"_native.{ext}",
        ]:
            archive.getinfo(f"xrack/{name}")
        for license_name in ["LICENSE-APACHE", "LICENSE-MIT"]:
            archive.getinfo(f"{dist_info}/licenses/{license_name}")
        stub = archive.read("xrack/_native.pyi").decode("utf-8")
        declarations = (
            "class Component",
            "class Program",
            "class Brush",
            "class Record",
            "def call_on_ui(",
        )
        if not all(item in stub for item in declarations):
            raise ValueError(f"Missing generated declarations in {wheel.name}")


def verify_linux_cpp_runtime(wheel: Path, directory: Path) -> None:
    with zipfile.ZipFile(wheel) as archive:
        binding = archive.extract("xrack/_native.so", path=directory)
    dynamic = subprocess.check_output(["readelf", "-d", binding], text=True)
    needed = re.findall(r"\(NEEDED\).*\[([^\]]+)\]", dynamic)
    shared_runtimes = [
        name
        for name in needed
        if name.startswith(("libc++.so", "libc++abi.so", "libstdc++.so", "libunwind.so"))
    ]
    if shared_runtimes:
        raise ValueError(f"{wheel.name} links shared C++ runtimes: {', '.join(shared_runtimes)}")


def smoke_test(wheel: Path) -> None:
    with tempfile.TemporaryDirectory(prefix="xrack-wheel-") as temporary:
        directory = Path(temporary)
        venv.EnvBuilder(with_pip=True, symlinks=sys.platform != "win32").create(directory / "venv")
        python = directory / (
            "venv/Scripts/python.exe" if sys.platform == "win32" else "venv/bin/python"
        )
        subprocess.run(
            [
                str(python),
                "-I",
                "-m",
                "pip",
                "install",
                "--disable-pip-version-check",
                "--no-deps",
                "--no-index",
                str(wheel),
            ],
            cwd=directory,
            check=True,
        )
        shutil.copyfile(ROOT / "bindings/python/binding_test.py", directory / "binding_test.py")
        shutil.copyfile(ROOT / "examples/async_demo/async.xr", directory / "async.xr")
        subprocess.run(
            [str(python), "-I", "binding_test.py", "async.xr"],
            cwd=directory,
            check=True,
            timeout=60,
        )


def build(target: str, output: Path) -> None:
    raw_platform, final_platform = MATRIX[target]
    release_version = version()
    subprocess.run(
        ["bazel", "build", *BAZEL_OPTIONS, "//bindings/python:wheel"],
        cwd=ROOT,
        check=True,
    )
    bin_dir = Path(
        subprocess.check_output(
            ["bazel", "info", *BAZEL_OPTIONS, "bazel-bin"], cwd=ROOT, text=True
        ).strip()
    )
    raw_wheel = bin_dir / "bindings/python" / f"xrack-{release_version}-{ABI}-{raw_platform}.whl"
    output.mkdir(parents=True, exist_ok=True)
    # Only publish the artifact after verification and isolated installation.
    with tempfile.TemporaryDirectory(prefix="wheel-", dir=output) as staging_dir:
        staging = Path(staging_dir)
        if raw_platform.startswith("linux_"):
            verify_linux_cpp_runtime(raw_wheel, staging / "native")
            subprocess.run(
                [
                    sys.executable,
                    "-m",
                    "auditwheel",
                    "repair",
                    "--plat",
                    final_platform,
                    "--wheel-dir",
                    str(staging),
                    str(raw_wheel),
                ],
                check=True,
            )
            wheels = list(staging.glob("*.whl"))
            if len(wheels) != 1:
                raise ValueError("Expected auditwheel to produce one wheel")
            staged_wheel = wheels[0]
        else:
            staged_wheel = staging / raw_wheel.name
            shutil.copyfile(raw_wheel, staged_wheel)
        verify_wheel(staged_wheel, release_version, final_platform)
        smoke_test(staged_wheel)
        artifact = output / staged_wheel.name
        staged_wheel.replace(artifact)
    print(artifact)


def verify_release(directory: Path, tag: str) -> None:
    release_version = version()
    if tag != f"v{release_version}":
        raise ValueError(f"Tag {tag!r} does not match XRack {release_version}")
    remaining = set(directory.glob("*.whl"))
    for _, expected in MATRIX.values():
        matching = [
            wheel for wheel in remaining if expected in wheel.stem.rsplit("-", 1)[-1].split(".")
        ]
        if len(matching) != 1:
            raise ValueError(f"Expected exactly one wheel for {expected}")
        wheel = matching[0]
        verify_wheel(wheel, release_version, expected)
        remaining.remove(wheel)
    if remaining:
        raise ValueError(f"Unexpected wheel files: {sorted(remaining)}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    package = commands.add_parser("build")
    package.add_argument("--target", choices=MATRIX, required=True)
    package.add_argument("--output", type=Path, required=True)
    release = commands.add_parser("verify-release")
    release.add_argument("--artifacts", type=Path, required=True)
    release.add_argument("--tag", required=True)
    args = parser.parse_args()
    if args.command == "build":
        build(args.target, args.output.resolve())
    else:
        verify_release(args.artifacts, args.tag)


if __name__ == "__main__":
    main()
