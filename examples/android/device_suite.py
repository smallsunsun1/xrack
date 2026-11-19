import argparse
import fcntl
import hashlib
import json
import os
import shutil
import subprocess
import tempfile
import xml.etree.ElementTree as ET
from collections.abc import Iterator
from dataclasses import dataclass
from pathlib import Path
from typing import Any
from urllib.parse import unquote, urlsplit

from python.runfiles import runfiles


PACKAGE = "//examples/android"
APK_TAG = "xrack-device-apk:"
EXTRA_TAG = "xrack-device-extra:"
REPO_OPTIONS = ("--repo_env=CARGO_BAZEL_ISOLATED=false",)


@dataclass(frozen=True)
class TestCase:
    label: str
    apk: str
    extra_apks: tuple[tuple[str, str], ...]


def discover_cases(
    bazel: str, workspace: Path
) -> tuple[dict[str, TestCase], dict[str, str]]:
    result = subprocess.run(
        (
            bazel,
            "query",
            f'kind("py_test rule|android_binary rule", {PACKAGE}:*)',
            "--output=xml",
            "--noshow_progress",
            *REPO_OPTIONS,
        ),
        cwd=workspace,
        check=True,
        stdout=subprocess.PIPE,
    )
    cases: dict[str, TestCase] = {}
    manifests: dict[str, str] = {}
    for rule in ET.fromstring(result.stdout).findall("rule"):
        label = rule.attrib["name"]
        if rule.attrib["class"] == "android_binary":
            manifest = rule.find("./label[@name='manifest']")
            if manifest is not None:
                manifests[label.split(":", 1)[1]] = manifest.attrib["value"]
            continue
        if not label.endswith("_ui_test"):
            continue
        tags = [
            entry.attrib["value"]
            for entry in rule.findall("./list[@name='tags']/string")
        ]
        apks = [tag.removeprefix(APK_TAG) for tag in tags if tag.startswith(APK_TAG)]
        if len(apks) != 1:
            raise ValueError(f"{label} must declare one APK in android_device_test")
        extra = [
            tag.removeprefix(EXTRA_TAG).split("=", 1)
            for tag in tags
            if tag.startswith(EXTRA_TAG)
        ]
        extra_apks = tuple(sorted((argument, artifact) for argument, artifact in extra))
        cases[label] = TestCase(label, apks[0], extra_apks)
    if not cases:
        raise ValueError("No Android device tests were found")
    return cases, manifests


def build_events(path: Path) -> Iterator[dict[str, Any]]:
    with path.open(encoding="utf-8") as stream:
        for line in stream:
            yield json.loads(line)


def build_apks(
    bazel: str, workspace: Path, output: Path, artifacts: set[str]
) -> dict[str, str]:
    bep = output / "build.jsonl"
    subprocess.run(
        (
            bazel,
            "build",
            *REPO_OPTIONS,
            "--config=android",
            "--remote_download_outputs=all",
            f"--symlink_prefix={output.relative_to(workspace)}/bazel-",
            f"--build_event_json_file={bep}",
            *(f"{PACKAGE}:{name}" for name in sorted(artifacts)),
        ),
        cwd=workspace,
        check=True,
    )
    names = {f"examples/android/{name}.apk": name for name in artifacts}
    hashes: dict[str, str] = {}
    for event in build_events(bep):
        for file in event.get("namedSetOfFiles", {}).get("files", []):
            name = names.get(file.get("name"))
            if name is None or name in hashes:
                continue
            uri = urlsplit(file.get("uri", ""))
            expected = file.get("digest")
            if uri.scheme != "file" or not expected:
                raise ValueError(
                    f"Bazel did not report a local APK and digest for {name}"
                )
            snapshot = output / f"{name}.apk"
            shutil.copyfile(Path(unquote(uri.path)), snapshot)
            with snapshot.open("rb") as stream:
                actual = hashlib.file_digest(stream, "sha256").hexdigest()
            if actual != expected:
                raise ValueError(f"{name}.apk changed after the build")
            hashes[name] = actual
    if missing := artifacts - hashes.keys():
        raise ValueError(
            f"The build report is missing APKs for {', '.join(sorted(missing))}"
        )
    return hashes


def android_package(workspace: Path, manifest: str) -> str:
    package_path, basename = manifest.removeprefix("//").split(":", 1)
    root = ET.parse(workspace / package_path / basename).getroot()
    package = root.get("package")
    if not package:
        raise ValueError(f"{manifest} does not declare an Android package")
    return package


def stop_apps(adb: Path, serial: str, packages: set[str]) -> None:
    for package in sorted(packages):
        subprocess.run(
            (adb, "-s", serial, "shell", "am", "force-stop", package),
            check=True,
            timeout=30,
        )


def run_cases(
    bazel: str,
    workspace: Path,
    output: Path,
    adb: Path,
    serial: str,
    cases: list[TestCase],
    manifests: dict[str, str],
) -> int:
    artifacts = {case.apk for case in cases}
    artifacts.update(apk for case in cases for _, apk in case.extra_apks)
    if missing := artifacts - manifests.keys():
        raise ValueError(f"Missing APK manifests for {', '.join(sorted(missing))}")
    packages = {apk: android_package(workspace, manifests[apk]) for apk in artifacts}
    hashes = build_apks(bazel, workspace, output, artifacts)
    results = dict.fromkeys((case.label for case in cases), "NOT_RUN")
    report = output / "results.json"
    print(f"Android device suite results: {report}", flush=True)

    def write_report() -> None:
        pending = report.with_suffix(".tmp")
        pending.write_text(
            json.dumps(
                {
                    "device": serial,
                    "apk_sha256": dict(sorted(hashes.items())),
                    "tests": results,
                },
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )
        pending.replace(report)

    write_report()
    stop_apps(adb, serial, set(packages.values()))
    command_failed = False
    for index, case in enumerate(cases, 1):
        bep = output / f"tests-{index}.jsonl"
        result = subprocess.run(
            (
                bazel,
                "test",
                *REPO_OPTIONS,
                "--test_output=streamed",
                "--nocache_test_results",
                "--test_env=DEVELOPER_DIR",
                f"--symlink_prefix={output.relative_to(workspace)}/bazel-",
                f"--build_event_json_file={bep}",
                f"--test_arg=--serial={serial}",
                f"--test_arg=--apk={output / (case.apk + '.apk')}",
                *(
                    f"--test_arg=--{argument}={output / (artifact + '.apk')}"
                    for argument, artifact in case.extra_apks
                ),
                case.label,
            ),
            cwd=workspace,
            check=False,
        )
        command_failed |= result.returncode != 0
        if bep.exists():
            for event in build_events(bep):
                label = event.get("id", {}).get("testSummary", {}).get("label")
                if label == case.label:
                    results[label] = event["testSummary"].get(
                        "overallStatus", "NO_STATUS"
                    )
        write_report()
        # Android system dialogs can stay in front of the next fixture even
        # after the next Activity has started.
        touched = {packages[case.apk]}
        touched.update(packages[apk] for _, apk in case.extra_apks)
        stop_apps(adb, serial, touched)
    passed = sum(result == "PASSED" for result in results.values())
    for label, result in results.items():
        if result != "PASSED":
            print(f"{result} {label}")
    print(f"{passed}/{len(cases)} Android device tests passed. Results: {report}")
    return int(command_failed or passed != len(cases))


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Build once and run XRack Android device tests"
    )
    parser.add_argument("--serial", help="ADB serial of the running emulator")
    parser.add_argument("--list", action="store_true", help="List tests and their APKs")
    parser.add_argument("--adb-runfile", required=True, help=argparse.SUPPRESS)
    parser.add_argument("targets", nargs="*", help="Test names or full Bazel labels")
    args = parser.parse_intermixed_args()
    if not args.list and not args.serial:
        parser.error("--serial is required when running tests")
    bazel = shutil.which("bazel")
    if bazel is None:
        parser.error("bazel must be available on PATH")
    workspace = Path(os.environ.get("BUILD_WORKSPACE_DIRECTORY", str(Path.cwd())))
    try:
        catalog, manifests = discover_cases(bazel, workspace)
        requested = {
            name if name.startswith("//") else f"{PACKAGE}:{name}"
            for name in args.targets
        }
        if unknown := requested - catalog.keys():
            parser.error(f"unknown Android device tests: {', '.join(sorted(unknown))}")
        cases = [catalog[label] for label in sorted(requested or catalog.keys())]
        if args.list:
            for case in cases:
                extra = " ".join(
                    f"--{argument}={apk}.apk" for argument, apk in case.extra_apks
                )
                print(f"{case.label} --apk={case.apk}.apk {extra}".rstrip())
            return 0
        files = runfiles.Create()
        adb_location = files.Rlocation(args.adb_runfile) if files else None
        if adb_location is None:
            parser.error(
                "Run the device suite using bazel run //examples/android:device_suite"
            )
        adb = Path(adb_location).resolve(strict=True)
        subprocess.run((adb, "-s", args.serial, "get-state"), check=True, timeout=30)
        runs = workspace / ".tools" / "android"
        runs.mkdir(parents=True, exist_ok=True)
        serial_hash = hashlib.sha256(args.serial.encode()).hexdigest()
        with (runs / f"device-suite-{serial_hash}.lock").open("a") as lock:
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                parser.exit(1, f"A device suite is already running on {args.serial}\n")
            output = Path(tempfile.mkdtemp(prefix="device-suite-", dir=runs))
            return run_cases(
                bazel, workspace, output, adb, args.serial, cases, manifests
            )
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        parser.exit(1, f"{error}\n")


if __name__ == "__main__":
    raise SystemExit(main())
