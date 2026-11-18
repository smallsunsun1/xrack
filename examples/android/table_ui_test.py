import argparse
import os
import re
import subprocess
import tempfile
import time
from pathlib import Path

from examples.android.simulator_ui_test import (
    Emulator,
    Label,
    Screen,
    test_input_method,
)


PACKAGE = "dev.xrack.android_table"
ACTIVITY = f"{PACKAGE}/dev.xrack.platform.XRackActivity"


class TableEmulator(Emulator):
    def screen(self) -> Screen:
        screen = super().screen()
        labels: list[Label] = []
        for label in screen.labels:
            match = re.fullmatch(
                r"(Cells|Pump units|Selected|Cursor|Expanded|Collapsed|Changes|Browse width|Edit height"
                r"|Taps|Starts|Finishes|Drops|Cancels|Offset X|Offset Y|Key|Index|Choices|Entries|Scrolls|Links)"
                r"(?: +|(?=-))(-?[0-9OoIl]+(?: +[0-9OoIl]+)*)",
                label.text,
            )
            text = (
                label.text
                if match is None
                else (
                    match[1]
                    + " "
                    + match[2].replace(" ", "").translate(str.maketrans("OoIl", "0011"))
                )
            )
            labels.append(Label(label.x, label.y, text))
        return Screen(screen.png, screen.width, screen.height, labels)


def ctrl(ui: Emulator, key: str) -> None:
    ui.call("shell", "input", "keycombination", "KEYCODE_CTRL_LEFT", f"KEYCODE_{key}")


def touch_cell(ui: Emulator, cell: tuple[float, float]) -> None:
    with ui.press_contact(cell):
        # Keep the physical touch down for a short tap, below the hold interval.
        time.sleep(0.12)


def exercise(ui: Emulator) -> None:
    initial = ui.visible(
        "XRack Inventory",
        "Window active",
        "Pump",
        "Valve",
        "Part",
        "Units",
        "Cells 0",
        timeout=75,
    )
    unit_cell = (initial.require("Units").x, initial.require("Pump").y)
    # The emulator can display its first frame before it routes touch to the
    # new native window. Retry the same selection after the double-tap window.
    for attempt in range(3):
        touch_cell(ui, unit_cell)
        try:
            selected = ui.visible("Cells 1", "Selection idle", "Reset view", timeout=5)
            break
        except AssertionError:
            if attempt == 2:
                raise
            print("Waiting for the launched window to accept input", flush=True)
    ui.tap(selected, "Reset view")
    base = ui.visible("Pump", "Part", "Units", "Cells 0")
    pump = base.require("Pump")
    with ui.drag_contact((0.78, pump.y), (0.40, pump.y)):
        ui.wait_for(
            "horizontal touch scroll reveals Status and keeps the frozen Part column",
            lambda screen: (
                (frozen := screen.find("Pump")) is not None
                and abs(frozen.x - pump.x) < 0.01
                and screen.find("Status") is not None
                and screen.find("Cells 0") is not None
            ),
        )
        # Let the last movement age out of the fling window before lifting.
        time.sleep(0.15)
    ui.tap(ui.visible("Reset view"), "Reset view")
    top = ui.visible("Pump", "Units", "Motor", "Cells 0")
    motor = top.require("Motor")
    with ui.drag_contact((0.57, motor.y), (0.57, pump.y)):
        ui.visible("Strainer", "Cells 0", "Selection idle")
        time.sleep(0.15)
    ui.tap(ui.visible("Reset view"), "Reset view")
    top = ui.visible("Pump", "Valve", "Units", "Cells 0")

    source = top.require("Pump")
    target = (top.require("Units").x, top.require("Valve").y)
    start = (source.x, source.y)
    with ui.press_contact(start):
        ui.visible("Selecting", "Cells 0")
        ui.move_contact(start, target)
        ui.visible("Selecting", "Cells 0")
    selected = ui.visible("Cells 4", "Selection idle")
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "table-selection.png").write_bytes(selected.png)

    ctrl(ui, "C")
    gasket = ui.visible("Gasket").require("Gasket")
    touch_cell(ui, (gasket.x, gasket.y))
    ui.visible("Cells 1")
    ctrl(ui, "V")
    ui.visible("Pasted range matches")


def exercise_cell_edit(ui: Emulator) -> None:
    ui.tap(ui.visible("Edit pump units"), "Edit pump units")
    ui.visible("XRack test keyboard", "Submit")

    ctrl(ui, "A")
    ui.tap(ui.visible("Commit Unicode"), "Commit Unicode")
    ui.tap(ui.visible("Commit Unicode sent", "Submit"), "Submit")
    ui.visible("Submit sent", "Pump units 12")
    # The rejected draft stays in the same open editor, so it can be replaced.
    ctrl(ui, "A")
    ui.tap(ui.visible("Commit number"), "Commit number")
    ui.tap(ui.visible("Commit number sent", "Pump units 12", "Submit"), "Submit")
    final = ui.wait_for(
        "IME submit commits 47 and closes the cell editor",
        lambda screen: (
            screen.find("Pump units 47") is not None
            and screen.find("Pasted range matches") is not None
            and screen.find("Submit") is None
        ),
    )
    (ui.artifacts / "table-edited.png").write_bytes(final.png)


def exercise_directory(ui: Emulator) -> None:
    start = ui.visible("XRack Inventory", "Window active", "Open directory", timeout=75)
    for attempt in range(3):
        ui.tap(start, "Open directory")
        try:
            ui.visible("Inspection directory", "Workshop", "Archive", timeout=5)
            break
        except AssertionError:
            if attempt == 2:
                raise
    density = ui.call("shell", "wm", "density").decode()
    dpi = re.findall(r"(?:Physical|Override) density:\s*(\d+)", density)
    if not dpi:
        raise AssertionError(f"Android did not report display density: {density}")
    scale = int(dpi[-1]) / 160

    def toggle(screen: Screen, name: str, depth: int, *, rtl: bool = False) -> None:
        # The centered dialog has a 16px body inset. The tree owns the 22px
        # disclosure slot at 4px + one 44px indentation per level.
        left = (screen.width - 350 * scale) / 2
        x = (left + (16 + 4 + 11 + 44 * depth) * scale) / screen.width
        touch_cell(ui, (1 - x if rtl else x, screen.require(name).y))

    def folded(*texts: str, absent: tuple[str, ...]) -> Screen:
        return ui.wait_for(
            f"{', '.join(texts)} with hidden nodes excluded",
            lambda screen: (
                all(screen.find(text) for text in texts)
                and all(screen.find(text) is None for text in absent)
            ),
        )

    initial = folded(
        "Workshop",
        "Archive",
        "Selected -1",
        "Expanded 0",
        absent=("Reports", "Old notes"),
    )
    toggle(initial, "Workshop", 0)
    toggle(ui.visible("Reports", "Expanded 1"), "Reports", 1)
    expanded = ui.visible("Pump check", "Valve check", "Expanded 2", "Changes 0")
    leaf = expanded.require("Pump check")
    touch_cell(ui, (leaf.x, leaf.y))
    selection = ui.visible("Selected 21", "Cursor 21", "Changes 1")
    toggle(selection, "Workshop", 0)
    collapsed = folded(
        "Workshop",
        "Cursor 10",
        "Selected 21",
        "Collapsed 1",
        "Changes 1",
        absent=("Reports", "Pump check"),
    )
    toggle(collapsed, "Workshop", 0)
    ui.visible("Reports", "Pump check", "Selected 21", "Expanded 3", "Changes 1")

    ui.tap(ui.visible("Find old notes"), "Find old notes")
    filtered = folded(
        "Archive",
        "Old notes",
        "Show all",
        "Selected 21",
        "Expanded 3",
        "Changes 1",
        absent=("Workshop", "Pump check", "Old photos"),
    )
    ui.tap(filtered, "Show all")
    restored = folded(
        "Workshop",
        "Reports",
        "Pump check",
        "Archive",
        "Selected 21",
        "Changes 1",
        absent=("Old notes",),
    )
    ui.tap(restored, "Insert report")
    inserted = ui.visible("Fresh notes", "Pump check", "Selected 21", "Changes 1")
    if inserted.require("Fresh notes").y >= inserted.require("Pump check").y:
        raise AssertionError(
            "The new sibling was not inserted above the selected record"
        )

    ui.tap(inserted, "Mirror tree")
    mirrored = ui.visible("Restore direction", "Workshop", "Pump check", "Selected 21")
    if mirrored.require("Workshop").x <= inserted.require("Workshop").x + 0.20:
        raise AssertionError("The RTL layout did not move the Workshop row content")
    toggle(mirrored, "Workshop", 0, rtl=True)
    collapsed = folded(
        "Workshop", "Selected 21", "Collapsed 2", "Changes 1", absent=("Pump check",)
    )
    toggle(collapsed, "Workshop", 0, rtl=True)
    final = ui.visible(
        "Fresh notes", "Pump check", "Selected 21", "Expanded 4", "Changes 1"
    )
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "tree-rtl.png").write_bytes(final.png)
    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
    folded("Open directory", "Part", "Units", absent=("Inspection directory",))


def exercise_split(ui: Emulator) -> None:
    start = ui.visible("XRack Inventory", "Window active", "Open split", timeout=75)
    for attempt in range(3):
        ui.tap(start, "Open split")
        try:
            ui.visible("Split workspace", "Browse", "Edit", timeout=5)
            break
        except AssertionError:
            if attempt == 2:
                raise
    density = ui.call("shell", "wm", "density").decode()
    dpi = re.findall(r"(?:Physical|Override) density:\s*(\d+)", density)
    if not dpi:
        raise AssertionError(f"Android did not report display density: {density}")
    scale = int(dpi[-1]) / 160

    def state(
        width: int, height: int, *texts: str, phase: str = "Split idle"
    ) -> Screen:
        return ui.visible(
            f"Browse width {width}", f"Edit height {height}", phase, *texts
        )

    def column_handle(
        screen: Screen, width: int, *, rtl: bool = False
    ) -> tuple[float, float]:
        # The painted divider is 8px wide. Start 12px from its center,
        # outside that strip and within the 36px touch target.
        left = (screen.width - 338 * scale) / 2
        x = (left + (16 + width + 4 + 12) * scale) / screen.width
        y = screen.require("Browse").y - 76 * scale / screen.height
        return (1 - x if rtl else x, y)

    def row_handle(screen: Screen, height: int) -> tuple[float, float]:
        pane = screen.require("Edit")
        return pane.x, pane.y + (height / 2 + 4 + 11) * scale / screen.height

    def displaced(
        screen: Screen, point: tuple[float, float], dx: float, dy: float
    ) -> tuple[float, float]:
        return point[0] + dx * scale / screen.width, point[
            1
        ] + dy * scale / screen.height

    initial = state(112, 120, "Browse", "Edit")
    handle = column_handle(initial, 112)
    outside = displaced(initial, handle, 14, 0)
    with ui.drag_contact(outside, displaced(initial, outside, 64, 0)):
        state(112, 120)
    with ui.drag_contact(handle, displaced(initial, handle, 90, 28)):
        state(166, 120, phase="Resizing columns")
    ui.tap(state(166, 120, "Save split"), "Save split")
    saved = state(166, 120, "Split saved", "Edit")

    divider = row_handle(saved, 120)
    with ui.drag_contact(divider, displaced(saved, divider, -20, -90)):
        state(166, 64, phase="Resizing rows")
    narrow = state(166, 64, "Browse")
    handle = column_handle(narrow, 166)
    with ui.drag_contact(handle, displaced(narrow, handle, -112, 0)):
        state(80, 64, phase="Resizing columns")
    ui.tap(state(80, 64, "Restore split"), "Restore split")
    ui.tap(state(166, 120, "Mirror panes"), "Mirror panes")
    mirrored = state(166, 120, "Restore sides", "Browse", "Edit")
    if mirrored.require("Browse").x <= mirrored.require("Edit").x:
        raise AssertionError("RTL did not put the Browse pane on the right")
    handle = column_handle(mirrored, 166, rtl=True)
    with ui.drag_contact(handle, displaced(mirrored, handle, 112, 0)):
        state(80, 120, phase="Resizing columns")
    ui.tap(state(80, 120, "Restore split"), "Restore split")
    restored = state(166, 120, "Restore sides", "Lock split")
    ui.tap(restored, "Lock split")
    locked = state(166, 120, "Unlock split", "Browse")
    handle = column_handle(locked, 166, rtl=True)
    with ui.drag_contact(handle, displaced(locked, handle, 112, 0)):
        state(166, 120, "Unlock split")
    ui.tap(state(166, 120, "Unlock split"), "Unlock split")
    final = state(166, 120, "Restore sides", "Lock split")
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "split-restored.png").write_bytes(final.png)
    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
    ui.wait_for(
        "Back leaves the split workspace and returns to the table",
        lambda screen: bool(
            screen.find("Open split")
            and screen.find("Part")
            and not screen.find("Split workspace")
        ),
    )


def exercise_transfer(ui: Emulator) -> None:
    entry = ui.visible("XRack Inventory", "Window active", "Open transfer", timeout=75)
    for attempt in range(3):
        ui.tap(entry, "Open transfer")
        try:
            ui.visible("Inspection transfer", "Card A", "Copy bay", timeout=5)
            break
        except AssertionError:
            if attempt == 2:
                raise

    def point(screen: Screen, label: str) -> tuple[float, float]:
        item = screen.require(label)
        return item.x, item.y

    initial = ui.visible("Card A", "Taps 0", "Starts 0", "Drops 0", "No transfer yet")
    touch_cell(ui, point(initial, "Card A"))
    tapped = ui.visible("Card A", "Taps 1", "Starts 0", "Finishes 0", "Drops 0")
    with ui.drag_contact(point(tapped, "Card A"), point(tapped, "Other bay")):
        ui.visible("Starts 1", "Finishes 0", "Drops 0", "No target", "Card A")
    rejected = ui.visible(
        "Transfer canceled",
        "Taps 1",
        "Starts 1",
        "Finishes 1",
        "Drops 0",
        "Cancels 1",
        "Card A",
    )

    with ui.drag_contact(point(rejected, "Card A"), point(rejected, "Copy bay")):
        ui.visible("Over copy", "Starts 2", "Finishes 1", "Drops 0", "Card A")
        back = point(rejected, "Card A")
        target = point(rejected, "Copy bay")
        ui.move_contact(target, back)
        ui.visible("No target", "Starts 2", "Finishes 1", "Drops 0")
        ui.move_contact(back, target)
        ui.visible("Over copy", "Starts 2", "Finishes 1", "Drops 0")
    copied = ui.visible(
        "Copied Card A",
        "No target",
        "Taps 1",
        "Finishes 2",
        "Drops 1",
        "Cancels 1",
        "Card A",
    )

    with ui.drag_contact(point(copied, "Card A"), point(copied, "Archive bay")):
        ui.visible("Over archive", "Starts 3", "Finishes 2", "Drops 1", "Card A")
        ui.call("shell", "input", "keyevent", "KEYCODE_HOME")
        ui.wait_for(
            "transfer leaves the foreground before the touch lifts",
            lambda screen: (
                screen.find("Inspection transfer") is None
                and screen.find("XRack Inventory") is None
            ),
        )
    ui.call("shell", "am", "start", "-W", "-n", ACTIVITY)
    resumed = ui.visible(
        "Inspection transfer",
        "Window active",
        "Transfer canceled",
        "Taps 1",
        "Starts 3",
        "Finishes 3",
        "Drops 1",
        "Cancels 2",
        "Card A",
    )
    touch_cell(ui, point(resumed, "Card A"))
    ready = ui.visible("Taps 2", "Starts 3", "Finishes 3", "Card A")
    with ui.drag_contact(point(ready, "Card A"), point(ready, "Archive bay")):
        ui.visible("Over archive", "Starts 4", "Finishes 3", "Drops 1", "Card A")
    final = ui.wait_for(
        "the move commits once and removes Card A from the source",
        lambda screen: (
            all(
                screen.find(text)
                for text in (
                    "Archived Card A",
                    "No target",
                    "Taps 2",
                    "Starts 4",
                    "Finishes 4",
                    "Drops 2",
                    "Cancels 2",
                )
            )
            and screen.find("Card A") is None
        ),
    )
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "transfer-archived.png").write_bytes(final.png)


def exercise_canvas(ui: Emulator) -> None:
    entry = ui.visible("XRack Inventory", "Window active", "Open canvas", timeout=75)
    for attempt in range(3):
        ui.tap(entry, "Open canvas")
        try:
            ui.visible("Inspection canvas", timeout=5)
            break
        except AssertionError:
            if attempt == 2:
                raise

    def point(screen: Screen, text: str) -> tuple[float, float]:
        label = screen.require(text)
        return label.x, label.y

    def has_offsets(screen: Screen) -> bool:
        for axis in ("X", "Y"):
            if not any(
                label.text.startswith(f"Offset {axis} ")
                and label.text.rsplit(" ", 1)[-1].isdigit()
                and int(label.text.rsplit(" ", 1)[-1]) > 0
                for label in screen.labels
            ):
                return False
        return True

    origin = ui.visible(
        "Inspect tile",
        "Across",
        "Down",
        "Offset X 0",
        "Offset Y 0",
        "Canvas idle",
        "Taps 0",
        "Lock pan",
    )
    assert origin.find("Far corner") is None
    tile = point(origin, "Inspect tile")
    touch_cell(ui, tile)
    ui.visible("Taps 1")
    end = (tile[0] - 0.18, tile[1] - 0.065)
    with ui.drag_contact(tile, end):
        ui.wait_for(
            "a touch on the tile pans both axes without clicking",
            lambda screen: (
                screen.find("Canvas dragging") is not None
                and screen.find("Taps 1") is not None
                and has_offsets(screen)
            ),
        )
        # Lift after the last movement stops contributing to flick velocity.
        time.sleep(0.15)
    panned = ui.wait_for(
        "both attached thumbs follow the viewport",
        lambda screen: (
            screen.find("Canvas idle") is not None
            and screen.find("Taps 1") is not None
            and has_offsets(screen)
            and (across := screen.find("Across")) is not None
            and across.x > origin.require("Across").x + 0.035
            and (down := screen.find("Down")) is not None
            and down.y > origin.require("Down").y + 0.018
        ),
    )
    touch_cell(ui, point(panned, "Reset canvas"))
    reset = ui.visible("Inspect tile", "Offset X 0", "Offset Y 0", "Lock pan")
    touch_cell(ui, point(reset, "Lock pan"))
    ui.visible("Unlock pan")
    with ui.drag_contact(tile, end):
        pass
    touch_cell(ui, tile)
    locked = ui.visible("Offset X 0", "Offset Y 0", "Canvas idle", "Taps 2")

    across = point(locked, "Across")
    with ui.drag_contact(across, (across[0] + 0.38, across[1])):
        ui.visible("Offset X 248", "Offset Y 0", "Canvas idle", "Taps 2")
    right_edge = ui.visible("Offset X 248", "Offset Y 0", "Down")
    down = point(right_edge, "Down")
    with ui.drag_contact(down, (down[0], down[1] + 0.15)):
        ui.visible("Offset X 248", "Offset Y 192", "Far corner", "Taps 2")
    corner = ui.wait_for(
        "the far corner is rendered at the clamped offsets",
        lambda screen: (
            screen.find("Far corner") is not None
            and screen.find("Inspect tile") is None
            and screen.find("Offset X 248") is not None
            and screen.find("Offset Y 192") is not None
        ),
    )
    touch_cell(ui, point(corner, "Reset canvas"))
    ui.wait_for(
        "programmatic reset restores content and thumbs while pan is locked",
        lambda screen: (
            all(
                screen.find(text)
                for text in ("Inspect tile", "Offset X 0", "Offset Y 0", "Taps 2")
            )
            and screen.find("Far corner") is None
            and (across_label := screen.find("Across")) is not None
            and abs(across_label.x - origin.require("Across").x) < 0.01
            and (down_label := screen.find("Down")) is not None
            and abs(down_label.y - origin.require("Down").y) < 0.01
        ),
    )
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "canvas-corner.png").write_bytes(corner.png)


def exercise_catalog(ui: Emulator) -> None:
    entry = ui.visible("XRack Inventory", "Window active", "Open catalog", timeout=75)
    for attempt in range(3):
        ui.tap(entry, "Open catalog")
        try:
            ui.visible("Parts catalog", timeout=8)
            break
        except AssertionError:
            if attempt == 2:
                raise

    def tap(screen: Screen, text: str) -> None:
        label = screen.require(text)
        touch_cell(ui, (label.x, label.y))

    initial = ui.visible(
        "Pump", "Valve", "Pin Valve", "Key -1", "Index -1", "Choices 0"
    )
    valve = initial.require("Valve")
    assert any(
        label.text == "Pump"
        and abs(label.y - valve.y) < 0.015
        and label.x < valve.x - 0.2
        for label in initial.labels
    ), "Pump and Valve should be on the first grid row"
    tap(initial, "Valve")
    selected = ui.visible("Key 20", "Index 1", "Choices 1", "Pin Valve")
    tap(selected, "Pin Valve")
    pinned = ui.visible("Unpin Valve", "Key 20", "Index 1", "Choices 1")

    checkbox = pinned.require("Unpin Valve")
    with ui.drag_contact((checkbox.x, checkbox.y), (checkbox.x, checkbox.y - 0.22)):
        scrolled = ui.wait_for(
            "dragging from the checkbox scrolls the selected tile out of view",
            lambda screen: (
                all(
                    screen.find(text)
                    for text in ("Shaft", "Key 20", "Index 1", "Choices 1")
                )
                and screen.find("Unpin Valve") is None
                and screen.find("Valve") is None
            ),
        )
        time.sleep(0.15)
    tap(scrolled, "Move Valve")
    moved = ui.visible(
        "Key 20", "Index 10", "Choices 1", "Unpin Valve", "Hide Valve", "Pin Panel"
    )
    tap(moved, "Hide Valve")
    filtered = ui.wait_for(
        "filtering suspends the displayed index and retains the business key",
        lambda screen: (
            all(screen.find(text) for text in ("Show Valve", "Key 20", "Choices 1"))
            and any(label.text == "Index -1" for label in screen.labels)
            and screen.find("Valve") is None
            and screen.find("Unpin Valve") is None
        ),
    )
    tap(filtered, "Show Valve")
    restored = ui.visible("Key 20", "Index 10", "Choices 1", "Unpin Valve", "Pin Panel")
    tap(restored, "Pin Panel")
    independent = ui.visible(
        "Unpin Valve", "Unpin Panel", "Key 20", "Index 10", "Choices 1"
    )
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "catalog-restored.png").write_bytes(independent.png)


def exercise_inspection_log(ui: Emulator) -> None:
    entry = ui.visible("XRack Inventory", "Window active", "Open log", timeout=75)
    for attempt in range(3):
        ui.tap(entry, "Open log")
        try:
            ui.visible("Inspection log", timeout=8)
            break
        except AssertionError:
            if attempt == 2:
                raise

    def tap(screen: Screen, text: str) -> None:
        label = screen.require(text)
        touch_cell(ui, (label.x, label.y))

    initial = ui.visible("Intake 4", "Seal intake", "Entries 12", "Scrolls 0")
    start = initial.require("Seal intake")
    top = initial.require("Intake 4").y
    with ui.drag_contact((start.x, start.y), (start.x, start.y - 0.108)):
        pinned = ui.wait_for(
            "the intake heading stays at the top while rows move under it",
            lambda screen: (
                (heading := screen.find("Intake 4 pinned")) is not None
                and abs(heading.y - top) < 0.01
                and (seal := screen.find("Seal intake")) is not None
                and 0.084 < start.y - seal.y < 0.115
                and screen.find("Bench 4") is not None
            ),
        )
        time.sleep(0.15)

    seal_y = pinned.require("Seal intake").y
    scrolls = next(
        label.text for label in pinned.labels if label.text.startswith("Scrolls ")
    )
    assert scrolls != "Scrolls 0"

    def preserved(entries: int, section_size: int, action: str) -> Screen:
        return ui.wait_for(
            f"{action} keeps the seal at its reading position",
            lambda screen: (
                screen.find(f"Entries {entries}") is not None
                and screen.find(scrolls) is not None
                and (heading := screen.find(f"Intake {section_size} pinned"))
                is not None
                and abs(heading.y - top) < 0.01
                and (seal := screen.find("Seal intake")) is not None
                and abs(seal.y - seal_y) < 0.012
            ),
        )

    tap(pinned, "Insert intake")
    inserted = preserved(13, 5, "inserting above the viewport")
    tap(inserted, "Hide bracket")
    filtered = preserved(12, 4, "filtering an earlier row")
    tap(filtered, "Show bracket")
    restored = preserved(13, 5, "restoring the earlier row")

    bench = restored.require("Bench 4")
    first = (bench.x, top + 0.216)
    edge = (first[0], first[1] - (bench.y - top - 0.018))
    with ui.drag_contact(first, edge):
        ui.wait_for(
            "the bench heading approaches the pinned intake heading",
            lambda screen: (
                (heading := screen.find("Bench 4")) is not None
                and 0.008 < heading.y - top < 0.043
            ),
        )
        ui.move_contact(edge, (edge[0], edge[1] - 0.081))
        handed_off = ui.wait_for(
            "the bench heading takes over the pinned position",
            lambda screen: (
                (heading := screen.find("Bench 4 pinned")) is not None
                and abs(heading.y - top) < 0.01
                and screen.find("Intake 5 pinned") is None
                and screen.find("Impeller check") is not None
                and screen.find("Entries 13") is not None
            ),
        )
        time.sleep(0.15)
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "log-anchored.png").write_bytes(restored.png)
    (ui.artifacts / "log-bench-pinned.png").write_bytes(handed_off.png)


def exercise_bulletin(ui: Emulator) -> None:
    entry = ui.visible("XRack Inventory", "Window active", "Open bulletin", timeout=75)
    for attempt in range(3):
        ui.tap(entry, "Open bulletin")
        try:
            ui.visible("Calibration notes", timeout=8)
            break
        except AssertionError:
            if attempt == 2:
                raise

    def point(screen: Screen, text: str) -> tuple[float, float]:
        label = screen.require(text)
        return label.x, label.y

    def tap(screen: Screen, text: str) -> None:
        touch_cell(ui, point(screen, text))

    initial = ui.visible(
        "Calibration", "Procedure", "Limits", "Links 0", "No selection"
    )
    tap(initial, "Procedure")
    first = ui.visible("Target procedure", "Links 1", "Limits")
    tap(first, "Limits")
    second = ui.visible("Target limits", "Links 2", "Compact text")

    tap(second, "Compact text")
    cropped = ui.wait_for(
        "both links disappear when the document is limited to one line",
        lambda screen: (
            screen.find("Show full text") is not None
            and screen.find("Links 2") is not None
            and screen.find("Procedure") is None
            and screen.find("Limits") is None
        ),
    )
    touch_cell(ui, point(initial, "Procedure"))
    touch_cell(ui, point(initial, "Limits"))
    tap(cropped, "Show full text")
    restored = ui.visible("Procedure", "Limits", "Links 2", "Target limits")
    tap(restored, "Procedure")
    ready = ui.visible("Target procedure", "Links 3", "Limits", "No selection")

    def copy_word(screen: Screen, word: str) -> Screen:
        with ui.press_contact(point(screen, word)):
            ui.visible(f"Selected {word}", "Target procedure", "Links 3")
        menu = ui.visible("Copy")
        tap(menu, "Copy")
        return ui.wait_for(
            "copy closes the read-only selection menu",
            lambda screen: (
                screen.find("Copy") is None and screen.find("Links 3") is not None
            ),
        )

    copied = copy_word(ready, "Limits")
    tap(copied, "Paste copied word")
    ctrl(ui, "V")
    pasted = ui.visible("Pasted Limits", "Links 3", "Procedure")

    copied_again = copy_word(pasted, "Procedure")
    field = next(
        label
        for label in copied_again.labels
        if label.text == "Limits"
        and label.y > copied_again.require("Target procedure").y
    )
    touch_cell(ui, (field.x, field.y))
    ctrl(ui, "A")
    ctrl(ui, "V")
    replaced = ui.visible("Pasted Procedure", "Links 3", "Selected Procedure")
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "bulletin-pasted.png").write_bytes(replaced.png)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Exercise Android table gestures and IME"
    )
    parser.add_argument("--serial", required=True)
    parser.add_argument("--adb", required=True, type=Path)
    parser.add_argument("--apk", required=True, type=Path)
    parser.add_argument("--keyboard-apk", type=Path)
    scenario = parser.add_mutually_exclusive_group()
    scenario.add_argument("--tree-only", action="store_true")
    scenario.add_argument("--split-only", action="store_true")
    scenario.add_argument("--transfer-only", action="store_true")
    scenario.add_argument("--canvas-only", action="store_true")
    scenario.add_argument("--catalog-only", action="store_true")
    scenario.add_argument("--log-only", action="store_true")
    scenario.add_argument("--bulletin-only", action="store_true")
    args = parser.parse_args()
    if (
        not (
            args.tree_only
            or args.split_only
            or args.transfer_only
            or args.canvas_only
            or args.catalog_only
            or args.log_only
            or args.bulletin_only
        )
        and args.keyboard_apk is None
    ):
        parser.error("Table editing requires --keyboard-apk")
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
        ui = TableEmulator(args.adb.resolve(), args.serial, recognize, artifacts)
        rotation = ui.call("shell", "cmd", "window", "user-rotation").decode().split()
        try:
            ui.call("shell", "cmd", "window", "user-rotation", "lock", "0")
            ui.call("install", "-r", str(args.apk.resolve(strict=True)))
            ui.call("shell", "am", "force-stop", PACKAGE)
            ui.call("shell", "am", "start", "-W", "-f", "0x10008000", "-n", ACTIVITY)
            if args.tree_only:
                exercise_directory(ui)
            elif args.split_only:
                exercise_split(ui)
            elif args.transfer_only:
                exercise_transfer(ui)
            elif args.canvas_only:
                exercise_canvas(ui)
            elif args.catalog_only:
                exercise_catalog(ui)
            elif args.log_only:
                exercise_inspection_log(ui)
            elif args.bulletin_only:
                exercise_bulletin(ui)
            else:
                exercise(ui)
                with test_input_method(ui, args.keyboard_apk.resolve(strict=True)):
                    exercise_cell_edit(ui)
        finally:
            ui.call("shell", "cmd", "window", "user-rotation", *rotation)


if __name__ == "__main__":
    main()
