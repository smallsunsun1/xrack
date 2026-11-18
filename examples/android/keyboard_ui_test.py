import argparse
import os
import re
import subprocess
import tempfile
import time
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path

from examples.android.navigation_ui_test import ACTIVITY as NAVIGATION_ACTIVITY
from examples.android.navigation_ui_test import PACKAGE as NAVIGATION_PACKAGE
from examples.android.navigation_ui_test import NavigationEmulator
from examples.android.simulator_ui_test import (
    ACTIVITY,
    PACKAGE,
    Emulator,
    Label,
    Screen,
)


class KeyboardEmulator(NavigationEmulator):
    def screen(self) -> Screen:
        screen = super().screen()
        labels: list[Label] = []
        for label in screen.labels:
            text = label.text
            match = re.fullmatch(r"Hotkeys ([0-9OoIl]+)", text)
            if match is not None:
                text = "Hotkeys " + match[1].translate(str.maketrans("OoIl", "0011"))
            labels.append(Label(label.x, label.y, text))
        return Screen(screen.png, screen.width, screen.height, labels)


def combo(ui: Emulator, *keys: str) -> None:
    ui.call("shell", "input", "keycombination", *keys)


def ctrl(ui: Emulator, letter: str, *, shift: bool = False) -> None:
    keys = ["KEYCODE_CTRL_LEFT"]
    if shift:
        keys.append("KEYCODE_SHIFT_LEFT")
    combo(ui, *keys, f"KEYCODE_{letter.upper()}")


def text_in_row(
    ui: Emulator, screen: Screen, value: str, y: float, *, max_x: float = 1
) -> bool:
    wanted = value.casefold()
    spellings = {wanted, wanted + "i", wanted + "l", wanted + "|"}
    if wanted == "new task":
        # Vision merges the caret into the empty editor's placeholder N.
        spellings.add("ew task")

    def in_editor(labels: list[Label]) -> bool:
        return any(
            label.x < max_x
            and abs(label.y - y) * screen.height < 36
            and label.text.casefold() in spellings
            for label in labels
        )

    if in_editor(screen.labels):
        return True
    # At half resolution, Vision can lose letters next to the caret or
    # selected text. Retry only this row at the original resolution.
    output = subprocess.check_output(
        [ui.recognize, "--full-size", f"--row-y={y}"],
        input=screen.png,
        timeout=20,
    )
    labels: list[Label] = []
    for line in output.decode().splitlines():
        column, row, text = line.split("\t", 2)
        labels.append(Label(float(column), float(row), text))
    return in_editor(labels)


def draft(ui: Emulator, value: str, description: str) -> Screen:
    def matches(screen: Screen) -> bool:
        add = screen.find("Add")
        return add is not None and text_in_row(ui, screen, value, add.y, max_x=add.x)

    return ui.wait_for(description, matches)


@contextmanager
def held_control(ui: Emulator) -> Iterator[subprocess.Popen[bytes]]:
    # Headless virtio emulators have no hardware keyboard for `emu event send`.
    # Hold both Ctrl keys through Android's input dispatcher instead, then let
    # their key-up events reach the launcher after the app loses focus.
    process = subprocess.Popen(
        [
            ui.adb,
            "-s",
            ui.serial,
            "shell",
            "input",
            "keycombination",
            "-t",
            "15000",
            "KEYCODE_CTRL_LEFT",
            "KEYCODE_CTRL_RIGHT",
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    try:
        time.sleep(0.5)
        if process.poll() is not None:
            raise AssertionError("The Ctrl hold ended before the test started")
        yield process
    finally:
        output, _ = process.communicate(timeout=20)
        if process.returncode != 0:
            raise AssertionError(f"Holding Ctrl failed: {output.decode()}")


def application_shortcuts(ui: Emulator, apk: Path) -> None:
    ui.call("install", "-r", str(apk))
    ui.call("shell", "am", "force-stop", NAVIGATION_PACKAGE)
    ui.call("shell", "am", "start", "-W", "-n", NAVIGATION_ACTIVITY)
    ui.visible("Hotkeys 0", "Mode Balanced", timeout=75)
    ctrl(ui, "K")
    ui.visible("Hotkeys 1")
    with held_control(ui) as process:
        ui.call("shell", "input", "keyevent", "KEYCODE_K")
        ui.visible("Hotkeys 2")
        ui.call("shell", "input", "keyevent", "KEYCODE_HOME")
        if process.poll() is not None:
            raise AssertionError("Ctrl was released before switching to the launcher")
    ui.call("shell", "am", "start", "-W", "-n", NAVIGATION_ACTIVITY)
    ui.visible("Hotkeys 2", "Overview", "Settings")
    ui.call("shell", "input", "keyevent", "KEYCODE_K")
    # A fresh page render proves the plain K did not activate a stale Ctrl+K.
    ui.tap(ui.visible("Settings"), "Settings")
    ui.visible("Settings page", "Hotkeys 2")
    ctrl(ui, "K")
    ui.visible("Hotkeys 3")


def editing_shortcuts(ui: Emulator, apk: Path) -> None:
    ui.call("install", "-r", str(apk))
    ui.call("shell", "am", "force-stop", PACKAGE)
    ui.call("shell", "am", "start", "-W", "-f", "0x10008000", "-n", ACTIVITY)
    ui.tap(ui.visible("Open 9", "New task", timeout=75), "New task")
    ui.type_text("First")
    ctrl(ui, "A")
    ui.type_text("New")
    ctrl(ui, "A")
    ctrl(ui, "C")
    ctrl(ui, "X")
    draft(ui, "New task", "cut empties the editor")
    ctrl(ui, "Z")
    draft(ui, "New", "undo restores the cut text")
    ctrl(ui, "Z", shift=True)
    draft(ui, "New task", "Ctrl+Shift+Z reapplies the cut")
    ctrl(ui, "Z")
    draft(ui, "New", "undo restores text before Ctrl+Y")
    ctrl(ui, "Y")
    draft(ui, "New task", "Ctrl+Y reapplies the cut")
    ctrl(ui, "V")
    draft(ui, "New", "paste restores the copied value")
    combo(ui, "KEYCODE_SHIFT_LEFT", "KEYCODE_DPAD_LEFT")
    ui.type_text("t")
    ui.tap(ui.visible("Add"), "Add")
    ui.visible("Open 10", "Net")

    ui.tap(ui.visible("Notes"), "Notes")
    ui.tap(
        ui.visible("Record findings and follow-up steps"),
        "Record findings and follow-up steps",
    )
    ctrl(ui, "V")
    ui.wait_for(
        "the separate editor receives the clipboard",
        lambda screen: any(label.text.startswith("New") for label in screen.labels),
    )
    ui.tap(ui.visible("Read-only"), "Read-only")

    # A successful copy from the read-only field must replace a different value.
    ui.tap(ui.visible("Tasks"), "Tasks")
    ui.tap(ui.visible("New task"), "New task")
    ui.type_text("Other")
    ctrl(ui, "A")
    ctrl(ui, "C")
    ctrl(ui, "X")
    draft(ui, "New task", "the clipboard sentinel is cut from the task draft")
    ui.tap(ui.visible("Notes"), "Notes")
    note = ui.visible("New", "Read-only")
    note_y = note.require("New").y
    ui.tap(note, "New")
    ctrl(ui, "A")
    # Paste must be checked while the clipboard still contains Other.
    for key in ["V", "X", "Z"]:
        ctrl(ui, key)
        ui.wait_for(
            f"read-only Ctrl+{key} preserves the note",
            lambda screen: text_in_row(ui, screen, "New", note_y),
        )
        # Force a new page render, so an old frame cannot hide an edit.
        ui.tap(ui.visible("Tasks"), "Tasks")
        ui.tap(ui.visible("Net", "Notes"), "Notes")
        note = ui.visible("New", "Read-only")
        note_y = note.require("New").y
        ui.tap(note, "New")
        ctrl(ui, "A")
    ctrl(ui, "C")
    ui.tap(ui.visible("Tasks"), "Tasks")
    ui.tap(ui.visible("Net", "Notes"), "Notes")
    ui.visible("New", "Read-only")
    ui.tap(ui.visible("Tasks"), "Tasks")
    ui.tap(ui.visible("New task"), "New task")
    ctrl(ui, "V")
    draft(ui, "New", "read-only copy replaces the clipboard sentinel")
    ui.tap(ui.visible("Add"), "Add")
    final = ui.visible("Open 11", "Net", "New")
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "keyboard-shortcuts.png").write_bytes(final.png)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Exercise Android editing and application shortcuts"
    )
    parser.add_argument("--serial", required=True)
    parser.add_argument("--apk", required=True, type=Path)
    parser.add_argument("--adb", required=True, type=Path)
    parser.add_argument("--navigation-apk", required=True, type=Path)
    args = parser.parse_args()
    adb = args.adb.resolve()
    if not adb.is_file() or not args.apk.is_file() or not args.navigation_apk.is_file():
        parser.error("Both APKs and adb must exist")
    artifacts = Path(os.environ.get("TEST_UNDECLARED_OUTPUTS_DIR", "."))
    with tempfile.TemporaryDirectory() as temporary:
        recognize = Path(temporary) / "screen_text"
        subprocess.run(
            [
                "xcrun",
                "--sdk",
                "macosx",
                "swiftc",
                Path(__file__).with_name("screen_text.swift"),
                "-o",
                recognize,
            ],
            check=True,
        )
        ui = KeyboardEmulator(adb, args.serial, recognize, artifacts)
        rotation = ui.call("shell", "cmd", "window", "user-rotation").decode().split()
        try:
            ui.call("shell", "cmd", "window", "user-rotation", "lock", "0")
            application_shortcuts(ui, args.navigation_apk.resolve())
            editing_shortcuts(ui, args.apk.resolve())
            ui.call("shell", "am", "start", "-W", "-n", NAVIGATION_ACTIVITY)
            final = ui.visible("Settings page", "Hotkeys 3")
            (artifacts / "keyboard-action.png").write_bytes(final.png)
            print(
                "PASS editing shortcuts, read-only protection and focus-loss modifier reset",
                flush=True,
            )
        finally:
            ui.call("shell", "cmd", "window", "user-rotation", *rotation)


if __name__ == "__main__":
    main()
