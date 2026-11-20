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
from concurrent.futures import ThreadPoolExecutor
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


def exercise_landscape_contact_form(ui: Simulator, *, soft_actions: bool) -> None:
    def keyboard_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
        return [row for row in rows if "KeyboardKey" in row.get("traits", [])]

    def app_width(rows: list[dict[str, Any]]) -> float:
        return next(
            row["frame"]["width"] for row in rows if row.get("type") == "Application"
        )

    def tap_key(label: str) -> None:
        def matches(row: dict[str, Any]) -> bool:
            return str(row.get("AXLabel")).casefold() == label

        rows = ui.wait(
            f"landscape key {label}",
            lambda rows: any(map(matches, keyboard_rows(rows))),
        )
        frame: dict[str, float] = next(
            row["frame"] for row in keyboard_rows(rows) if matches(row)
        )
        # fb-idb reports system keyboard keys in device coordinates even when
        # the app and the coordinate touch API follow the Landscape Left rotation.
        x: float = app_width(rows) - frame["y"] - frame["height"] / 2
        y: float = frame["x"] + frame["width"] / 2
        ui.ui("tap", str(round(x)), str(round(y)), "--duration", "0.12")

    def keypad_next_frame() -> dict[str, float]:
        rows = ui.wait(
            "telephone keypad", lambda rows: ui.has(keyboard_rows(rows), "1")
        )
        keys: list[dict[str, float]] = [row["frame"] for row in keyboard_rows(rows)]
        # The accessory is 44 points high and its Next button sits above the
        # keypad's right edge. Resolve the actual button before touching it.
        x: float = app_width(rows) - min(frame["y"] for frame in keys) - 22
        y: float = min(frame["x"] for frame in keys) - 22
        point: dict[str, Any] = json.loads(
            ui.ui("describe-point", str(round(x)), str(round(y)))
        )
        assert point.get("AXLabel") == "Next" and point.get("enabled"), point
        return point["frame"]

    def field_above_keyboard(
        rows: list[dict[str, Any]], label: str, toolbar_top: float | None = None
    ) -> bool:
        keyboard_top: float = min(
            (row["frame"]["x"] for row in keyboard_rows(rows)), default=0.0
        )
        visible_bottom: float = (
            keyboard_top if toolbar_top is None else min(keyboard_top, toolbar_top)
        )
        return any(
            row.get("AXLabel") == label
            and row["frame"]["height"] >= 37.5
            and row["frame"]["y"] + row["frame"]["height"] <= visible_bottom + 1.0
            for row in rows
        )

    ui.ui("rotate", "PORTRAIT")
    ui.launch()
    try:
        ui.tap("Open readings")
        ui.tap("Contact details")
        ui.tap("Contact email")
        ui.key(4)  # A
        ui.value("Contact email", "a")
        ui.key(5)  # B
        ui.value("Contact email", "ab")
        ui.ui("rotate", "LANDSCAPE_LEFT")
        ui.wait(
            "landscape contact form",
            lambda rows: any(
                row.get("type") == "Application"
                and row["frame"]["width"] > row["frame"]["height"]
                for row in rows
            ),
        )
        ui.wait(
            "email above the keyboard",
            lambda rows: field_above_keyboard(rows, "Contact email"),
        )
        ui.value("Contact email", "ab")
        if soft_actions:
            tap_key("next")
        else:
            ui.key(40)  # Submit and focus the telephone field.

        next_frame: dict[str, float] = keypad_next_frame()
        x: str = str(round(next_frame["x"] + next_frame["width"] / 2))
        y: str = str(round(next_frame["y"] + next_frame["height"] / 2))
        ui.wait(
            "telephone above the keyboard toolbar",
            lambda rows: field_above_keyboard(
                rows, "Contact telephone", next_frame["y"]
            ),
        )
        for usage, draft in [(30, "1"), (31, "12"), (32, "123")]:
            if soft_actions:
                tap_key(draft[-1])
            else:
                ui.key(usage)
            ui.value("Contact telephone", draft)
            if soft_actions and draft == "1":
                # An incomplete phone number must keep the keypad and focus.
                ui.ui("tap", x, y, "--duration", "0.12")
        if soft_actions:
            ui.ui("tap", x, y, "--duration", "0.12")
            tap_key("/")
        else:
            ui.key(40)
            ui.key(56)  # /
        ui.value("Contact website", "/")
        ui.wait(
            "website above the keyboard",
            lambda rows: field_above_keyboard(rows, "Contact website"),
        )
        if soft_actions:
            tap_key("done")
        else:
            ui.key(40)
        ui.label("Website: /")
        ui.keyboard(False)
        ui.value("Contact email", "ab")
        ui.value("Contact telephone", "123")
        ui.tap("Close contact details")
        ui.label("Contact details")
        action: str = "soft Next and Done" if soft_actions else "hardware Return"
        print(f"PASS: landscape contact form with {action}")
    finally:
        ui.ui("rotate", "PORTRAIT")


def float_software_keyboard(ui: Simulator) -> None:
    ui.keyboard(visible=True)
    rows = ui.wait(
        "QWERTY keyboard",
        lambda rows: any(str(row.get("AXLabel")).casefold() == "q" for row in rows),
    )
    q_frame: dict[str, float] = next(
        row["frame"] for row in rows if str(row.get("AXLabel")).casefold() == "q"
    )
    if q_frame["width"] >= 50:
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
    ui.label("Floating keyboard grabber")


def move_floating_keyboard(ui: Simulator, x: float, y: float) -> None:
    grabber = ui.frame("Floating keyboard grabber")
    ui.ui(
        "swipe",
        str(round(grabber["x"] + grabber["width"] / 2)),
        str(round(grabber["y"] + grabber["height"] / 2)),
        str(round(x)),
        str(round(y)),
        "--duration",
        "0.7",
        "--delta",
        "8",
    )


def exercise_floating_keyboard(ui: Simulator) -> None:
    # iPad mini (A17 Pro), portrait, with an English QWERTY keyboard.
    ui.launch()
    ui.tap("Open readings")
    field: str = "Find instrument"
    ui.tap(field)
    float_software_keyboard(ui)
    move_floating_keyboard(ui, 535, 1040)
    ui.key(4, command=True)
    ui.key(36)
    ui.value(field, "7")
    original: dict[str, float] = ui.frame(field)
    header: dict[str, float] = ui.frame("Open field notes")
    move_floating_keyboard(ui, 350, 500)
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
    move_floating_keyboard(ui, 535, 1040)
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


def open_review_menu(ui: Simulator) -> None:
    menu = ui.frame("Review options")
    ui.ui(
        "tap",
        str(round(menu["x"] + menu["width"] / 2)),
        str(round(menu["y"] + menu["height"] / 2)),
        "--duration",
        "0.8",
    )


def exercise_edge_back(ui: Simulator) -> None:
    ui.launch("Review options", "--native-actions")
    open_review_menu(ui)
    ui.tap("Test accessible navigation")
    ui.tap("Open inspection detail")
    ui.label("Page detail")

    def swipe(start: int, end: int) -> None:
        # Start near mid-height, clear of system status and home gestures.
        ui.ui(
            "swipe",
            str(start),
            "424",
            str(end),
            "424",
            "--duration",
            "0.5",
            "--delta",
            "5",
        )

    swipe(60, 220)
    ui.label("Page detail")
    swipe(3, 38)
    ui.label("Page detail")
    ui.tap("Lock return")
    swipe(3, 180)
    ui.label("Blocked returns 1")
    ui.label("Page detail")
    ui.tap("Open review overlay")
    swipe(3, 180)
    ui.label("Dismiss review overlay")
    ui.tap("Dismiss review overlay")
    ui.label("Blocked returns 1")
    ui.tap("Unlock return")

    ui.tap("Open review overlay")
    swipe(3, 180)
    ui.label("Dismiss review overlay", present=False)
    ui.label("Page detail")
    swipe(3, 180)
    ui.label("Page summary")
    swipe(3, 180)
    ui.label("Page summary")
    ui.label("Blocked returns 1")
    print("PASS: iOS edge swipe honors the active overlay and page leave guard")


def exercise_drawer_preview(ui: Simulator) -> None:
    ui.launch("Review options", "--native-actions")
    open_review_menu(ui)
    ui.tap("Open tools drawer")
    title: str = "Inspection tools"
    opened: dict[str, float] = ui.frame(title)
    x: str = str(round(opened["x"] + opened["width"] / 2))
    bottom: int = round(opened["y"] + 214) - 6
    grip: int = round(opened["y"]) + 7
    ui.key(41)
    ui.label("Tool closures 1")

    def swipe(start: int, end: int, duration: str = "0.45") -> str:
        return ui.ui(
            "swipe", x, str(start), x, str(end), "--duration", duration, "--delta", "3"
        )

    output: Path = Path(os.environ.get("TEST_UNDECLARED_OUTPUTS_DIR", "."))
    output.mkdir(parents=True, exist_ok=True)
    swipe(bottom, bottom - 30)
    ui.label("Review options")
    ui.label("Check instrument", present=False)
    ui.label("Tool closures 1")

    with ThreadPoolExecutor(max_workers=1) as worker:
        opening = worker.submit(swipe, bottom, bottom - 124, "3.2")
        time.sleep(1.0)
        assert not opening.done(), (
            "opening swipe finished before the preview was sampled"
        )
        ui.simctl("io", ui.udid, "screenshot", str(output / "drawer-opening.png"))
        ui.label("Check instrument", present=False)
        opening.result()
    ui.label("Tool gestures 1")
    ui.label("Check instrument")
    ui.label("Review options", present=False)
    opened = ui.frame(title)
    with ThreadPoolExecutor(max_workers=1) as worker:
        closing = worker.submit(swipe, grip, grip + 30, "2.0")
        ui.wait(
            "drawer title and controls to move before releasing the finger",
            lambda rows: any(
                row.get("AXLabel") == title and row["frame"]["y"] > opened["y"] + 8
                for row in rows
            ),
            timeout=3.0,
        )
        assert not closing.done(), (
            "closing swipe finished before the preview was sampled"
        )
        ui.simctl("io", ui.udid, "screenshot", str(output / "drawer-closing.png"))
        closing.result()
    ui.wait(
        "drawer to return to the open position after a short swipe",
        lambda rows: any(
            row.get("AXLabel") == title and row["frame"] == opened for row in rows
        ),
    )
    ui.label("Tool gestures 1")
    swipe(grip, grip + 124)
    ui.label("Tool closures 2")
    ui.label("Review options")
    ui.label("Check instrument", present=False)
    print("PASS: bottom drawer follows a held swipe and cancels without a callback")


def exercise_tab_dynamic_type(ui: Simulator) -> None:
    setting: tuple[str, ...] = ("ui", ui.udid, "content_size")
    original: str = ui.simctl(*setting)
    try:
        ui.simctl(*setting, "large")
        ui.launch("Review options", "--native-actions")
        open_review_menu(ui)
        ui.tap("Manage tabs")
        ui.label("Draft content")
        compact: float = ui.frame("Draft")["height"]
        ui.simctl(*setting, "accessibility-extra-extra-extra-large")
        rows = ui.wait(
            "the selected tab to grow and keep its own close button",
            lambda rows: (
                any(
                    row.get("AXLabel") == "Draft"
                    and row["frame"]["height"] >= compact + 12
                    and "Selected" in row.get("traits", [])
                    for row in rows
                )
                and any(
                    row.get("AXLabel") == "Close Draft" and row["frame"]["width"] >= 44
                    for row in rows
                )
            ),
        )
        enlarged: float = next(
            row["frame"]["height"] for row in rows if row.get("AXLabel") == "Draft"
        )
        ui.label("Draft content")
        ui.tap("Draft")
        ui.key(79)  # Right
        ui.label("Results content")
        close: dict[str, float] = ui.frame("Close Results")
        assert close["width"] >= 44 and close["height"] >= 44, close
        output: Path = Path(os.environ.get("TEST_UNDECLARED_OUTPUTS_DIR", "."))
        output.mkdir(parents=True, exist_ok=True)
        ui.simctl("io", ui.udid, "screenshot", str(output / "tabs-dynamic-type.png"))
        ui.tap("Close Results")
        ui.label("Pinned content")
        ui.label("Results", present=False)
        print(
            f"PASS: tab headers grew from {compact:g} to {enlarged:g} points, "
            "with selection, keyboard reveal and touch close at maximum Dynamic Type"
        )
    finally:
        ui.simctl(*setting, original)


def exercise_button_shapes(ui: Simulator) -> None:
    label: str = "Button Shapes"

    def open_setting() -> str:
        ui.simctl(
            "openurl",
            ui.udid,
            "App-prefs:root=ACCESSIBILITY&path=DISPLAY_AND_TEXT_SIZE",
        )
        rows: list[dict[str, Any]] = ui.wait(
            "the accessibility setting in Settings",
            lambda rows: (
                ui.has(rows, "Settings")
                and any(
                    ui.has(rows, item)
                    for item in (label, "Display & Text Size", "Accessibility")
                )
            ),
        )
        if not ui.has(rows, label):
            if not ui.has(rows, "Display & Text Size"):
                ui.tap("Accessibility")
            ui.tap("Display & Text Size")
        rows = ui.wait(
            "the Button Shapes switch",
            lambda rows: any(
                row.get("AXLabel") == label and row.get("AXValue") in ("0", "1")
                for row in rows
            ),
        )
        return next(
            row["AXValue"]
            for row in rows
            if row.get("AXLabel") == label and row.get("AXValue") in ("0", "1")
        )

    def set_setting(value: str) -> None:
        if open_setting() != value:
            ui.tap(label, right_inset=40)
            ui.value(label, value)

    original: str = open_setting()
    try:
        set_setting("0")
        process: str = ui.launch("Plain action", "--button-shapes")
        original_frame: dict[str, float] = ui.frame("Plain action")
        output: Path = Path(os.environ.get("TEST_UNDECLARED_OUTPUTS_DIR", "."))
        output.mkdir(parents=True, exist_ok=True)
        ui.simctl("io", ui.udid, "screenshot", str(output / "button-shapes-off.png"))
        set_setting("1")
        resumed: str = ui.simctl("launch", ui.udid, ui.bundle)
        assert resumed == process, (process, resumed)
        assert ui.frame("Plain action") == original_frame
        ui.simctl("io", ui.udid, "screenshot", str(output / "button-shapes-on.png"))
        ui.tap("Plain action")
        ui.label("Actions 1")
        ui.wait(
            "the disabled control to keep its UIKit state",
            lambda rows: any(
                row.get("AXLabel") == "Unavailable action"
                and not row.get("enabled", True)
                for row in rows
            ),
        )
        print(
            "PASS: toggled Button Shapes and captured the same running app with unchanged hit bounds"
        )
    finally:
        set_setting(original)


def exercise_button_dynamic_type(ui: Simulator) -> None:
    setting: tuple[str, ...] = ("ui", ui.udid, "content_size")
    original: str = ui.simctl(*setting)
    try:
        ui.simctl(*setting, "large")
        ui.launch("Review options", "--native-actions")
        heading: str = "Inspection queue"
        standard_heading: float = ui.frame(heading)["height"]
        standard_button: dict[str, float] = ui.frame("Review options")
        ui.simctl(*setting, "accessibility-extra-extra-extra-large")
        ui.wait(
            "system font change to reach the app",
            lambda rows: any(
                row.get("AXLabel") == heading
                and row["frame"]["height"] > standard_heading * 2
                for row in rows
            ),
        )
        button: dict[str, float] = ui.frame("Review options")
        assert button["height"] == standard_button["height"] == 44, button
        output: Path = Path(os.environ.get("TEST_UNDECLARED_OUTPUTS_DIR", "."))
        output.mkdir(parents=True, exist_ok=True)
        ui.simctl("io", ui.udid, "screenshot", str(output / "button-dynamic-type.png"))
        open_review_menu(ui)
        ui.tap("Record review")
        ui.label("Reviews 1")
        print(
            "PASS: the fixed 44-point button keeps its accessible title and works at maximum Dynamic Type"
        )
    finally:
        ui.simctl(*setting, original)


def exercise_group_title_dynamic_type(ui: Simulator) -> None:
    setting: tuple[str, ...] = ("ui", ui.udid, "content_size")
    original: str = ui.simctl(*setting)
    expander: str = "Inspection options"
    group: str = "Network settings"
    child: str = "Check network"

    def tap_lower_line(label: str) -> None:
        bounds: dict[str, float] = ui.frame(label)
        ui.ui(
            "tap",
            str(round(bounds["x"] + bounds["width"] / 2)),
            str(round(bounds["y"] + bounds["height"] - 8)),
            "--duration",
            "0.12",
        )

    def require_child_enabled(enabled: bool) -> None:
        ui.wait(
            f"the group child to be enabled={enabled}",
            lambda rows: any(
                row.get("AXLabel") == child and row.get("enabled") == enabled
                for row in rows
            ),
        )

    try:
        ui.simctl(*setting, "large")
        ui.launch(expander, "--group-titles")
        standard: dict[str, float] = {
            label: ui.frame(label)["height"] for label in (expander, group)
        }
        ui.simctl(*setting, "accessibility-extra-extra-extra-large")
        for label in (expander, group):
            ui.wait(
                f"{label} to span two lines",
                lambda rows: any(
                    row.get("AXLabel") == label
                    and row["frame"]["height"] > standard[label] * 3
                    for row in rows
                ),
            )
        ui.label("Record", present=False)
        tap_lower_line(expander)
        ui.label("Record")
        heading: dict[str, float] = ui.frame(expander)
        assert heading["y"] + heading["height"] <= ui.frame("Record")["y"] + 1
        tap_lower_line(group)
        require_child_enabled(False)
        tap_lower_line(group)
        require_child_enabled(True)
        ui.tap(child)
        ui.label("Network checked")
        output: Path = Path(os.environ.get("TEST_UNDECLARED_OUTPUTS_DIR", "."))
        output.mkdir(parents=True, exist_ok=True)
        ui.simctl(
            "io", ui.udid, "screenshot", str(output / "group-title-dynamic-type.png")
        )
        tap_lower_line(expander)
        ui.label("Record", present=False)
        print(
            "PASS: the disclosure and checkable group expose their full titles "
            "and both title lines accept touch at maximum Dynamic Type"
        )
    finally:
        ui.simctl(*setting, original)


def exercise_modal_title_dynamic_type(ui: Simulator) -> None:
    setting: tuple[str, ...] = ("ui", ui.udid, "content_size")
    original: str = ui.simctl(*setting)
    try:
        ui.simctl(*setting, "large")
        ui.launch("Open calibration", "--modal-titles")
        ui.tap("Open calibration")
        review: str = "Calibration review"
        standard: float = ui.frame(review)["height"]
        ui.simctl(*setting, "accessibility-extra-extra-extra-large")
        ui.wait(
            "the full dialog heading to grow to two lines",
            lambda rows: any(
                row.get("AXLabel") == review and row["frame"]["height"] > standard * 2.8
                for row in rows
            ),
        )
        heading: dict[str, float] = ui.frame(review)
        assert heading["y"] + heading["height"] <= ui.frame("Save review")["y"] + 1
        output: Path = Path(os.environ.get("TEST_UNDECLARED_OUTPUTS_DIR", "."))
        output.mkdir(parents=True, exist_ok=True)
        ui.simctl(
            "io", ui.udid, "screenshot", str(output / "dialog-title-dynamic-type.png")
        )
        ui.tap("Save review")
        ui.label("Review saved")

        ui.tap("Open evidence")
        drawer: dict[str, float] = ui.frame("Required inspection evidence")
        keep: dict[str, float] = ui.frame("Keep evidence")
        assert drawer["height"] < heading["height"]
        assert drawer["y"] + drawer["height"] <= keep["y"] + 1
        assert keep["height"] == 44, keep
        ui.simctl(
            "io", ui.udid, "screenshot", str(output / "drawer-title-dynamic-type.png")
        )
        ui.tap("Keep evidence")
        ui.label("Evidence kept")
        print(
            "PASS: modal headings keep full UIKit labels and leave both actions "
            "reachable at maximum Dynamic Type"
        )
    finally:
        ui.simctl(*setting, original)


def exercise_toggle_dynamic_type(ui: Simulator) -> None:
    setting: tuple[str, ...] = ("ui", ui.udid, "content_size")
    original: str = ui.simctl(*setting)
    try:
        ui.simctl(*setting, "large")
        heading: str = "Inspection toggles"
        check: str = "Include scheduled maintenance"
        alerts: str = "Receive inspection notifications"
        east: str = "الفرع الشرقي للتفتيش الدوري"
        west: str = "الفرع الغربي للتفتيش الدوري"
        blocked: str = "Keep required inspection records"
        ui.launch(heading, "--compact-toggles")
        standard: float = ui.frame(heading)["height"]
        ui.simctl(*setting, "accessibility-extra-extra-extra-large")
        ui.wait(
            "the toggle demo to receive the system text size",
            lambda rows: any(
                row.get("AXLabel") == heading and row["frame"]["height"] > standard * 2
                for row in rows
            ),
        )
        for label in (check, alerts, east, west, blocked):
            assert ui.frame(label)["height"] == 44, label
        ui.value(east, "1")
        ui.value(west, "0")
        ui.tap(check)
        ui.value(check, "1")
        ui.key(43)  # Tab
        ui.key(44)  # Space
        ui.value(alerts, "1")
        ui.tap(west)
        ui.value(west, "1")
        ui.value(east, "0")
        ui.tap(blocked)
        ui.value(blocked, "1")

        output: Path = Path(os.environ.get("TEST_UNDECLARED_OUTPUTS_DIR", "."))
        output.mkdir(parents=True, exist_ok=True)
        ui.simctl("io", ui.udid, "screenshot", str(output / "toggle-dynamic-type.png"))
        print(
            "PASS: fixed-height toggles keep full accessibility labels and accept "
            "touch and hardware keyboard input at maximum Dynamic Type"
        )
    finally:
        ui.simctl(*setting, original)


def exercise_table_dynamic_type(ui: Simulator) -> None:
    setting: tuple[str, ...] = ("ui", ui.udid, "content_size")
    original: str = ui.simctl(*setting)
    try:
        ui.simctl(*setting, "large")
        ui.launch("Review options", "--native-actions")
        heading: str = "Inspection queue"
        standard_heading: float = ui.frame(heading)["height"]
        ui.simctl(*setting, "accessibility-extra-extra-extra-large")
        ui.wait(
            "the table demo to receive the system font change",
            lambda rows: any(
                row.get("AXLabel") == heading
                and row["frame"]["height"] > standard_heading * 2
                for row in rows
            ),
        )
        for label in ("Instrument", "Location", "Gauge", "Pump room", "East riser"):
            ui.label(label)
        output: Path = Path(os.environ.get("TEST_UNDECLARED_OUTPUTS_DIR", "."))
        output.mkdir(parents=True, exist_ok=True)
        ui.simctl("io", ui.udid, "screenshot", str(output / "table-dynamic-type.png"))
        ui.tap("Instrument")
        ui.value("Instrument", "Sorted ascending")
        assert ui.frame("Sensor")["y"] < ui.frame("Valve")["y"]
        ui.tap("Pump room")
        ui.keyboard(visible=True)
        editor: str = "Location editor"
        ui.value(editor, "Pump room")
        ui.key(30)
        ui.key(31)
        ui.value(editor, "12")
        ui.key(40)
        ui.keyboard(visible=False)
        ui.label("12")
        print(
            "PASS: the table keeps its full accessible values, sorts and commits a cell edit at maximum Dynamic Type"
        )
    finally:
        ui.simctl(*setting, original)


def exercise_row_dynamic_type(ui: Simulator) -> None:
    setting: tuple[str, ...] = ("ui", ui.udid, "content_size")
    original: str = ui.simctl(*setting)
    try:
        ui.simctl(*setting, "large")
        heading: str = "Inspection rows"
        ui.launch(heading, "--compact-rows")
        standard: float = ui.frame(heading)["height"]
        ui.simctl(*setting, "accessibility-extra-extra-extra-large")
        ui.wait(
            "the rows demo to receive the system text size",
            lambda rows: any(
                row.get("AXLabel") == heading and row["frame"]["height"] > standard * 2
                for row in rows
            ),
        )
        ui.label("Daily inspections requiring follow-up")
        ui.label("Pump calibration due tomorrow")
        ui.tap("Replace the intake valve filter")
        ui.label("Selected 1")
        branch: str = "Cooling system"
        ui.value(branch, "Collapsed")
        frame: dict[str, float] = ui.frame(branch)
        x: str = str(round(frame["x"] + 15))
        y: str = str(round(frame["y"] + frame["height"] / 2))
        ui.ui("tap", x, y, "--duration", "0.12")
        ui.value(branch, "Expanded")
        note: str = "Verify the pressure calibration"
        ui.label(note)
        output: Path = Path(os.environ.get("TEST_UNDECLARED_OUTPUTS_DIR", "."))
        output.mkdir(parents=True, exist_ok=True)
        ui.simctl("io", ui.udid, "screenshot", str(output / "rows-dynamic-type.png"))
        ui.tap(note)
        ui.key(59)  # F2
        ui.keyboard(visible=True)
        editor: str = "Cell editor"
        ui.value(editor, note)
        ui.key(36)
        ui.value(editor, "7")
        ui.key(40)
        ui.keyboard(visible=False)
        ui.label("7")
        ui.ui("tap", x, y, "--duration", "0.12")
        ui.value(branch, "Collapsed")
        ui.label("Intake valve", present=False)
        print(
            "PASS: the compact list and tree retain full accessibility labels, "
            "selection, disclosure, and editing at maximum Dynamic Type"
        )
    finally:
        ui.simctl(*setting, original)


def exercise_floating_keyboard_choices(ui: Simulator) -> None:
    def keyboard_bounds(
        rows: list[dict[str, Any]],
    ) -> tuple[float, float, float, float]:
        parts: list[dict[str, float]] = [
            row["frame"]
            for row in rows
            if row.get("AXLabel") == "Floating keyboard grabber"
            or {"KeyboardKey", "AutoCorrectCandidate"}.intersection(
                row.get("traits", [])
            )
        ]
        return (
            min(part["x"] for part in parts),
            min(part["y"] for part in parts),
            max(part["x"] + part["width"] for part in parts),
            max(part["y"] + part["height"] for part in parts),
        )

    def suggestions(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
        return [row for row in rows if str(row.get("AXLabel")).startswith("Pump ")]

    def unobstructed(rows: list[dict[str, Any]]) -> bool:
        if not any("KeyboardKey" in row.get("traits", []) for row in rows):
            return False
        left, top, right, bottom = keyboard_bounds(rows)
        visible: list[dict[str, Any]] = suggestions(rows)
        return bool(visible) and all(
            frame["x"] + frame["width"] <= left
            or frame["x"] >= right
            or frame["y"] + frame["height"] <= top
            or frame["y"] >= bottom
            for frame in (row["frame"] for row in visible)
        )

    ui.launch("Review options", "--native-actions")
    open_review_menu(ui)
    ui.tap("Inspect popup choices")
    ui.tap("Search inspections")
    float_software_keyboard(ui)
    move_floating_keyboard(ui, 535, 1095)
    ui.key(36)  # 7
    ui.value("Search inspections", "7")
    original: dict[str, float] = ui.frame("Pump 01")
    covered: dict[str, float] = ui.frame("Pump 03")
    grabber: dict[str, float] = ui.frame("Floating keyboard grabber")
    left, top, right, _ = keyboard_bounds(ui.tree())
    target_x: float = (
        grabber["x"]
        + grabber["width"] * 0.5
        + covered["x"]
        + covered["width"] * 0.5
        - (left + right) * 0.5
    )
    target_y: float = grabber["y"] + grabber["height"] * 0.5 + covered["y"] + 6 - top

    def relocated(rows: list[dict[str, Any]]) -> bool:
        return unobstructed(rows) and any(
            row.get("AXLabel") == "Pump 01" and row["frame"] != original for row in rows
        )

    move_floating_keyboard(ui, target_x, target_y)
    ui.wait("search suggestions to move clear of the floating keyboard", relocated)
    ui.value("Search inspections", "7")
    move_floating_keyboard(ui, 535, 1095)
    ui.wait(
        "suggestions to return beside the search field",
        lambda rows: any(
            row.get("AXLabel") == "Pump 01" and row["frame"] == original for row in rows
        ),
    )
    move_floating_keyboard(ui, target_x, target_y)
    ui.wait("search suggestions to avoid the keyboard again", relocated)
    output: Path = Path(os.environ.get("TEST_UNDECLARED_OUTPUTS_DIR", "."))
    output.mkdir(parents=True, exist_ok=True)
    ui.simctl(
        "io", ui.udid, "screenshot", str(output / "floating-keyboard-choices.png")
    )
    ui.tap("Pump 01")
    ui.value("Search inspections", "Pump 01")
    ui.label("Submitted: Pump 01", present=False)
    ui.tap("Submit search")
    ui.label("Submitted: Pump 01")
    print(
        "PASS: floating keyboard misses the relocated suggestions and search submits the selected value"
    )


def exercise_floating_keyboard_long_note(ui: Simulator) -> None:
    ui.launch("Review options", "--native-actions")
    open_review_menu(ui)
    ui.tap("Write long note")
    ui.label("Ready to write")
    float_software_keyboard(ui)
    move_floating_keyboard(ui, 535, 1040)

    field: str = "Inspection narrative text"
    for usage, draft in [
        (30, "1"),
        (40, "1\n"),
        (31, "1\n2"),
        (40, "1\n2\n"),
        (32, "1\n2\n3"),
    ]:
        ui.key(usage)
        ui.value(field, draft)
    original = ui.frame(field)
    status = ui.frame("Showing first lines")
    # With the third line covered, there is room below the keyboard for the
    # caret. The 520-point editor cannot fit as a whole in the dialog viewport.
    move_floating_keyboard(ui, original["x"] + 72, original["y"] + 119)
    ui.wait(
        "multiline caret to move below the floating keyboard",
        lambda rows: any(
            row.get("AXLabel") == field and row["frame"]["y"] > original["y"] + 20
            for row in rows
        ),
    )
    assert ui.frame("Showing first lines") == status
    ui.key(33)
    ui.value(field, "1\n2\n34")
    assert ui.frame(field)["y"] > original["y"] + 20

    move_floating_keyboard(ui, 535, 1040)
    ui.wait(
        "multiline editor to return to its original position",
        lambda rows: any(
            row.get("AXLabel") == field and row["frame"] == original for row in rows
        ),
    )
    assert ui.frame("Showing first lines") == status
    ui.value(field, "1\n2\n34")
    print(
        "PASS: the floating keyboard reveals the multiline caret and restores the viewport"
    )


def exercise_menu_dynamic_type(ui: Simulator) -> None:
    settings: tuple[str, ...] = ("ui", ui.udid, "content_size")
    original: str = ui.simctl(*settings)
    largest: str = "accessibility-extra-extra-extra-large"

    def expect_first_height(minimum: float, maximum: float = float("inf")) -> None:
        ui.wait(
            "the open menu row to resize",
            lambda rows: any(
                row.get("AXLabel") == "Record review"
                and minimum <= row["frame"]["height"] <= maximum
                for row in rows
            ),
        )

    try:
        ui.simctl(*settings, "large")
        ui.launch("Review options", "--native-actions")
        open_review_menu(ui)
        standard: float = ui.frame("Record review")["height"]
        ui.simctl(*settings, largest)
        expect_first_height(standard + 12)
        enlarged = ui.frame("Record review")["height"]
        ui.tap("Record review")
        ui.label("Reviews 1")

        open_review_menu(ui)
        ui.simctl(*settings, "large")
        expect_first_height(standard - 0.5, standard + 0.5)
        ui.simctl(*settings, largest)
        expect_first_height(standard + 12)
        ui.ui("rotate", "LANDSCAPE_LEFT")
        ui.wait(
            "landscape menu",
            lambda rows: any(
                row.get("type") == "Application"
                and row["frame"]["width"] > row["frame"]["height"]
                for row in rows
            ),
        )
        first = ui.frame("Record review")
        ui.label("Sign-in fields", present=False)
        x: str = str(round(first["x"] + first["width"] / 2))
        start: str = str(round(first["y"] + first["height"] * 3.5))
        end: str = str(round(first["y"] + first["height"] * 0.5))
        for _ in range(4):
            if ui.has(ui.tree(), "Sign-in fields"):
                break
            ui.ui("swipe", x, start, x, end, "--duration", "0.4", "--delta", "8")
        ui.tap("Sign-in fields")
        ui.label("Account email")
        print(
            f"PASS: Dynamic Type grows open menu rows from {standard:g} to {enlarged:g} points and keeps the last landscape item reachable"
        )
    finally:
        ui.ui("rotate", "PORTRAIT")
        ui.simctl(*settings, original)


def exercise_choice_dynamic_type(ui: Simulator) -> None:
    settings: tuple[str, ...] = ("ui", ui.udid, "content_size")
    original: str = ui.simctl(*settings)
    categories: tuple[str, ...] = (
        "Boilers",
        "Cabinets",
        "Drains",
        "Filters",
        "Gauges",
        "Hoses",
        "Intakes",
        "Meters",
        "Pipes",
        "Pumps",
        "Sensors",
        "Tanks",
        "Valves",
    )
    suggestions: tuple[str, ...] = tuple(f"Pump {index:02}" for index in range(1, 13))

    def visible_options(
        rows: list[dict[str, Any]], labels: tuple[str, ...]
    ) -> list[dict[str, Any]]:
        return sorted(
            (row for row in rows if row.get("AXLabel") in labels),
            key=lambda row: row["frame"]["y"],
        )

    def scroll_to_last(labels: tuple[str, ...]) -> None:
        last: str = labels[-1]
        for _ in labels:
            visible = visible_options(ui.tree(), labels)
            assert visible, labels
            if visible[-1]["AXLabel"] == last:
                return
            previous: str = visible[0]["AXLabel"]
            bottom: dict[str, float] = visible[-1]["frame"]
            x: str = str(round(bottom["x"] + bottom["width"] / 2))
            start: float = bottom["y"] + bottom["height"] * 0.8
            end: float = max(28, start - bottom["height"] * 1.65)
            ui.ui(
                "swipe",
                x,
                str(round(start)),
                x,
                str(round(end)),
                "--duration",
                "0.45",
                "--delta",
                "8",
            )
            ui.wait(
                f"choices to advance from {previous}",
                lambda rows: (
                    next(
                        (row["AXLabel"] for row in visible_options(rows, labels)), None
                    )
                    not in (None, previous)
                ),
            )
        raise AssertionError(f"last option {last} did not become visible")

    def expect_suggestions_above_keyboard() -> None:
        rows = ui.tree()
        keyboard_top: float = min(
            row["frame"]["y"] for row in rows if "KeyboardKey" in row.get("traits", [])
        )
        visible = visible_options(rows, suggestions)
        assert visible and all(
            row["frame"]["y"] + row["frame"]["height"] <= keyboard_top + 1
            for row in visible
        )

    try:
        ui.simctl(*settings, "large")
        ui.launch("Review options", "--native-actions")
        open_review_menu(ui)
        ui.tap("Inspect popup choices")
        ui.tap("Inspection category")
        standard: float = ui.frame("Boilers")["height"]
        ui.simctl(*settings, "accessibility-extra-extra-extra-large")
        ui.wait(
            "the open ComboBox row to grow",
            lambda rows: any(
                row.get("AXLabel") == "Boilers"
                and row["frame"]["height"] > standard + 15
                for row in rows
            ),
        )
        enlarged: float = ui.frame("Boilers")["height"]
        scroll_to_last(categories)
        ui.value("Inspection category", "Boilers")
        ui.tap("Valves")
        ui.value("Inspection category", "Valves")

        ui.tap("Search inspections")
        ui.keyboard(visible=True)
        ui.key(36)  # 7
        ui.value("Search inspections", "7")
        ui.label("Pump 01")
        expect_suggestions_above_keyboard()
        scroll_to_last(suggestions)
        expect_suggestions_above_keyboard()
        ui.tap("Pump 12")
        ui.value("Search inspections", "Pump 12")
        ui.label("Submitted: Pump 12", present=False)
        ui.tap("Submit search")
        ui.label("Submitted: Pump 12")
        print(
            f"PASS: choice rows grew from {standard:g} to {enlarged:g} points, with touch scrolling and search submission above the keyboard"
        )
    finally:
        ui.simctl(*settings, original)


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


def exercise_function_keys(ui: Simulator) -> None:
    ui.launch("Last key Ready", "--function-keys")
    ui.tap("Inspection commands")
    for number in range(1, 13):
        ui.key(57 + number)  # UIKit HID keyboard F1 through F12.
        ui.label(f"Last key F{number}")
    ui.label("Refreshes 1")
    ui.label("F2 releases 1")

    ui.ui("key", "67", "--shift", "--duration", "0.05")  # Shift+F10
    ui.label("Open runbook")
    ui.key(81)  # Down
    ui.key(40)  # Return
    ui.label("Runbooks 1")

    ui.key(101)  # Keyboard Application / ContextMenu
    ui.label("Open runbook")
    ui.key(81)
    ui.key(40)
    ui.label("Runbooks 2")

    ui.tap("Command draft")
    ui.key(59)  # F2
    ui.label("Refreshes 2")
    ui.label("F2 releases 2")
    ui.key(69, command=True)  # Command+F12
    ui.label("Snapshots 1")
    ui.value("Command draft", "Draft")
    ui.key(4)  # A remains ordinary text input.
    ui.value("Command draft", "Drafta")
    print("PASS: hardware function and menu keys work in controls and text editors")


def exercise_keyboard_dismissal(ui: Simulator) -> None:
    ui.launch("Open readings")
    ui.tap("Open readings")
    ui.tap("Contact details")
    ui.tap("Contact telephone")
    ui.keyboard(True)
    for digit in ("1", "2", "3"):
        ui.tap(digit)
    ui.value("Contact telephone", "123")
    ui.tap("Hide keyboard")
    time.sleep(0.6)  # Wait through UIKit's dismissal animation.
    ui.keyboard(False)
    ui.ui("rotate", "LANDSCAPE_LEFT")
    ui.wait(
        "landscape window",
        lambda rows: any(
            row.get("type") == "Application"
            and row["frame"]["width"] > row["frame"]["height"]
            for row in rows
        ),
    )
    ui.keyboard(False)
    ui.value("Contact telephone", "123")
    ui.label("Telephone: 123", present=False)
    ui.key(33)  # 4 with the software keyboard closed.
    ui.value("Contact telephone", "1234")
    ui.keyboard(False)
    ui.tap("Contact telephone")
    ui.keyboard(True)
    ui.value("Contact telephone", "1234")
    ui.key(34)  # 5
    ui.value("Contact telephone", "12345")
    ui.tap("Close contact details")

    ui.tap("Open field notes")
    ui.tap("Inspection notes")
    ui.keyboard(True)
    ui.key(4)  # A
    rows = ui.wait(
        "typed note",
        lambda rows: any(
            row.get("AXLabel") == "Inspection notes"
            and row.get("AXValue") in ("a", "A")
            for row in rows
        ),
    )
    note: str = next(
        row["AXValue"] for row in rows if row.get("AXLabel") == "Inspection notes"
    )
    ui.tap("Hide keyboard")
    time.sleep(0.6)
    ui.keyboard(False)
    ui.ui("rotate", "PORTRAIT")
    ui.wait(
        "portrait window",
        lambda rows: any(
            row.get("type") == "Application"
            and row["frame"]["width"] < row["frame"]["height"]
            for row in rows
        ),
    )
    ui.keyboard(False)
    ui.value("Inspection notes", note)
    ui.tap("Inspection notes")
    ui.keyboard(True)
    ui.value("Inspection notes", note)
    print(
        "PASS: iPad keeps the keyboard hidden across rotation and reopens both editors with their drafts"
    )


def exercise_picker_dynamic_type(ui: Simulator) -> None:
    settings: tuple[str, ...] = ("ui", ui.udid, "content_size")
    original: str = ui.simctl(*settings)
    maximum: str = "accessibility-extra-extra-extra-large"
    output: Path = Path(os.environ.get("TEST_UNDECLARED_OUTPUTS_DIR", "."))
    output.mkdir(parents=True, exist_ok=True)

    def frame_in(rows: list[dict[str, Any]], label: str) -> dict[str, float]:
        return next((row["frame"] for row in rows if row.get("AXLabel") == label), {})

    def landscape_wheels(rows: list[dict[str, Any]]) -> bool:
        minute: dict[str, float] = frame_in(rows, "Minute")
        save: dict[str, float] = frame_in(rows, "Save time")
        return (
            bool(minute and save)
            and save["x"] >= minute["x"] + minute["width"]
            and any(
                row.get("type") == "Application"
                and row["frame"]["width"] > row["frame"]["height"]
                for row in rows
            )
        )

    try:
        ui.simctl(*settings, "large")
        ui.launch()
        ui.tap("Open readings")
        ui.tap("Choose inspection date, 2024-02-28")
        standard: float = ui.frame("Calendar")["height"]
        ui.simctl(*settings, maximum)
        rows: list[dict[str, Any]] = ui.wait(
            "open calendar to grow at maximum Dynamic Type",
            lambda rows: frame_in(rows, "Calendar").get("height", 0) > standard * 1.5,
        )
        calendar: dict[str, float] = frame_in(rows, "Calendar")
        save: dict[str, float] = ui.frame("Save date")
        assert calendar["y"] + calendar["height"] <= save["y"] + 1, rows
        ui.simctl("io", ui.udid, "screenshot", str(output / "picker-large-date.png"))
        # Thursday February 29 is the middle column of the fifth week.
        ui.ui(
            "tap",
            str(round(calendar["x"] + calendar["width"] * 0.5)),
            str(round(calendar["y"] + calendar["height"] * 6.5 / 8)),
            "--duration",
            "0.12",
        )
        ui.tap("Save date")
        ui.label("Choose inspection date, 2024-02-29")

        ui.simctl(*settings, "large")
        ui.launch()
        ui.tap("Open readings")
        ui.tap("Choose inspection time, 12:30:41")
        ui.value("Minute", "30")
        ui.simctl(*settings, maximum)
        ui.ui("rotate", "LANDSCAPE_LEFT")
        rows = ui.wait(
            "landscape time wheels with confirmation at their side",
            landscape_wheels,
        )
        minute: dict[str, float] = frame_in(rows, "Minute")
        ui.simctl("io", ui.udid, "screenshot", str(output / "picker-large-time.png"))
        x: str = str(round(minute["x"] + minute["width"] / 2))
        ui.ui(
            "swipe",
            x,
            str(round(minute["y"] + minute["height"] * 0.82)),
            x,
            str(round(minute["y"] + minute["height"] * 0.18)),
            "--duration",
            "0.75",
            "--delta",
            "8",
        )
        rows = ui.wait(
            "minute to advance after a touch drag",
            lambda rows: any(
                row.get("AXLabel") == "Minute"
                and row.get("AXValue") in ("31", "32", "33")
                for row in rows
            ),
        )
        value: str = next(
            row["AXValue"] for row in rows if row.get("AXLabel") == "Minute"
        )
        ui.tap("Save time")
        ui.label("Minute", present=False)
        ui.ui("rotate", "PORTRAIT")
        ui.simctl(*settings, "large")
        ui.label(f"Choose inspection time, 12:{value}:41")
        print(
            f"PASS: Calendar grew from {standard:g} to {calendar['height']:g} points and saved the leap day. "
            f"The landscape time wheel advanced to {value} and saved at maximum Dynamic Type."
        )
    finally:
        ui.ui("rotate", "PORTRAIT")
        ui.simctl(*settings, original)


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
        "--floating-keyboard-long-note",
        action="store_true",
        help="Exercise caret avoidance in the XRack Controls long note on iPad mini",
    )
    mode.add_argument(
        "--floating-keyboard-choices",
        action="store_true",
        help="Move the iPad floating keyboard over open search suggestions",
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
    mode.add_argument(
        "--edge-back",
        action="store_true",
        help="Swipe from the leading edge through a dialog and guarded StackView",
    )
    mode.add_argument(
        "--drawer-preview",
        action="store_true",
        help="Check the bottom drawer while an opening or closing touch is still active",
    )
    mode.add_argument(
        "--tab-dynamic-type",
        action="store_true",
        help="Enlarge a tab strip while open and verify selection, reveal and closing",
    )
    mode.add_argument(
        "--button-shapes",
        action="store_true",
        help="Switch the iOS Button Shapes preference and capture transparent buttons",
    )
    mode.add_argument(
        "--button-dynamic-type",
        action="store_true",
        help="Capture and activate a fixed-height button at maximum Dynamic Type",
    )
    mode.add_argument(
        "--group-title-dynamic-type",
        action="store_true",
        help="Exercise wrapped disclosure and group headings at maximum Dynamic Type",
    )
    mode.add_argument(
        "--modal-title-dynamic-type",
        action="store_true",
        help="Wrap dialog headings and fit a short drawer at maximum Dynamic Type",
    )
    mode.add_argument(
        "--toggle-dynamic-type",
        action="store_true",
        help="Activate fixed-height checkboxes, switches and radio buttons at maximum Dynamic Type",
    )
    mode.add_argument(
        "--table-dynamic-type",
        action="store_true",
        help="Capture, sort, and edit a fixed-size table at maximum Dynamic Type",
    )
    mode.add_argument(
        "--row-dynamic-type",
        action="store_true",
        help="Check compact list and tree rows at maximum Dynamic Type",
    )
    mode.add_argument(
        "--menu-dynamic-type",
        action="store_true",
        help="Change system text size while an XRack Controls menu is open on iPhone",
    )
    mode.add_argument(
        "--choice-dynamic-type",
        action="store_true",
        help="Scroll enlarged dropdown options and search suggestions in XRack Controls",
    )
    mode.add_argument(
        "--picker-dynamic-type",
        action="store_true",
        help="Choose a date and drag a landscape time wheel at maximum system text size",
    )
    mode.add_argument(
        "--function-keys",
        action="store_true",
        help="Exercise hardware F1 through F12 and context-menu keys in XRack Controls",
    )
    mode.add_argument(
        "--keyboard-dismissal",
        action="store_true",
        help="Use the iPad system keyboard's Hide keyboard button in Pocket Tasks",
    )
    mode.add_argument(
        "--landscape-contact-form",
        action="store_true",
        help="Check the landscape iPhone contact form with soft keys and hardware Return",
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
        elif args.edge_back:
            exercise_edge_back(ui)
        elif args.drawer_preview:
            exercise_drawer_preview(ui)
        elif args.tab_dynamic_type:
            exercise_tab_dynamic_type(ui)
        elif args.button_shapes:
            exercise_button_shapes(ui)
        elif args.button_dynamic_type:
            exercise_button_dynamic_type(ui)
        elif args.group_title_dynamic_type:
            exercise_group_title_dynamic_type(ui)
        elif args.modal_title_dynamic_type:
            exercise_modal_title_dynamic_type(ui)
        elif args.toggle_dynamic_type:
            exercise_toggle_dynamic_type(ui)
        elif args.table_dynamic_type:
            exercise_table_dynamic_type(ui)
        elif args.row_dynamic_type:
            exercise_row_dynamic_type(ui)
        elif args.menu_dynamic_type:
            exercise_menu_dynamic_type(ui)
        elif args.choice_dynamic_type:
            exercise_choice_dynamic_type(ui)
        elif args.picker_dynamic_type:
            exercise_picker_dynamic_type(ui)
        elif args.function_keys:
            exercise_function_keys(ui)
        elif args.keyboard_dismissal:
            exercise_keyboard_dismissal(ui)
        elif args.landscape_contact_form:
            exercise_landscape_contact_form(ui, soft_actions=False)
            exercise_landscape_contact_form(ui, soft_actions=True)
        elif args.split_demo:
            exercise_split_demo(ui)
        elif args.floating_keyboard:
            exercise_floating_keyboard(ui)
        elif args.floating_keyboard_long_note:
            exercise_floating_keyboard_long_note(ui)
        elif args.floating_keyboard_choices:
            exercise_floating_keyboard_choices(ui)
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
            if args.range_demo
            or args.scroll_bar_demo
            or args.native_actions_demo
            or args.edge_back
            or args.drawer_preview
            or args.tab_dynamic_type
            or args.button_dynamic_type
            or args.table_dynamic_type
            or args.menu_dynamic_type
            or args.choice_dynamic_type
            or args.function_keys
            or args.floating_keyboard_long_note
            or args.floating_keyboard_choices
            else "Open wheels"
            if args.path_view_demo
            else "Inspection rows"
            if args.row_dynamic_type
            else "Inspection toggles"
            if args.toggle_dynamic_type
            else "Open calibration"
            if args.modal_title_dynamic_type
            else "Plain action"
            if args.button_shapes
            else "Inspection options"
            if args.group_title_dynamic_type
            else "保存布局"
            if args.split_demo
            else "Check pump pressure"
        )
        demo_arguments: tuple[str, ...] = (
            ("--compact-rows",)
            if args.row_dynamic_type
            else ("--compact-toggles",)
            if args.toggle_dynamic_type
            else ("--modal-titles",)
            if args.modal_title_dynamic_type
            else ("--button-shapes",)
            if args.button_shapes
            else ("--group-titles",)
            if args.group_title_dynamic_type
            else ()
        )
        ui.launch(ready, *demo_arguments)


if __name__ == "__main__":
    main()
