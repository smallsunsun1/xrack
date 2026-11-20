# XRack Python bindings

`load_file(path)` compiles an `.xr` file and resolves its imports and assets. `load_mlir_file(path)` loads exported XRack MLIR and its file resources. Call `program.create(name)` to instantiate an exported component.

The Python API reads and writes public properties with `get` and `set`, restores declarative bindings with `reset`, and exposes callbacks through `on` and `invoke`. Coroutine handlers can be connected to void callbacks.

Public values include Python scalars and string lists, plus XRack types for lengths and percentages, colors and brushes, dates, times, fonts, styled text, images, records, input events and enums. The same types are used for property values and callback arguments and results.

```python
ui.set("accent", xrack.Color.from_hex("#536dfe"))
ui.set("spacing", xrack.Length(12))
ui.set("direction", xrack.LayoutDirection.RightToLeft)
```

`ui.get("names")` returns a string-list snapshot. `ui.string_model("names")` gives you the live model of a writable list. Use `ui.record_model("rows")` to append, replace, or delete individual records. A replacement keeps the row identity. XRack validates a new row against the schema before writing it. `ui.get("rows")` remains a snapshot.

Use the live models and UI-owned values such as images and records on the main thread. From a coroutine, pass plain data back and construct or inspect these values inside `call_on_ui`.

Run the existing asynchronous search UI with Python business logic:

```sh
bazel run //bindings/python:async_demo
```

Call `ui.run()` on Python's main thread. XRack owns the native window there and runs a standard `asyncio` loop on a dedicated Python thread. Synchronous callbacks run on the main thread. Coroutine handlers can use `asyncio.sleep`, `asyncio.open_connection`, and `asyncio.to_thread`. Use `await ui.call_on_ui(ui.set, "loading", True)` to make a UI change from a coroutine. The native binding uses `pyo3-async-runtimes` to return the result or exception to the caller's event loop. You can also pass an async startup function to `ui.run(main)`.

Closing the last window cancels outstanding asyncio tasks and waits for their cleanup. Coroutine cancellation is cooperative. Work already running in `asyncio.to_thread` follows Python's executor shutdown behavior. Start the native event loop once per process.

Use `ui.on_request("search", search, publish)` when a coroutine produces a result for the UI. The `.xr` callback is declared without a return value. The Python `search` coroutine can return any Python value accepted by `publish`, and only the latest request for that callback applies its result. Optional `on_start` and `on_error` hooks run on the UI thread, along with `publish`. See the asynchronous search example for how to update the loading state.

Use `await ui.invoke_async("result-label", item)` from a Python coroutine to retrieve an immediate XRack callback result. If the callback starts an asynchronous handler, the await finishes after the callback schedules the handler.

Build a native wheel with `bazel build //bindings/python:wheel`. Bazel generates `xrack/_native.pyi` from the native binding and includes it and `py.typed` in the wheel. For example, install the macOS ARM64 build as follows:

```sh
bazel build //bindings/python:wheel
python3.11 -m venv /tmp/xrack-python-venv
/tmp/xrack-python-venv/bin/pip install bazel-bin/bindings/python/xrack-0.8.0-cp311-abi3-macosx_13_0_arm64.whl
/tmp/xrack-python-venv/bin/python bindings/python/async_demo.py examples/async_demo/async.xr
```

From an installed package, run `python -m xrack.stubgen ui/async.xr -o async_ui.pyi`. This writes `async_ui.py` and `async_ui.pyi` together. Import `AsyncDemo` from `async_ui` to use typed properties such as `ui.query` and typed callback registration such as `ui.on_search(handler)`. Re-run the generator after changing the component's public API.

Open the Python example as a reloadable application with `bazel run //bindings/python:async_demo -- --live --project "$PWD"`. You can also pass `live=True` to `xrack.load_file` or to a generated component constructor. On Unix, open the project directory in VS Code and run **XRack: Sync Editor to Running App** to attach.

Format Python sources with `bazel run //bindings/python:format -- <files>`.

Run the binding tests with `bazel test //bindings/python:binding_test`. On a desktop session, run `bazel test //bindings/python:window_test` to exercise an actual window, and add `--test_arg=--close-early` to test cancellation.

## PyPI releases

The Python workflow builds CPython 3.11+ wheels for macOS ARM64, Linux ARM64 and x86-64, and Windows ARM64 and x86-64. The macOS wheel targets macOS 13 or later. Linux wheels are repaired and checked for the manylinux glibc 2.39 baseline. Each job installs its wheel into a clean virtual environment and runs the binding tests.

A stable version tag on `main` publishes the five verified wheels. The tag must match the workspace version and the wheel version. For the first release, configure a PyPI pending publisher for the distribution `xrack` with owner `smallsunsun1`, repository `xrack`, workflow `python.yml`, and GitHub environment `pypi`.
