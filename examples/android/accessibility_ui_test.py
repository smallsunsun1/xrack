import argparse
import os
import subprocess
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Exercise XRack with Android UiAutomation"
    )
    parser.add_argument("--adb", type=Path, required=True)
    parser.add_argument("--serial", required=True)
    parser.add_argument("--apk", type=Path, required=True)
    parser.add_argument("--probe-apk", type=Path, required=True)
    scenarios = parser.add_mutually_exclusive_group()
    scenarios.add_argument(
        "--inventory", dest="scenario", action="store_const", const="inventory"
    )
    scenarios.add_argument(
        "--text", dest="scenario", action="store_const", const="text"
    )
    scenarios.add_argument(
        "--password", dest="scenario", action="store_const", const="password"
    )
    scenarios.add_argument(
        "--traversal", dest="scenario", action="store_const", const="traversal"
    )
    parser.set_defaults(scenario="controls")
    args = parser.parse_args()
    adb = [str(args.adb.resolve(strict=True)), "-s", args.serial]

    def call(*command: str) -> bytes:
        return subprocess.check_output([*adb, *command], timeout=180)

    rotation = call("shell", "cmd", "window", "user-rotation").decode().split()
    probe_installed = False
    try:
        call("shell", "cmd", "window", "user-rotation", "lock", "0")
        call("install", "-r", str(args.apk.resolve(strict=True)))
        call("install", "-r", str(args.probe_apk.resolve(strict=True)))
        probe_installed = True
        scenario = args.scenario
        package = {
            "controls": "dev.xrack.android_controls",
            "inventory": "dev.xrack.android_table",
            "text": "dev.xrack.android_demo",
            "password": "dev.xrack.android_controls",
            "traversal": "dev.xrack.android_demo",
        }[scenario]
        call("shell", "am", "force-stop", package)
        call(
            "shell",
            "am",
            "start",
            "-W",
            "-f",
            "0x10008000",
            "-n",
            f"{package}/dev.xrack.platform.XRackActivity",
        )
        process = call("shell", "pidof", "-s", package).strip()
        result = call(
            "shell",
            "am",
            "instrument",
            "-w",
            "-r",
            "-e",
            "scenario",
            scenario,
            "dev.xrack.accessibility_test/.ControlsProbe",
        ).decode()
        print(result, flush=True)
        artifacts = Path(os.environ.get("TEST_UNDECLARED_OUTPUTS_DIR", "."))
        artifacts.mkdir(parents=True, exist_ok=True)
        (artifacts / f"accessibility-{scenario}.png").write_bytes(
            call("exec-out", "screencap", "-p")
        )
        assert (
            "PASS accessibility " in result and "INSTRUMENTATION_CODE: -1" in result
        ), result
    finally:
        try:
            if probe_installed:
                call("uninstall", "dev.xrack.accessibility_test")
        finally:
            call("shell", "cmd", "window", "user-rotation", *rotation)

    assert call("shell", "pidof", "-s", package).strip() == process, (
        "XRack restarted when UiAutomation disconnected"
    )


if __name__ == "__main__":
    main()
