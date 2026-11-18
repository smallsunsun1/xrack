"""Configure the software keyboard on the temporary XCTest simulator."""

from __future__ import annotations

import os
import subprocess


def main() -> None:
    device: str = os.environ["SIMULATOR_UDID"]
    if device == "unused":
        return

    # Seed the keyboard before the first UIKit text responder starts.
    command: list[str] = ["xcrun", "simctl", "spawn", device, "defaults", "write", "-g"]
    for arguments in (
        ["AppleLanguages", "-array", "en-US"],
        ["AppleLocale", "-string", "en_US"],
        ["AppleKeyboards", "-array", "en_US@sw=QWERTY;hw=Automatic"],
    ):
        subprocess.run([*command, *arguments], check=True)


if __name__ == "__main__":
    main()
