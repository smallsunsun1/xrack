"""Exercise a real native window. Requires access to the desktop."""

import asyncio
import sys
import threading

import xrack


def run(path: str, close_early: bool) -> None:
    ui = xrack.load_file(path).create("AsyncDemo")
    started = asyncio.Event()
    release_older = asyncio.Event()
    older_returned = asyncio.Event()
    cancelled = False

    async def search(query: str) -> list[str]:
        nonlocal cancelled
        if query == "XRack":
            started.set()
            try:
                await release_older.wait()
            except asyncio.CancelledError:
                cancelled = True
                raise
            older_returned.set()
        if query == "failure":
            raise ValueError("search failed")
        return [query]

    def begin_search(query: str) -> None:
        ui.set("loading", True)

    def publish(rows: list[str]) -> None:
        assert threading.current_thread() is threading.main_thread()
        ui.set("results", rows)
        ui.set("loading", False)

    def failed(error: Exception) -> None:
        assert threading.current_thread() is threading.main_thread()
        ui.set("local-status", str(error))
        ui.set("loading", False)

    async def wait_for_value(name: str, value: xrack.Value) -> None:
        async with asyncio.timeout(5):
            while await ui.call_on_ui(ui.get, name) != value:
                await asyncio.sleep(0.02)

    async def main() -> None:
        assert threading.current_thread() is not threading.main_thread()
        await asyncio.wait_for(started.wait(), 5)
        if close_early:
            assert await ui.call_on_ui(ui.get, "loading")
        else:
            assert await ui.invoke_async("result-label", "ready") == "ready"
            await ui.invoke_async("search", "newer")
            await wait_for_value("results", ["newer"])
            assert not await ui.call_on_ui(ui.get, "loading")
            release_older.set()
            await asyncio.wait_for(older_returned.wait(), 5)
            assert await ui.call_on_ui(ui.get, "results") == ["newer"]
            await ui.invoke_async("search", "failure")
            await wait_for_value("local-status", "search failed")
            assert not await ui.call_on_ui(ui.get, "loading")
            try:
                await ui.call_on_ui(ui.set, "selection", "invalid")
            except TypeError:
                pass
            else:
                raise AssertionError("The asyncio bridge did not preserve the Python exception")
            await asyncio.sleep(0.1)
        await ui.call_on_ui(ui.close)

    ui.on_request("search", search, publish, on_start=begin_search, on_error=failed)
    ui.run(main)
    assert cancelled == close_early
    assert ui.get("results") == ([] if close_early else ["newer"])
    print(
        "Window closed with pending search cancelled"
        if close_early
        else "Latest return reached the native UI; stale result and current error handled"
    )


if __name__ == "__main__":
    run(sys.argv[1], "--close-early" in sys.argv[2:])
