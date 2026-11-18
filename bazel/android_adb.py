"""Run the ADB downloaded by the pinned Bazel Android SDK."""

import os
import sys
from pathlib import Path

from python.runfiles import runfiles


def main() -> None:
    files = runfiles.Create()
    if files is None or len(sys.argv) < 2:
        raise SystemExit("Run ADB with bazel run //bazel:adb -- <arguments>")
    location = files.Rlocation(sys.argv[1])
    if location is None:
        raise SystemExit("The Bazel Android SDK did not provide ADB")
    adb = str(Path(location).resolve(strict=True))
    os.execv(adb, [adb, *sys.argv[2:]])


if __name__ == "__main__":
    main()
