"""Use the existing XRack search UI with Python asyncio business logic."""

import argparse
import asyncio

import xrack


def main(path: str, *, live: bool = False, project: str | None = None) -> None:
    ui = xrack.load_file(path, live=live, project=project).create("AsyncDemo")

    async def search(query: str) -> tuple[str, list[str]]:
        await asyncio.sleep(0.35)  # Simulate a nonblocking network request.
        return query, [f"{query} · 结果 {index}" for index in range(1, 13)]

    def begin_search(query: str) -> None:
        ui.set("loading", True)

    def publish(result: tuple[str, list[str]]) -> None:
        query, rows = result
        ui.set("loading", False)
        if ui.get("query") == query:
            ui.set("result-query", query)
            ui.set("results", rows)
            ui.set("suggestions", rows[:8])

    def failed(error: Exception) -> None:
        ui.set("loading", False)
        ui.set("local-status", f"搜索失败：{error}")

    async def start_local() -> None:
        await ui.call_on_ui(ui.set, "local-status", "Python 协程等待中，界面仍可操作")
        await asyncio.sleep(0.5)
        await ui.call_on_ui(ui.set, "local-status", "Python 协程已完成")

    ui.on_request("search", search, publish, on_start=begin_search, on_error=failed)
    ui.on("start-local", start_local)
    ui.run()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("path")
    parser.add_argument("--live", action="store_true")
    parser.add_argument("--project")
    args = parser.parse_args()
    main(args.path, live=args.live, project=args.project)
