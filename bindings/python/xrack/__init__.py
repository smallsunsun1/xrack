"""Load XRack components and connect them to Python callbacks."""

from __future__ import annotations

import asyncio
import inspect
import threading
import weakref
from collections.abc import Awaitable, Callable
from concurrent.futures import Future
from functools import partial
from os import PathLike
from typing import ParamSpec, TypeAlias, TypeVar, cast

from . import _native
from . import values as _values
from .values import *
from .values import RecordModel, StringModel, Value

__all__ = ["Component", "Program", "load_file", "load_mlir_file", *_values.__all__]

Handler: TypeAlias = Callable[..., Value | Awaitable[object] | None]
_Path: TypeAlias = str | PathLike[str]
_P = ParamSpec("_P")
_T = TypeVar("_T")


def _check_ui_thread() -> None:
    if threading.current_thread() is not threading.main_thread():
        raise RuntimeError("Access XRack components on the main thread or use call_on_ui()")


class Program:
    """A compiled UI from which independent component instances can be created."""

    def __init__(self, program: _native.Program) -> None:
        self._program = program

    def components(self) -> list[str]:
        _check_ui_thread()
        return self._program.components()

    def create(self, name: str) -> "Component":
        _check_ui_thread()
        return Component(self._program.create(name))


def load_file(path: _Path, *, live: bool = False, project: _Path | None = None) -> Program:
    """Compile an .xr file, resolving its imports and assets relative to the source."""
    _check_ui_thread()
    return Program(_native.load_file(path, live, project))


def load_mlir_file(path: _Path) -> Program:
    """Load an exported XRack MLIR file and its resources."""
    _check_ui_thread()
    return Program(_native.load_mlir_file(path))


class _AsyncRuntime:
    def __init__(self) -> None:
        ready: Future[tuple[asyncio.AbstractEventLoop, asyncio.Event]] = Future()

        async def serve() -> None:
            stop = asyncio.Event()
            ready.set_result((asyncio.get_running_loop(), stop))
            await stop.wait()

        def run() -> None:
            try:
                asyncio.run(serve())
            except BaseException as error:
                if not ready.done():
                    ready.set_exception(error)
                else:
                    raise

        self.thread = threading.Thread(target=run, name="XRack asyncio")
        self.thread.start()
        self.loop, self.stop = ready.result()

    def submit(self, awaitable: Awaitable[_T]) -> Future[_T]:
        async def wait_for_callback() -> _T:
            return await awaitable

        return asyncio.run_coroutine_threadsafe(wait_for_callback(), self.loop)

    def shutdown(self) -> None:
        self.loop.call_soon_threadsafe(self.stop.set)
        # asyncio.run cancels and waits for all remaining tasks before returning.
        self.thread.join()


class Component:
    """A single XRack component. UI methods execute on the main thread."""

    def __init__(self, instance: _native.Component) -> None:
        self._instance = instance
        self._handlers: dict[str, Callable[..., Value | None]] = {}
        self._runtime: _AsyncRuntime | None = None

    def get(self, name: str) -> Value:
        _check_ui_thread()
        return self._instance.get(name)

    def string_model(self, name: str) -> StringModel:
        """Return the live model of a string-list property."""
        _check_ui_thread()
        return self._instance.string_model(name)

    def record_model(self, name: str) -> RecordModel:
        """Return a schema-checked live model of a writable record list."""
        _check_ui_thread()
        return self._instance.record_model(name)

    def set(self, name: str, value: Value) -> None:
        _check_ui_thread()
        self._instance.set(name, value)

    def reset(self, name: str) -> None:
        """Remove a host override and restore the XRack property binding."""
        _check_ui_thread()
        self._instance.reset(name)

    def invoke(self, name: str, *arguments: Value) -> Value | None:
        _check_ui_thread()
        return self._instance.invoke(name, arguments)

    async def invoke_async(self, name: str, *arguments: Value) -> Value | None:
        """Invoke a callback on the UI thread and return its immediate XRack result."""
        return await self.call_on_ui(self.invoke, name, *arguments)

    def on(self, name: str, handler: Handler) -> None:
        """Register a Python callable. A coroutine handler requires a void callback."""
        _check_ui_thread()
        if not callable(handler):
            raise TypeError("XRack callback handler must be callable")
        returns_value = self._instance.callback_returns_value(name)
        if returns_value and inspect.iscoroutinefunction(handler):
            raise TypeError("Async handlers require a void XRack callback")

        def dispatch(*args: Value) -> Value | None:
            try:
                result = handler(*args)
            except Exception as error:
                if self._runtime is not None:
                    self._report_callback_error(name, error)
                raise
            if not inspect.isawaitable(result):
                return result
            if returns_value or self._runtime is None:
                if inspect.iscoroutine(result):
                    result.close()
                if returns_value:
                    raise TypeError("Async handlers require a void XRack callback")
                raise RuntimeError("Run the XRack window before invoking async handlers")
            task = self._runtime.submit(result)

            def completed(done: Future[object]) -> None:
                if not done.cancelled() and (error := done.exception()) is not None:
                    self._report_callback_error(name, error)

            task.add_done_callback(completed)
            return None

        # Keeping ownership in Python lets GC see cycles through user callbacks.
        self._instance.on(name, weakref.ref(dispatch))
        self._handlers[name] = dispatch

    def on_request(
        self,
        name: str,
        handler: Callable[_P, Awaitable[_T]],
        apply: Callable[[_T], None],
        *,
        on_start: Callable[_P, None] | None = None,
        on_error: Callable[[Exception], None] | None = None,
    ) -> None:
        """Handle a void XRack callback and apply only the latest request's result.

        The handler runs on the asyncio loop. The hooks and apply run on the UI thread.
        """
        _check_ui_thread()
        if self._instance.callback_returns_value(name):
            raise TypeError("Async requests require a void XRack callback")
        latest: object | None = None

        def start(*args: _P.args, **kwargs: _P.kwargs) -> Awaitable[None]:
            nonlocal latest
            runtime = self._runtime
            if runtime is None:
                raise RuntimeError("Run the XRack window before invoking async requests")
            registration = self._handlers[name]
            request = latest = object()
            if on_start is not None:
                on_start(*args, **kwargs)

            def is_current() -> bool:
                return (
                    request is latest
                    and self._handlers.get(name) is registration
                    and self._runtime is runtime
                )

            def publish(value: _T) -> None:
                if is_current():
                    apply(value)

            def report(error: Exception) -> None:
                if not is_current():
                    return
                if on_error is None:
                    raise error
                on_error(error)

            async def resolve() -> None:
                try:
                    result = await handler(*args, **kwargs)
                except Exception as error:
                    await self.call_on_ui(report, error)
                else:
                    await self.call_on_ui(publish, result)

            return resolve()

        self.on(name, start)

    def _report_callback_error(self, name: str, error: BaseException) -> None:
        runtime = self._runtime
        if runtime is None:
            return
        loop = runtime.loop
        loop.call_soon_threadsafe(
            loop.call_exception_handler,
            {"message": f"Exception in XRack callback {name!r}", "exception": error},
        )

    def close(self) -> None:
        """Request that the native window close."""
        _check_ui_thread()
        self._instance.close()

    async def call_on_ui(
        self,
        callback: Callable[_P, _T],
        *args: _P.args,
        **kwargs: _P.kwargs,
    ) -> _T:
        """Call a synchronous function on the UI thread and await its result."""
        if threading.current_thread() is threading.main_thread():
            return callback(*args, **kwargs)
        reply = _native.call_on_ui(partial(callback, *args, **kwargs))
        return await cast(Awaitable[_T], reply)

    def run(self, main: Callable[[], Awaitable[None]] | None = None) -> None:
        """Open the window and run asyncio tasks until the window closes.

        If provided, main is an async function called after UI posting is ready.
        """
        _check_ui_thread()
        if self._runtime is not None:
            raise RuntimeError("The XRack window is already running")
        runtime = _AsyncRuntime()
        self._runtime = runtime
        main_task: Future[None] | None = None

        def ready() -> None:
            nonlocal main_task
            if main is not None:
                main_task = runtime.submit(main())
                main_task.add_done_callback(close_on_main_error)

        def close_on_main_error(done: Future[None]) -> None:
            if not done.cancelled() and done.exception() is not None:
                try:
                    _native.post(self.close)
                except RuntimeError:
                    # The window may have closed while the coroutine was failing.
                    pass

        try:
            self._instance.run(ready)
        finally:
            runtime.shutdown()
            self._runtime = None
        if main_task is not None and not main_task.cancelled():
            main_task.result()
