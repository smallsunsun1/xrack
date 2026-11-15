"""Verify Python hot reload through the running-app editor protocol."""

from __future__ import annotations

import asyncio
import json
import sys
import tempfile
from pathlib import Path
from typing import Any, cast

import xrack


async def snapshot(socket: str) -> dict[str, Any]:
    reader, writer = await asyncio.open_unix_connection(socket)
    try:
        return cast(dict[str, Any], json.loads(await reader.read()))
    finally:
        writer.close()
        await writer.wait_closed()


def run(source: Path, project: Path) -> None:
    path = project / "async.xr"
    initial = source.read_text(encoding="utf-8").replace(
        '    title: "XRack · Async Rust";',
        '    title: "XRack · Async Rust";\n    out property <string> reload-marker: "original";',
    )
    path.write_text(initial, encoding="utf-8")
    ui = xrack.load_file(path, live=True, project=project).create("AsyncDemo")
    session = next((project / ".xrack-live").glob("*.json"))
    metadata = json.loads(session.read_text(encoding="utf-8"))
    started = asyncio.Event()
    finish = asyncio.Event()
    searches = 0

    def begin_search(query: str) -> None:
        ui.set("loading", True)

    async def search(query: str) -> list[str]:
        nonlocal searches
        searches += 1
        started.set()
        await finish.wait()
        return [query]

    def publish(rows: list[str]) -> None:
        ui.set("results", rows)
        ui.set("loading", False)

    async def await_value(name: str, value: xrack.Value) -> None:
        async with asyncio.timeout(5):
            while await ui.call_on_ui(ui.get, name) != value:
                await asyncio.sleep(0.02)

    async def main() -> None:
        await asyncio.wait_for(started.wait(), 5)
        reader, writer = await asyncio.open_unix_connection(metadata["editorSocket"])
        try:
            assert json.loads(await reader.readline())["version"] == 1
            draft = initial.replace('"original"', '"edited"').replace('"搜索";', '"Python search";')
            writer.write(
                (
                    json.dumps({"version": 1, "buffers": {str(path.resolve()): draft}}) + "\n"
                ).encode()
            )
            writer.write(
                (json.dumps({"version": 1, "inspection": {"type": "attach"}}) + "\n").encode()
            )
            await writer.drain()

            await await_value("reload-marker", "edited")
            assert await ui.call_on_ui(ui.get, "loading") is True
            finish.set()
            await await_value("results", ["XRack"])
            state = await snapshot(metadata["socket"])
            assert any(p["name"] == "results" and p["count"] == 1 for p in state["properties"])
            assert any(node["kind"] == "SearchField" for node in state["tree"]["nodes"])
            assert state["inspection"] is not None
            assert path.read_text(encoding="utf-8") == initial
            await ui.call_on_ui(ui.close)
        finally:
            writer.close()
            await writer.wait_closed()

    ui.on_request("search", search, publish, on_start=begin_search)
    ui.run(main)
    assert searches == 1
    print("Reloaded editor buffer while the Python search was pending")


if __name__ == "__main__":
    with tempfile.TemporaryDirectory() as directory:
        run(Path(sys.argv[1]), Path(directory))
