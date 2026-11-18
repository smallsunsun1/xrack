"""Exercise UIKit hardware presses on an explicitly selected iOS Simulator.

Uses an existing fb-idb installation with modifier-key injection support. The IPA
is supplied separately because the application and this driver target different
platforms. No input is sent to the developer's active desktop application.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import plistlib
import subprocess
import tempfile
import time
from collections.abc import Callable
from pathlib import Path
from typing import Any


class Simulator:
    def __init__(self, args: argparse.Namespace) -> None:
        self.udid: str = args.udid
        self.python: str = args.idb_python
        self.bundle: str = ""
        self.env: dict[str, str] = dict(os.environ)
        self.env["IDB_UDID"] = self.udid
        self.env["PATH"] = f"{args.idb_companion.parent}:{self.env.get('PATH', '')}"
        if args.idb_site_packages:
            self.env["PYTHONPATH"] = str(args.idb_site_packages)

    def call(self, *parts: str, check: bool = True) -> str:
        result: subprocess.CompletedProcess[str] = subprocess.run(
            parts,
            env=self.env,
            check=check,
            capture_output=True,
            text=True,
            timeout=45,
        )
        return result.stdout.strip()

    def simctl(self, *parts: str, check: bool = True) -> str:
        return self.call("xcrun", "simctl", *parts, check=check)

    def idb(self, *parts: str) -> str:
        return self.call(self.python, "-m", "idb.cli.main", *parts)

    def ui(self, *parts: str) -> str:
        return self.idb(
            "ui", *parts, "--reason", "Verify XRack native hardware keyboard input"
        )

    def tree(self) -> list[dict[str, Any]]:
        return json.loads(self.ui("describe-all"))

    def wait(
        self,
        description: str,
        predicate: Callable[[list[dict[str, Any]]], bool],
        timeout: float = 12,
    ) -> list[dict[str, Any]]:
        deadline: float = time.monotonic() + timeout
        rows: list[dict[str, Any]] = []
        while time.monotonic() < deadline:
            rows = self.tree()
            if predicate(rows):
                return rows
            time.sleep(0.15)
        raise AssertionError(f"Timed out waiting for {description}: {rows!r}")

    def has(self, rows: list[dict[str, Any]], label: str) -> bool:
        return any(row.get("AXLabel") == label for row in rows)

    def label(self, label: str, *, present: bool = True) -> None:
        self.wait(
            f"{label!r} present={present}",
            lambda rows: self.has(rows, label) == present,
        )

    def value(self, label: str, value: str) -> None:
        self.wait(
            f"{label!r} value={value!r}",
            lambda rows: any(
                row.get("AXLabel") == label and row.get("AXValue") == value
                for row in rows
            ),
        )

    def keyboard(self, visible: bool) -> None:
        self.wait(
            f"software keyboard visible={visible}",
            lambda rows: (
                any("KeyboardKey" in row.get("traits", []) for row in rows) == visible
            ),
        )

    def frame(self, label: str) -> dict[str, float]:
        rows: list[dict[str, Any]] = self.wait(
            label, lambda rows: self.has(rows, label)
        )
        return next(row["frame"] for row in rows if row.get("AXLabel") == label)

    def tap(self, label: str, *, right_inset: float | None = None) -> None:
        frame: dict[str, float] = self.frame(label)
        x: float = frame["x"] + (
            frame["width"] / 2 if right_inset is None else frame["width"] - right_inset
        )
        self.ui(
            "tap",
            str(round(x)),
            str(round(frame["y"] + frame["height"] / 2)),
            "--duration",
            "0.12",
        )

    def key(self, usage: int, *, command: bool = False) -> None:
        modifiers: tuple[str, ...] = ("--command",) if command else ()
        self.ui("key", str(usage), "--duration", "0.05", *modifiers)

    def launch(self, ready: str = "Check pump pressure", *arguments: str) -> str:
        self.simctl("terminate", self.udid, self.bundle, check=False)
        process: str = self.simctl("launch", self.udid, self.bundle, *arguments)
        self.label(ready)
        return process

    def install(self, ipa: Path, directory: Path) -> None:
        self.simctl("boot", self.udid, check=False)
        self.simctl("bootstatus", self.udid, "-b")
        self.call("unzip", "-q", str(ipa), "-d", str(directory))
        apps: list[Path] = list((directory / "Payload").glob("*.app"))
        if len(apps) != 1:
            raise AssertionError(f"Expected one application in IPA, found {apps!r}")
        app: Path = apps[0]
        with (app / "Info.plist").open("rb") as stream:
            info: dict[str, Any] = plistlib.load(stream)
        self.bundle = info["CFBundleIdentifier"]
        executable: str = info["CFBundleExecutable"]
        # A fresh mtime prevents Simulator's executable patch cache reusing an
        # older binary after a reproducible Bazel bundle is installed again.
        (app / executable).touch()
        (app / "_CodeSignature" / "CodeResources").touch()
        self.simctl("terminate", self.udid, self.bundle, check=False)
        self.simctl("install", self.udid, str(app))
        installed: Path = Path(
            self.simctl("get_app_container", self.udid, self.bundle, "app")
        )
        expected_hash: bytes = hashlib.sha256((app / executable).read_bytes()).digest()
        actual_hash: bytes = hashlib.sha256(
            (installed / executable).read_bytes()
        ).digest()
        if actual_hash != expected_hash:
            raise AssertionError(
                "Simulator did not install the supplied Bazel executable"
            )
        self.idb("connect", self.udid)
        self.ui("rotate", "PORTRAIT")

    def evidence(self) -> None:
        directory: Path = Path(os.environ.get("TEST_UNDECLARED_OUTPUTS_DIR", "."))
        directory.mkdir(parents=True, exist_ok=True)
        (directory / "accessibility.json").write_text(json.dumps(self.tree(), indent=2))
        self.simctl(
            "io", self.udid, "screenshot", str(directory / "hardware-keyboard.png")
        )


def exercise(ui: Simulator) -> None:
    ui.launch()
    ui.tap("New task")
    for usage in (30, 31, 32):  # 1, 2, 3
        ui.key(usage)
    ui.value("New task", "123")
    ui.key(80)  # Left
    ui.key(33)  # 4
    ui.value("New task", "1243")
    ui.key(4, command=True)  # Command+A
    ui.key(38)  # 9
    ui.value("New task", "9")
    ui.key(43)  # Tab from editor to Add button
    ui.value("New task", "9")
    ui.label("3 / 12")
    ui.key(40)  # Return activates the focused control
    ui.value("New task", "")
    ui.label("3 / 13")
    print("PASS: native editing, caret movement, Command+A, Tab and Return activation")

    ui.launch()
    action: str = "Complete Check pump pressure"
    for _ in range(12):
        ui.key(43)  # Tab to row or its nested body button
        ui.key(80)  # Left opens the containing SwipeDelegate
        if ui.has(ui.tree(), action):
            break
    ui.label(action)
    ui.label("3 / 12")
    ui.key(41)  # Escape closes without changing completion state
    ui.label(action, present=False)
    ui.label("3 / 12")
    ui.key(43)  # Next focusable: nested Button
    ui.key(80)
    ui.label(action)
    ui.key(79)  # Right closes from the nested button
    ui.label(action, present=False)
    ui.label("3 / 12")
    print("PASS: ancestor swipe navigation, Escape dismissal and Right dismissal")


def exercise_form_submission(ui: Simulator) -> None:
    for software_done in (False, True):
        ui.launch()
        ui.tap("Open readings")
        ui.tap("Inspection code")
        ui.keyboard(visible=True)

        def submit() -> None:
            if software_done:
                # The number pad's Done button is at the trailing end of its accessory toolbar.
                ui.tap("Toolbar", right_inset=34)
            else:
                ui.key(40)

        ui.key(36)  # 7, too short to submit
        submit()
        ui.value("Inspection code", "7")
        ui.keyboard(visible=True)
        ui.label("Code 7", present=False)
        ui.key(32)  # appends 3 in the same editor
        submit()
        ui.label("Code 73")
        ui.keyboard(visible=False)
        entry: str = "Done" if software_done else "hardware Return"
        print(
            f"PASS: {entry} kept an invalid code editable and dismissed after a valid submission"
        )

    ui.tap("Find instrument")
    ui.keyboard(visible=True)
    # Digits commit directly on both Latin and composing keyboards.
    for usage in (36, 32):  # 73
        ui.key(usage)
    ui.value("Find instrument", "73")
    ui.key(40)
    ui.label("Search: 73")
    ui.keyboard(visible=False)
    print("PASS: hardware Return submitted a search and dismissed its keyboard")

    ui.tap("Find instrument")
    ui.keyboard(visible=True)
    entry: dict[str, float] = ui.frame("Find instrument")
    submit: dict[str, float] = ui.frame("Submit search")
    clear: dict[str, float] = ui.frame("Clear search")
    assert submit["x"] + submit["width"] <= entry["x"] + 0.5
    assert entry["x"] + entry["width"] <= clear["x"] + 0.5
    ui.tap("Clear search")
    ui.value("Find instrument", "")
    ui.label("Clear search", present=False)
    ui.keyboard(visible=True)
    ui.key(29, command=True)  # Command+Z restores the cleared query.
    ui.value("Find instrument", "73")
    ui.key(36)
    ui.value("Find instrument", "737")
    ui.tap("Submit search")
    ui.label("Search: 737")
    print("PASS: UIKit exposes search actions and clearing preserves editable undo")

    ui.tap("Sample count")
    ui.keyboard(visible=True)
    ui.key(4, command=True)
    ui.key(36)  # 7
    ui.value("Sample count", "0")
    ui.key(40)
    ui.value("Sample count", "7")
    ui.keyboard(visible=False)
    print("PASS: hardware Return committed the spin draft and dismissed its keypad")

    ui.tap("Open field notes")
    ui.tap("Inspection notes")
    ui.keyboard(visible=True)
    ui.key(30)
    ui.key(40)  # a multiline editor must insert a newline
    ui.key(31)
    ui.value("Inspection notes", "1\n2")
    ui.keyboard(visible=True)
    print("PASS: hardware Return inserts a newline in Notes and continues editing")


def exercise_landscape_reactivation(ui: Simulator) -> None:
    ui.ui("rotate", "PORTRAIT")
    process: str = ui.launch()
    try:
        ui.tap("New task")
        for usage in (30, 31, 32):  # 123
            ui.key(usage)
        ui.tap("Add")
        ui.label("3 / 13")
        ui.ui("rotate", "LANDSCAPE_LEFT")
        ui.wait(
            "landscape application frame",
            lambda rows: any(
                row.get("type") == "Application"
                and row["frame"]["width"] > row["frame"]["height"]
                for row in rows
            ),
        )
        ui.tap("123")
        ui.label("4 / 13")
        ui.tap("Completed tasks")
        ui.label("Completed checklist")
        ui.ui("button", "HOME")
        ui.label("Completed checklist", present=False)
        assert ui.simctl("launch", ui.udid, ui.bundle) == process
        ui.label("Completed checklist")
        ui.label("4 / 13")
        print(
            "PASS: the same landscape application keeps its task model across Home and reactivation"
        )
    finally:
        ui.ui("rotate", "PORTRAIT")


def exercise_floating_keyboard(ui: Simulator) -> None:
    # iPad mini (A17 Pro), portrait, with a QWERTY keyboard.
    ui.launch()
    ui.tap("Open readings")
    field: str = "Find instrument"
    ui.tap(field)
    ui.keyboard(visible=True)
    if ui.frame("q")["width"] >= 50:
        hide: dict[str, float] = ui.frame("Hide keyboard")
        x: int = round(hide["x"] + hide["width"] / 2)
        y: int = round(hide["y"] + hide["height"] / 2)
        # Slow movement opens the keyboard mode menu and releases on Floating.
        ui.ui(
            "swipe",
            str(x),
            str(y),
            str(x),
            str(y - 62),
            "--duration",
            "5",
            "--delta",
            "1",
        )

    def move_keyboard(x: int, y: int) -> None:
        key: dict[str, float] = ui.frame("q")
        assert key["width"] < 50, "keyboard must be floating"
        # The drag handle is below the keys and centered relative to q.
        ui.ui(
            "swipe",
            str(round(key["x"] + 160)),
            str(round(key["y"] + 220)),
            str(x),
            str(y),
            "--duration",
            "0.7",
            "--delta",
            "8",
        )

    move_keyboard(535, 1040)
    ui.key(4, command=True)
    ui.key(36)
    ui.value(field, "7")
    original: dict[str, float] = ui.frame(field)
    header: dict[str, float] = ui.frame("Open field notes")
    move_keyboard(350, 500)
    ui.wait(
        "editor to clear the floating keyboard",
        lambda rows: any(
            row.get("AXLabel") == field
            and row["frame"]["y"] < original["y"] - 20
            and row["frame"]["height"] >= 37.5
            for row in rows
        ),
    )
    assert ui.frame("Open field notes") == header
    ui.key(32)
    ui.value(field, "73")
    move_keyboard(535, 1040)
    ui.wait(
        "editor to return to the original position",
        lambda rows: any(
            row.get("AXLabel") == field and row["frame"] == original for row in rows
        ),
    )
    assert ui.frame("Open field notes") == header
    ui.value(field, "73")
    print(
        "PASS: dragging the floating keyboard reveals and restores the focused editor"
    )


def exercise_split_demo(ui: Simulator) -> None:
    ui.launch("保存布局")

    def is_divider(row: dict[str, Any]) -> bool:
        return row.get("AXLabel") == "导航" and row.get("AXValue") is not None

    def divider() -> dict[str, Any]:
        rows: list[dict[str, Any]] = ui.wait(
            "navigation divider", lambda rows: any(is_divider(row) for row in rows)
        )
        return next(row for row in rows if is_divider(row))

    handle: dict[str, Any] = divider()
    assert "Adjustable" in handle["traits"], handle
    initial: float = float(handle["AXValue"])

    def expect_value(expected: float) -> None:
        ui.wait(
            f"navigation divider value {expected}",
            lambda rows: any(
                is_divider(row) and abs(float(row["AXValue"]) - expected) < 1.0
                for row in rows
            ),
        )

    def drag(delta: int) -> None:
        frame: dict[str, float] = divider()["frame"]
        center: float = frame["x"] + frame["width"] / 2
        # The demo sets a 44-point target around the 8-point divider.
        start: int = round(center + (-16 if delta > 0 else 16))
        y: int = round(frame["y"] + min(80.0, frame["height"] / 2))
        assert abs(start - center) > frame["width"] / 2
        ui.ui(
            "swipe",
            str(start),
            str(y),
            str(start + delta),
            str(y),
            "--duration",
            "0.35",
            "--delta",
            "4",
        )

    drag(48)
    expect_value(initial + 48)
    ui.key(79)  # Right
    expect_value(initial + 56)
    ui.tap("保存布局")
    before_mirror: dict[str, float] = divider()["frame"]
    ui.tap("LTR · 切换方向")
    ui.label("RTL · 切换方向")
    assert divider()["frame"]["x"] > before_mirror["x"]
    expect_value(initial + 56)
    drag(-32)
    expect_value(initial + 88)
    ui.tap("恢复布局")
    expect_value(initial + 56)
    print(
        "PASS: UIKit exposes an adjustable divider, and touch, keyboard, RTL and restore update the same value"
    )


def exercise_range_demo(ui: Simulator) -> None:
    joined: str = "Range 50 - 50"
    for delta, expected in [(64, "Range 50 - 70"), (-64, "Range 30 - 50")]:
        ui.launch(joined)
        handles: list[dict[str, Any]] = ui.wait(
            "both range endpoints",
            lambda rows: all(
                ui.has(rows, label) for label in ("Lower value", "Upper value")
            ),
        )
        for label in ("Lower value", "Upper value"):
            handle: dict[str, Any] = next(
                row for row in handles if row.get("AXLabel") == label
            )
            assert "Adjustable" in handle["traits"], handle
            assert float(handle["AXValue"]) == 50.0

        thumb: dict[str, float] = ui.frame("Lower value")
        x: int = round(thumb["x"] + thumb["width"] / 2)
        y: int = round(thumb["y"] + thumb["height"] / 2)
        ui.ui(
            "swipe",
            str(x),
            str(y),
            str(x + delta),
            str(y),
            "--duration",
            "0.50",
            "--delta",
            "4",
        )
        ui.label(expected)
        lower, upper = expected.removeprefix("Range ").split(" - ")
        ui.value("Lower value", lower)
        ui.value("Upper value", upper)
    print(
        "PASS: coincident range thumbs separate in either direction and expose independent UIKit values"
    )


def exercise_path_view(ui: Simulator) -> None:
    label: str = "Route cards"

    def expect(value: str, code: int, commits: int, *, enabled: bool = True) -> None:
        ui.label(f"Card code {code}")
        ui.label(f"Card commits {commits}")
        rows: list[dict[str, Any]] = ui.wait(
            f"{label} value={value!r} enabled={enabled}",
            lambda rows: any(
                row.get("AXLabel") == label
                and row.get("AXValue") == value
                and row.get("enabled") == enabled
                for row in rows
            ),
        )
        picker: dict[str, Any] = next(
            row for row in rows if row.get("AXLabel") == label
        )
        assert "Adjustable" in picker["traits"], picker
        assert all(
            row.get("AXLabel") not in {"Port", "Buoy", "Reef", "Pier", "Dock"}
            for row in rows
        )

    def drag(delta: int) -> None:
        frame: dict[str, float] = ui.frame(label)
        x: int = round(frame["x"] + frame["width"] / 2)
        y: int = round(frame["y"] + frame["height"] / 2)
        ui.ui(
            "swipe",
            str(x),
            str(y),
            str(x + delta),
            str(y),
            "--duration",
            "0.45",
            "--delta",
            "4",
        )

    ui.launch("Open wheels")
    ui.tap("Open wheels")
    expect("Buoy", 20, 0)
    drag(-80)
    expect("Reef", 30, 1)
    ui.tap("Lock wheels")
    expect("Reef", 30, 1, enabled=False)
    drag(-80)
    expect("Reef", 30, 1, enabled=False)
    ui.tap("Unlock wheels")
    ui.tap("Mirror cards")
    drag(80)
    expect("Pier", 40, 2)
    print("PASS: UIKit exposes the selected PathView card as one adjustable element")


def exercise_scroll_bars(ui: Simulator) -> None:
    def expect(label: str, value: float) -> None:
        def matches(row: dict[str, Any]) -> bool:
            return (
                row.get("AXLabel") == label
                and row.get("AXValue") is not None
                and abs(float(row["AXValue"]) - value) < 0.001
            )

        rows: list[dict[str, Any]] = ui.wait(
            f"{label}={value}", lambda rows: any(matches(row) for row in rows)
        )
        bar: dict[str, Any] = next(row for row in rows if matches(row))
        assert "Adjustable" in bar["traits"], bar

    def drag_to_end(label: str, thumb: float, *, horizontal: bool = False) -> None:
        frame: dict[str, float] = ui.frame(label)
        x: float = frame["x"] + frame["width"] * (thumb / 2 if horizontal else 0.5)
        y: float = frame["y"] + frame["height"] * (0.5 if horizontal else thumb / 2)
        delta: float = frame["width" if horizontal else "height"] * (1 - thumb)
        coordinates: tuple[float, ...] = (
            x,
            y,
            x + (delta + 2 if horizontal else 0),
            y + (0 if horizontal else delta + 2),
        )
        ui.ui(
            "swipe",
            *(str(round(value)) for value in coordinates),
            "--duration",
            "0.50",
            "--delta",
            "4",
        )

    standalone: str = "Canvas position"
    ui.launch(standalone, "--standalone-scroll")
    expect(standalone, 0)
    drag_to_end(standalone, 0.4)
    expect(standalone, 0.75)
    ui.key(82)  # Up
    expect(standalone, 0.625)
    ui.key(81)  # Down
    expect(standalone, 0.75)

    vertical: str = "Canvas vertical scroll"
    horizontal: str = "Canvas horizontal scroll"
    ui.launch(vertical, "--scroll-bars")
    expect(vertical, 0)
    expect(horizontal, 0)
    drag_to_end(vertical, 1 / 3)
    expect(vertical, 392)
    expect(horizontal, 0)
    ui.key(82)  # Up
    expect(vertical, 215.6)
    ui.value(vertical, "215.6")
    drag_to_end(horizontal, 0.5, horizontal=True)
    expect(horizontal, 260)
    expect(vertical, 215.6)
    ui.key(80)  # Left
    expect(horizontal, 232)
    print("PASS: UIKit exposes standalone and attached scroll bars as adjustable")


def exercise_native_actions(ui: Simulator) -> None:
    ui.launch("Review options", "--native-actions")
    expected: dict[str, set[str]] = {
        "Review options": {"Show context menu"},
        "1": {"Move row down"},
        "2": {"Move row up", "Move row down"},
        "3": {"Move row up"},
    }

    def actions_visible(rows: list[dict[str, Any]]) -> bool:
        actions = {
            row.get("AXLabel"): set(row.get("custom_actions", []))
            for row in rows
            if row.get("custom_actions")
        }
        return all(actions.get(label) == names for label, names in expected.items())

    rows: list[dict[str, Any]] = ui.wait("context and row actions", actions_visible)
    for label in ("Instrument", "Location", "Gauge", "Valve", "Sensor"):
        assert ui.has(rows, label), label

    def expect_sort(value: str, hint: str, order: tuple[str, ...]) -> None:
        def matches(rows: list[dict[str, Any]]) -> bool:
            return any(
                row.get("AXLabel") == "Instrument"
                and row.get("AXValue") == value
                and row.get("help") == hint
                and "Button" in row.get("traits", [])
                for row in rows
            )

        current: list[dict[str, Any]] = ui.wait(f"Instrument {value}", matches)
        positions = {
            row["AXLabel"]: row["frame"]["y"]
            for row in current
            if row.get("AXLabel") in order
        }
        assert tuple(sorted(positions, key=positions.__getitem__)) == order
        if value != "Unsorted":
            assert all(
                not any(name.startswith("Move row ") for name in row["custom_actions"])
                for row in current
            )

    expect_sort("Unsorted", "Sort ascending", ("Gauge", "Valve", "Sensor"))
    ui.tap("Instrument")
    expect_sort("Sorted ascending", "Sort descending", ("Gauge", "Sensor", "Valve"))
    ui.tap("Instrument")
    expect_sort("Sorted descending", "Sort ascending", ("Valve", "Sensor", "Gauge"))
    ui.tap("Clear sort")
    expect_sort("Unsorted", "Sort ascending", ("Gauge", "Valve", "Sensor"))
    ui.wait("row actions restored after clearing the sort", actions_visible)
    print(
        "PASS: UIKit exposes context actions, row moves and the current table sort order"
    )

    def cell(label: str) -> dict[str, Any]:
        current: list[dict[str, Any]] = ui.wait(label, lambda rows: ui.has(rows, label))
        return next(row for row in current if row.get("AXLabel") == label)

    location: dict[str, Any] = cell("Pump room")
    assert "Edit cell" in location["custom_actions"]
    identity: str = location["AXUniqueId"]
    assert identity
    ui.tap("Pump room")
    ui.keyboard(visible=True)
    editor: str = "Location editor"
    ui.value(editor, "Pump room")
    assert cell(editor)["AXUniqueId"] == identity
    assert set(cell(editor)["custom_actions"]) >= {"Save edit", "Cancel edit"}
    ui.key(30)
    ui.key(31)
    ui.value(editor, "12")
    ui.key(40)
    ui.keyboard(visible=False)
    assert cell("12")["AXUniqueId"] == identity

    ui.tap("12")
    ui.key(38)
    ui.value(editor, "9")
    ui.key(41)
    ui.keyboard(visible=False)
    assert cell("12")["AXUniqueId"] == identity
    ui.label(editor, present=False)
    print(
        "PASS: native keyboard edits, commits and cancels the same accessible table cell"
    )

    branch: str = "Pump area"
    ui.value(branch, "Collapsed")
    parent: dict[str, Any] = cell(branch)
    branch_id: str = parent["AXUniqueId"]
    assert "Expand" in parent["custom_actions"]
    ui.label("Intake valve", present=False)
    bounds: dict[str, float] = parent["frame"]
    x: int = round(bounds["x"] + 15)
    y: int = round(bounds["y"] + bounds["height"] / 2)
    ui.ui("tap", str(x), str(y), "--duration", "0.12")
    ui.value(branch, "Expanded")
    assert cell(branch)["AXUniqueId"] == branch_id
    assert "Collapse" in cell(branch)["custom_actions"]
    ui.label("Intake valve")
    ui.label("Return valve")
    assert "Edit cell" in cell("Service note")["custom_actions"]
    ui.ui("tap", str(x), str(y), "--duration", "0.12")
    ui.value(branch, "Collapsed")
    ui.label("Intake valve", present=False)
    ui.label("Return valve", present=False)
    assert cell(branch)["AXUniqueId"] == branch_id
    print(
        "PASS: UIKit exposes the same tree branch, its expanded children and their editor"
    )

    def review_option(label: str) -> None:
        trigger: dict[str, float] = ui.frame("Review options")
        ui.ui(
            "tap",
            str(round(trigger["x"] + trigger["width"] / 2)),
            str(round(trigger["y"] + trigger["height"] / 2)),
            "--duration",
            "0.7",
        )
        ui.tap(label)

    review_option("Manage tabs")
    ui.label("Draft content")
    assert "Selected" in cell("Draft")["traits"]
    assert "Button" in cell("Close Results")["traits"]
    ui.tap("Close Results")
    ui.label("Results", present=False)
    ui.label("Draft content")
    assert "Selected" in cell("Draft")["traits"]
    ui.tap("Close Draft")
    ui.label("Pinned content")
    assert "Selected" in cell("Pinned")["traits"]
    ui.label("Draft", present=False)
    ui.label("Close Pinned", present=False)
    print(
        "PASS: UIKit exposes tab close buttons and tracks the selected page after closing"
    )

    ui.key(41)
    review_option("Browse narrow tabs")
    ui.label("Close Draft")
    ui.label("Archive", present=False)
    strip: dict[str, float] = ui.frame("Scrollable tab strip")
    center_y: str = str(round(strip["y"] + strip["height"] / 2))
    ui.ui(
        "swipe",
        str(round(strip["x"] + strip["width"] - 8)),
        center_y,
        str(round(strip["x"] + 8)),
        center_y,
        "--duration",
        "0.45",
        "--delta",
        "4",
    )
    ui.label("Archive")
    ui.label("Current tab: overview")
    print("PASS: the narrow tab strip scrolls independently of the selected page")

    ui.key(41)
    review_option("Disclosure controls")
    ui.value("Advanced options", "Collapsed")
    header = cell("Advanced options")
    assert "Expand" in header["custom_actions"]
    assert "Button" in header["traits"]
    ui.label("Network details", present=False)
    ui.tap("Advanced options")
    ui.value("Advanced options", "Expanded")
    assert "Collapse" in cell("Advanced options")["custom_actions"]
    ui.tap("Network details")
    ui.label("Checks 1")
    ui.tap("Advanced options")
    ui.label("Network details", present=False)

    def settings_enabled(rows: list[dict[str, Any]], enabled: bool) -> bool:
        return any(
            row.get("AXLabel") == "Test settings" and row.get("enabled") == enabled
            for row in rows
        )

    ui.tap("Optional settings")
    ui.wait("disabled settings button", lambda rows: settings_enabled(rows, False))
    ui.tap("Test settings")
    ui.label("Checks 1")
    ui.tap("Optional settings")
    ui.wait("enabled settings button", lambda rows: settings_enabled(rows, True))
    ui.tap("Test settings")
    ui.label("Checks 2")
    print("PASS: system accessibility exposes disclosure state and each usable child")

    ui.key(41)
    review_option("Open tools drawer")
    ui.label("Check instrument")
    ui.label("Review options", present=False)
    drawer: dict[str, float] = ui.frame("Inspection tools")
    ui.key(41)
    ui.label("Tool closures 1")
    ui.label("Review options")

    x: int = round(drawer["x"] + drawer["width"] / 2)
    # The title anchors the demo's 214-point drawer at its safe-area bottom.
    bottom: int = round(drawer["y"] + 214) - 6
    ui.ui(
        "swipe",
        str(x),
        str(bottom),
        str(x),
        str(bottom - 124),
        "--duration",
        "0.45",
        "--delta",
        "4",
    )
    ui.label("Tool gestures 1")
    ui.label("Check instrument")
    ui.label("Review options", present=False)
    top: int = round(drawer["y"]) + 7
    ui.ui(
        "swipe",
        str(x),
        str(top),
        str(x),
        str(top + 132),
        "--duration",
        "0.45",
        "--delta",
        "4",
    )
    ui.label("Tool closures 2")
    ui.label("Review options")
    ui.label("Check instrument", present=False)
    print("PASS: a bottom drawer opens at the safe edge and closes from its inner edge")


def main() -> None:
    parser: argparse.ArgumentParser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ipa", type=Path, required=True)
    parser.add_argument("--udid", required=True)
    parser.add_argument("--idb-python", required=True)
    parser.add_argument("--idb-companion", type=Path, required=True)
    parser.add_argument("--idb-site-packages", type=Path)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument(
        "--floating-keyboard",
        action="store_true",
        help="Exercise the iPad mini's floating keyboard",
    )
    mode.add_argument(
        "--split-demo", action="store_true", help="Exercise the XRack Split iPad app"
    )
    mode.add_argument(
        "--range-demo",
        action="store_true",
        help="Exercise the XRack Controls range slider",
    )
    mode.add_argument(
        "--path-view-demo",
        action="store_true",
        help="Exercise the XRack Controls PathView picker",
    )
    mode.add_argument(
        "--scroll-bar-demo",
        action="store_true",
        help="Exercise the standalone and attached scroll bars in XRack Controls",
    )
    mode.add_argument(
        "--native-actions-demo",
        action="store_true",
        help="Inspect the system accessibility tree of the Inspection queue demo",
    )
    args: argparse.Namespace = parser.parse_args()
    ui: Simulator = Simulator(args)
    with tempfile.TemporaryDirectory(prefix="xrack-ios-keyboard-") as temporary:
        ui.install(args.ipa.resolve(), Path(temporary))
    try:
        if args.range_demo:
            exercise_range_demo(ui)
        elif args.path_view_demo:
            exercise_path_view(ui)
        elif args.scroll_bar_demo:
            exercise_scroll_bars(ui)
        elif args.native_actions_demo:
            exercise_native_actions(ui)
        elif args.split_demo:
            exercise_split_demo(ui)
        elif args.floating_keyboard:
            exercise_floating_keyboard(ui)
        else:
            exercise(ui)
            exercise_form_submission(ui)
            exercise_landscape_reactivation(ui)
        ui.evidence()
    except Exception:
        ui.evidence()
        raise
    finally:
        ready: str = (
            "Range 50 - 50"
            if args.range_demo or args.scroll_bar_demo or args.native_actions_demo
            else "Open wheels"
            if args.path_view_demo
            else "保存布局"
            if args.split_demo
            else "Check pump pressure"
        )
        ui.launch(ready)


if __name__ == "__main__":
    main()
