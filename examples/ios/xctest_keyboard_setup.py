"""Configure the software keyboard and photo fixtures on the temporary XCTest simulator."""

from __future__ import annotations

import os
import struct
import subprocess
import zlib
from pathlib import Path
from tempfile import TemporaryDirectory


def png_chunk(kind: bytes, content: bytes) -> bytes:
    return struct.pack(">I", len(content)) + kind + content + struct.pack(
        ">I", zlib.crc32(kind + content)
    )


def photo_png(color: bytes, width: int, height: int) -> bytes:
    rows = (bytes(1) + color * width) * height
    return (
        bytes.fromhex("89504e470d0a1a0a")
        + png_chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
        + png_chunk(b"IDAT", zlib.compress(rows))
        + png_chunk(b"IEND", b"")
    )


def seed_photos(device: str) -> None:
    with TemporaryDirectory(prefix="xrack-photo-fixtures-") as directory:
        png = Path(directory) / "pressure.png"
        source = Path(directory) / "reencode.png"
        heic = Path(directory) / "inspection.heic"
        png.write_bytes(photo_png(bytes((70, 135, 100)), 96, 64))
        source.write_bytes(photo_png(bytes((167, 90, 61)), 64, 96))
        subprocess.run(
            ["sips", "-s", "format", "heic", str(source), "--out", str(heic)],
            check=True,
            stdout=subprocess.DEVNULL,
        )
        subprocess.run(
            ["xcrun", "simctl", "addmedia", device, str(png), str(heic)],
            check=True,
        )


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
    if os.environ.get("XRACK_XCTEST_PHOTOS") != "0":
        seed_photos(device)


if __name__ == "__main__":
    main()
