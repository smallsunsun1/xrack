"""Install the second, independent bundle into the temporary XCTest simulator."""

import os
from pathlib import Path
import subprocess
from tempfile import TemporaryDirectory


def main() -> None:
    device = os.environ["SIMULATOR_UDID"]
    archive = Path(__file__).with_name("clipboard_source.ipa")
    with TemporaryDirectory(prefix="xrack-clipboard-source-") as directory:
        subprocess.run(["unzip", "-q", str(archive), "-d", directory], check=True, timeout=20)
        app = Path(directory) / "Payload" / "ClipboardFixture.app"
        subprocess.run(["xcrun", "simctl", "install", device, str(app)], check=True, timeout=45)


if __name__ == "__main__":
    main()
