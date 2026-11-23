"""Drive the native macOS puzzle through the XRack editor protocol."""

from __future__ import annotations

import json
import os
import socket
import subprocess
import sys
import tempfile
import time
from collections.abc import Callable
from pathlib import Path
from typing import Any, TypeVar, cast

T = TypeVar("T")


def wait_for(check: Callable[[], T | None], label: str) -> T:
    deadline = time.monotonic() + 12.0
    while time.monotonic() < deadline:
        result = check()
        if result is not None:
            return result
        time.sleep(0.06)
    raise AssertionError(f"Timed out waiting for {label}")


def request(path: str, message: dict[str, Any] | None = None) -> dict[str, Any]:
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(5.0)
        connection.connect(path)
        if message is not None:
            connection.sendall((json.dumps(message) + "\n").encode())
        with connection.makefile("rb") as incoming:
            return cast(dict[str, Any], json.loads(incoming.readline()))


def property_values(snapshot: dict[str, Any]) -> dict[str, Any]:
    return {p["name"]: p.get("value") for p in snapshot["properties"]}


def viewport_width(snapshot: dict[str, Any]) -> float:
    value = cast(str, property_values(snapshot)["viewport-width"])
    return float(value.removesuffix("px"))


def exercise(process: subprocess.Popen[bytes], project: Path, source: Path) -> None:
    def descriptor() -> dict[str, Any] | None:
        if process.poll() is not None:
            raise AssertionError(f"Play stopped during startup ({process.returncode})")
        path = next((project / ".xrack-live").glob("*.json"), None)
        try:
            return cast(dict[str, Any], json.loads(path.read_text())) if path else None
        except (OSError, ValueError):
            return None

    session = wait_for(descriptor, "Play descriptor")

    def snapshot() -> dict[str, Any]:
        state = request(session["socket"])
        assert "error" not in state, state.get("error")
        return state

    def ready() -> dict[str, Any] | None:
        try:
            state = snapshot()
            return state if state.get("tree") and "trace" in state else None
        except (OSError, AssertionError):
            return None

    initial = wait_for(ready, "native frame and inspection tree")
    original_width = viewport_width(initial)
    assert original_width > 400, original_width

    def click(id: str) -> dict[str, Any]:
        generation = snapshot()["trace"]["generation"]
        reply = request(
            session["querySocket"],
            {"generation": generation, "input": {"type": "click", "id": id}},
        )
        assert reply.get("applied"), reply
        return property_values(snapshot())

    assert click("north")["player-z"] == "0"
    assert click("amber-action")["amber-on"] is True
    assert click("toggle-view")["map-view"] is True
    assert property_values(snapshot())["gate-open"] is False
    assert property_values(snapshot())["gate-angle"] == "0"

    actual_path = Path(session["source"]).resolve()
    original = source.read_text(encoding="utf-8")
    assert actual_path.read_text(encoding="utf-8") == original
    edited = original.replace("light-yaw: 42;", "light-yaw: -58;").replace(
        "panel-width: 204px;", "panel-width: 248px;"
    )
    assert edited != original
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as editor:
        editor.settimeout(5.0)
        editor.connect(session["editorSocket"])
        with editor.makefile("rb") as ack:
            assert json.loads(ack.readline())["version"] == 1
        editor.sendall(
            (
                json.dumps({"version": 1, "buffers": {str(actual_path): edited}}) + "\n"
            ).encode()
        )

        def reloaded() -> dict[str, Any] | None:
            state = snapshot()
            values = property_values(state)
            if (
                values.get("light-yaw") == "-58"
                and values.get("panel-width") == "248px"
            ):
                if viewport_width(state) < original_width - 20:
                    return state
            return None

        current = property_values(wait_for(reloaded, "edited light and viewport size"))
        assert current["amber-on"] is True
        assert current["player-z"] == "0"
        assert current["map-view"] is True
        assert click("toggle-view")["map-view"] is False
        assert actual_path.read_text(encoding="utf-8") == original
        # The blue switch verifies the Rust game state as well as the UI
        # property. It refuses to open the door without the preserved amber state.
        gate = click("blue-action")
        assert gate["gate-open"] is True
        assert gate["gate-angle"] == "-90"
        assert click("north")["player-z"] == "-1"
        assert click("north")["finished"] is True
    print(
        "Native Play moved and activated a switch, retained state after editor sync, then finished"
    )


def main() -> None:
    executable = Path(sys.argv[1]).resolve()
    source = Path(sys.argv[2]).resolve()
    with (
        tempfile.TemporaryDirectory(prefix="xrack3d-") as directory,
        tempfile.TemporaryFile() as log,
    ):
        project = Path(directory)
        env = {
            **os.environ,
            "XRACK_LIVE_PROJECT": directory,
            "XRACK_LIVE_FILE": str(source),
            "XRACK_CONTROL_UI": "1",
            "XRACK_DEBUG_VALUES": "1",
        }
        with subprocess.Popen(
            [executable], env=env, stdout=log, stderr=subprocess.STDOUT
        ) as process:
            try:
                exercise(process, project, source)
            except Exception:
                log.seek(0)
                sys.stderr.write(log.read().decode(errors="replace"))
                raise
            finally:
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(3)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait(3)


if __name__ == "__main__":
    main()
