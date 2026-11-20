"""Verify the actual files written by the Pocket Tasks simulator test."""

from __future__ import annotations

import os
import subprocess
from pathlib import Path


def main() -> int:
    result = int(os.environ["TEST_EXIT_CODE"])
    device = os.environ["SIMULATOR_UDID"]
    log = Path(os.environ["TEST_LOG_FILE"]).read_text(errors="replace")
    if (
        device == "unused"
        or "testPhotoPickerExportsBothOriginalsAndKeepsThePreviewOnCancel" not in log
    ):
        return result
    if result and "2 originals saved" not in log:
        return result

    app = Path(
        subprocess.check_output(
            [
                "xcrun",
                "simctl",
                "get_app_container",
                device,
                "dev.xrack.ios-demo",
                "data",
            ],
            text=True,
            timeout=20,
        ).strip()
    )
    documents = app / "Documents"
    files: list[Path] = sorted(documents.glob("photo-*"))
    originals = [path for path in files if not path.stem.endswith(" (2)")]
    expected: dict[str, tuple[int, int]] = {"png": (96, 64), "heic": (64, 96)}
    if (
        len(files) != 2 * len(expected)
        or len(originals) != len(expected)
        or any(documents.glob(".xrack-export-*"))
    ):
        print(f"Expected two completed exports, found {[path.name for path in files]}.")
        return result or 1

    actual: dict[str, tuple[int, int]] = {}
    for path in originals:
        retry = path.with_stem(f"{path.stem} (2)")
        if not retry.is_file() or retry.read_bytes() != path.read_bytes():
            print(f"Expected a byte-identical sibling copy of {path.name}.")
            return result or 1
        description = subprocess.check_output(
            [
                "sips",
                "-g",
                "format",
                "-g",
                "pixelWidth",
                "-g",
                "pixelHeight",
                str(path),
            ],
            text=True,
            timeout=15,
        )
        fields = dict(
            line.strip().split(": ", 1)
            for line in description.splitlines()
            if line.startswith("  ")
        )
        actual[fields["format"]] = (
            int(fields["pixelWidth"]),
            int(fields["pixelHeight"]),
        )
    if actual != expected:
        print(
            f"Expected PNG and HEIC originals in Pocket Tasks Documents, found {actual}."
        )
        return result or 1
    print(f"Verified both originals and their sibling copies in Pocket Tasks Documents: {actual}")
    return result


if __name__ == "__main__":
    raise SystemExit(main())
