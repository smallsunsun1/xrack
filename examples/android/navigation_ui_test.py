import argparse
import json
import os
import re
import subprocess
import tempfile
import time
from pathlib import Path

from examples.android.controls_ui_test import (
    Surface,
    drag,
    drag_contact,
    keyboard_visible,
)
from examples.android.simulator_ui_test import (
    Emulator,
    Label,
    Screen,
    normalize,
    pixels_rgb,
    tap_leap_day,
    test_input_method,
)


PACKAGE = "dev.xrack.android_navigation"
ACTIVITY = f"{PACKAGE}/dev.xrack.platform.XRackActivity"


class NavigationEmulator(Emulator):
    def screen(self) -> Screen:
        screen = super().screen()
        labels: list[Label] = []
        for label in screen.labels:
            match = re.fullmatch(
                r"((?:Tab|Swipe|Detail|Pump|Valve|Meter)\s+total|Depth|Drawer\s+(?:opens|closes)"
                r"|Mounts|Records|(?:Pressure|Flow)(?: samples)?|(?:North|South) count|Stale"
                r"|Code|Picks|Accepts|Edits|Row|Searches|Opens|Closes|Actions"
                r"|Outside|Calls|Groups|Denied|Entries|Blocks|Routes|Layout clicks|Tab (?:count|moves)"
                r"|(?:Date|Time) commits)"
                r"(?:\s+|(?=-))(-?[0-9OoIl]+)",
                label.text,
                flags=re.IGNORECASE,
            )
            text = label.text
            if match is not None:
                text = (
                    match[1] + " " + match[2].translate(str.maketrans("OoIl", "0011"))
                )
            movement = re.fullmatch(r"Moved ([0-9OoIl]+) to ([0-9OoIl]+)", text)
            if movement is not None:
                digits = str.maketrans("OoIl", "0011")
                text = f"Moved {movement[1].translate(digits)} to {movement[2].translate(digits)}"
            # Vision can group the neighboring close glyph with a tab title.
            tab = re.fullmatch(r"(?:[x×]\s+)?(Pump|Valve|Meter)(?:\s+[x×])?", text)
            if tab is not None:
                text = tab[1]
            # Vision recognizes the radio indicator as a leading O.
            radio = re.fullmatch(r"[O0●○◉]\s+(Balanced|Fast|Quiet)", text)
            if radio is not None:
                text = radio[1]
            # Vision can merge the two status labels on this workshop row.
            focus = re.fullmatch(r"Memo disabled (Focus code|Focus outside)", text)
            if focus is not None:
                labels.append(Label(label.x, label.y, "Memo disabled"))
                text = focus[1]
            labels.append(Label(label.x, label.y, text))
        return Screen(screen.png, screen.width, screen.height, labels)


def coordinates(ui: Emulator, screen: Screen) -> Surface:
    density = ui.call("shell", "wm", "density").decode()
    values = re.findall(r"(?:Physical|Override) density:\s*(\d+)", density)
    if not values:
        raise AssertionError(f"Android did not report its display density: {density}")
    scale = int(values[-1]) / 160
    label = screen.require("Mode Balanced")
    return Surface(
        screen.width, screen.height, scale, label.y * screen.height - 70 * scale
    )


def open_drawer_item(ui: Emulator, initial: Screen, item: str) -> None:
    for attempt in range(3):
        ui.tap(initial, "Open menu")
        try:
            drawer = ui.wait_for(
                f"the open drawer accepts touch on {item}",
                lambda screen: (
                    screen.find("Navigation menu") is not None
                    and screen.find(item) is not None
                    and ui.window_accepts_touch(screen, ACTIVITY)
                ),
                timeout=8,
            )
            break
        except AssertionError:
            if attempt == 2:
                raise
    ui.tap(drawer, item)


def exercise_loader(ui: Emulator, initial: Screen) -> None:
    open_drawer_item(ui, initial, "Open tools")

    def point(screen: Screen, text: str) -> tuple[float, float]:
        label = screen.require(text)
        return label.x, label.y

    def tap(point: tuple[float, float]) -> None:
        with ui.press_contact(point):
            time.sleep(0.12)

    empty = ui.visible(
        "Inspection tools", "Loader null", "Mounts 0", "Records 0", "Load pressure"
    )
    ui.tap(empty, "Load pressure")
    pressure = ui.visible(
        "Loader ready", "Mounts 1", "Record Pressure", "Pressure samples 0"
    )
    tap(point(pressure, "Record Pressure"))
    recorded = ui.visible("Pressure samples 1", "Pressure 31", "Mounts 1", "Records 1")
    ui.tap(recorded, "Switch on press")
    armed = ui.visible("Switch armed", "Record Pressure", "Records 1")
    old_button = point(armed, "Record Pressure")
    with ui.press_contact(old_button):
        flow = ui.visible(
            "Loader ready", "Mounts 2", "Flow samples 0", "Record Flow", "Records 1"
        )
        assert flow.find("Record Pressure") is None
        assert abs(flow.require("Record Flow").y - old_button[1]) < 0.012

    ui.call("shell", "input", "keyevent", "KEYCODE_ENTER")
    activated = ui.visible("Flow samples 1", "Flow 61", "Mounts 2", "Records 2")
    ui.tap(activated, "Missing source")
    missing = ui.wait_for(
        "an unknown source removes the mounted controls",
        lambda screen: (
            all(
                screen.find(text)
                for text in (
                    "Loader error",
                    "No matching tool",
                    "Mounts 2",
                    "Records 2",
                )
            )
            and screen.find("Record Pressure") is None
            and screen.find("Record Flow") is None
        ),
    )
    tap(old_button)
    ui.tap(missing, "Load pressure")
    reset = ui.visible(
        "Loader ready", "Mounts 3", "Pressure samples 0", "Records 2", "Flow 61"
    )
    ui.tap(reset, "Unload tool")
    inactive = ui.visible(
        "Loader null", "Mounts 3", "Records 2", "Choose an inspection tool"
    )
    assert inactive.find("Record Pressure") is None
    tap(old_button)
    ui.tap(inactive, "Load pressure")
    restored = ui.visible(
        "Loader ready", "Mounts 4", "Pressure samples 0", "Records 2", "Flow 61"
    )
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "loader-remounted.png").write_bytes(restored.png)


def exercise_dynamic_cards(ui: Emulator, initial: Screen) -> None:
    open_drawer_item(ui, initial, "Open cards")
    empty = ui.visible(
        "Dynamic cards", "Mounts 0", "Original null", "South null", "Add cards"
    )
    ui.tap(empty, "Add cards")
    ready = ui.visible(
        "Mounts 2", "Original ready", "South ready", "North count 0", "South count 0"
    )
    top = ready.require("North count 0").y
    assert top < ready.require("South count 0").y
    ui.tap(ready, "North count 0")
    ui.tap(ui.visible("North count 1", "North down"), "North down")
    moved = ui.visible("North count 1", "South count 0", "Mounts 2", "North up")
    assert moved.require("North count 1").y > moved.require("South count 0").y
    ui.call("shell", "input", "keyevent", "KEYCODE_ENTER")
    focused = ui.visible("North count 2", "South count 0", "Mounts 2", "North up")
    ui.tap(focused, "North up")
    ui.tap(ui.visible("North count 2", "North down", "Arm removal"), "Arm removal")
    armed = ui.visible("Removal armed", "North count 2")
    north = armed.require("North count 2")
    with ui.press_contact((north.x, north.y)):
        time.sleep(0.12)
    ui.visible("Removal armed", "Original ready", "North count 3")
    time.sleep(0.65)
    still_mounted = ui.visible("Removal armed", "Original ready", "North count 3")
    north = still_mounted.require("North count 3")
    assert abs(north.y - top) < 0.012
    with ui.press_contact((north.x, north.y)):
        removed = ui.visible(
            "Original null", "Mounts 2", "South count 0", "Add fresh card"
        )
        assert removed.find("North count 3") is None
        assert abs(removed.require("South count 0").y - top) < 0.012

    ui.tap(removed, "South count 0")
    ui.tap(
        ui.visible("Original null", "South count 1", "Add fresh card"), "Add fresh card"
    )
    fresh = ui.visible("Mounts 3", "North count 0", "South count 1", "Try old handle")
    assert abs(fresh.require("North count 0").y - top) < 0.012
    ui.tap(fresh, "Try old handle")
    final = ui.wait_for(
        "the expired handle cannot change either live instance",
        lambda screen: (
            all(
                screen.find(text)
                for text in (
                    "Original null",
                    "Mounts 3",
                    "North count 0",
                    "South count 1",
                )
            )
            and any(label.text == "Stale -1" for label in screen.labels)
        ),
    )
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "dynamic-cards-remounted.png").write_bytes(final.png)


def exercise_service_choice(ui: Emulator, initial: Screen, keyboard_apk: Path) -> None:
    with test_input_method(ui, keyboard_apk):
        open_drawer_item(ui, initial, "Open choices")
        entry = ui.visible(
            "Service choices",
            "Code 10",
            "Draft empty",
            "Edits 0",
            "Compose ni",
        )
        ui.tap(entry, "Compose ni")
        ui.tap(ui.visible("Compose ni sent", "Backspace key"), "Backspace key")
        composing = ui.visible(
            "Editor buffer n",
            "Code 10",
            "Draft empty",
            "Edits 0",
            "Picks 0",
            "Accepts 0",
            "Commit number",
        )
        ui.tap(composing, "Commit number")
        edited = ui.visible("Draft 47", "Code 10", "Edits 1", "Choices", "Accepts 0")
        ui.tap(edited, "Choices")

        def popup_row(screen: Screen, text: str) -> Label | None:
            trigger = screen.find("Choices")
            if trigger is None:
                return None
            return next(
                (
                    label
                    for label in screen.labels
                    if normalize(label.text) == normalize(text)
                    and abs(label.y - trigger.y) > 0.025
                ),
                None,
            )

        def choices(screen: Screen) -> bool:
            return (
                all(
                    screen.find(text)
                    for text in ("Code 10", "Picks 0", "Row 2", "Compose ni")
                )
                and popup_row(screen, "47 Hold") is not None
                and popup_row(screen, "47 Service") is not None
            )

        def tap_row(screen: Screen, text: str) -> None:
            row = popup_row(screen, text)
            assert row is not None, f"Missing popup option {text}"
            ui.touch(round(row.x * screen.width), round(row.y * screen.height))

        menu = ui.wait_for(
            "the enabled prefix match is highlighted above the keyboard", choices
        )
        tap_row(menu, "47 Hold")
        unchanged = ui.wait_for(
            "touching the disabled option keeps the original choice", choices
        )
        tap_row(unchanged, "47 Service")
        picked = ui.visible(
            "Code 30",
            "Picks 1",
            "Accepts 0",
            "Draft 47 Service",
            "Selected finished",
            "Commit Unicode",
        )
        ui.call("shell", "input", "keycombination", "KEYCODE_CTRL_LEFT", "KEYCODE_A")
        ui.tap(picked, "Commit Unicode")
        unicode = ui.visible("Unicode draft", "Code 30", "Accepts 0", "Submit")
        ui.tap(unicode, "Submit")
        accepted = ui.visible(
            "Unicode accepted", "Code 80", "Picks 1", "Accepts 1", "Accepted finished"
        )
        ui.artifacts.mkdir(parents=True, exist_ok=True)
        (ui.artifacts / "choice-ime-popup.png").write_bytes(menu.png)
        (ui.artifacts / "choice-ime-accepted.png").write_bytes(accepted.png)


def exercise_search_responses(ui: Emulator, initial: Screen) -> None:
    open_drawer_item(ui, initial, "Open search")
    ui.visible("Parts search", "Query empty", "Batch none", "Searches 0")
    keyboard_visible(ui, True)
    ui.type_text("me")
    ui.visible("Query me", "Old request queued")
    ui.type_text("t")
    current = ui.visible("Batch met", "Query met", "Meter kit", "Meter body", "Picks 0")
    count = next(
        label.text for label in current.labels if label.text.startswith("Searches ")
    )
    row = current.require("Meter kit")
    with ui.press_contact((row.x, row.y)):
        ui.wait_for(
            "the late batch removes the previous popup while the touch is held",
            lambda screen: (
                all(
                    screen.find(text)
                    for text in ("Batch me", "Query met", "Picks 0", count)
                )
                and all(
                    screen.find(text) is None
                    for text in ("Mesh cover", "Meter kit", "Meter body")
                )
            ),
        )
    ui.visible("Query met", "Batch me", "Picks 0", count)
    ui.type_text("er")
    fresh = ui.visible(
        "Batch meter", "Query meter", "Meter gasket", "Meter sensor", "Picks 0"
    )
    before = next(
        int(label.text.split()[-1])
        for label in fresh.labels
        if label.text.startswith("Searches ")
    )
    ui.tap(fresh, "Meter sensor")
    selected = ui.visible(
        "Query Meter sensor",
        "Sent Meter sensor",
        "Picks 1",
        "Accepts 0",
        f"Searches {before + 1}",
    )
    ui.call("shell", "input", "keycombination", "KEYCODE_CTRL_LEFT", "KEYCODE_Z")
    ui.visible(
        "Query meter",
        "Sent meter",
        "Picks 1",
        "Accepts 0",
        f"Searches {before + 2}",
        "Meter sensor",
    )
    ui.call("shell", "input", "keyevent", "KEYCODE_ENTER")
    submitted = ui.visible(
        "Query meter", "Sent meter", "Picks 1", "Accepts 1", f"Searches {before + 3}"
    )
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "search-result-selected.png").write_bytes(selected.png)
    (ui.artifacts / "search-query-submitted.png").write_bytes(submitted.png)


def exercise_anchored_popup(
    ui: Emulator, initial: Screen
) -> list[dict[str, str | int | bool]]:
    def count(screen: Screen, prefix: str) -> int:
        return next(
            int(label.text.removeprefix(prefix))
            for label in screen.labels
            if label.text.startswith(prefix)
            and label.text.removeprefix(prefix).isdigit()
        )

    open_drawer_item(ui, initial, "Open popup")
    ready = ui.visible(
        "Popup workshop", "Opens 0", "Closes 0", "Count outside", "Show actions"
    )
    outside = ready.require("Count outside")
    ui.tap(ready, "Show actions")
    below = ui.visible(
        "Inspection actions", "Move anchor", "Show actions", "Opens 1", "Closes 0"
    )
    first = below.require("Inspection actions")
    assert first.y > below.require("Show actions").y + 0.035
    ui.tap(below, "Move anchor")
    flipped = ui.wait_for(
        "the open popup shifts horizontally and flips above the lower anchor",
        lambda screen: (
            all(screen.find(text) for text in ("Opens 1", "Closes 0", "Apply action"))
            and (panel := screen.find("Inspection actions")) is not None
            and (anchor := screen.find("Show actions")) is not None
            and panel.x > first.x + 0.23
            and panel.y < anchor.y - 0.1
        ),
    )
    ui.tap(flipped, "Apply action")
    applied = ui.visible("Inspection actions", "Actions 1", "Opens 1", "Closes 0")
    ui.tap(applied, "Count outside")

    def closed(count: int, outside_count: int, mode: str) -> Screen:
        return ui.wait_for(
            f"the workshop stays open after popup dismissal {count}",
            lambda screen: (
                all(
                    screen.find(text)
                    for text in (
                        "Popup workshop",
                        f"Opens {count}",
                        f"Closes {count}",
                        f"Outside {outside_count}",
                        "Actions 1",
                        mode,
                        "Show actions",
                    )
                )
                and screen.find("Inspection actions") is None
            ),
        )

    passed_through = closed(1, 1, "Modal popup")
    ui.tap(passed_through, "Modal popup")
    ui.tap(ui.visible("Modal on", "Show actions"), "Show actions")
    modal = ui.visible("Inspection actions", "Opens 2", "Closes 1")
    ui.touch(round(outside.x * modal.width), round(outside.y * modal.height))
    blocked = closed(2, 1, "Modal on")
    ui.tap(blocked, "Show actions")
    ui.visible("Inspection actions", "Opens 3", "Closes 2")
    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
    modal_back = closed(3, 1, "Modal on")
    ui.tap(modal_back, "Modal on")
    ui.tap(ui.visible("Modal popup", "Show actions"), "Show actions")
    ui.visible("Inspection actions", "Opens 4", "Closes 3")
    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
    before_enter = closed(4, 1, "Modal popup")
    ui.call("shell", "input", "keyevent", "KEYCODE_ENTER")
    reopened = ui.visible("Inspection actions", "Opens 5", "Closes 4")
    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
    popup_back = closed(5, 1, "Modal popup")
    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
    ui.wait_for(
        "Back closes the workshop and leaves navigation at home",
        lambda screen: (
            all(
                screen.find(text)
                for text in ("Mode Balanced", "Route home", "Open menu")
            )
            and screen.find("Popup workshop") is None
        ),
    )
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "popup-flipped.png").write_bytes(flipped.png)
    (ui.artifacts / "popup-back.png").write_bytes(popup_back.png)
    return [
        {
            "step": "open",
            "below_anchor": first.y > below.require("Show actions").y,
            "opens": count(below, "Opens "),
        },
        {
            "step": "move_anchor",
            "above_anchor": flipped.require("Inspection actions").y
            < flipped.require("Show actions").y,
            "opens": count(flipped, "Opens "),
            "closes": count(flipped, "Closes "),
        },
        {
            "step": "dismiss_outside",
            "actions": count(passed_through, "Actions "),
            "outside": count(passed_through, "Outside "),
            "closes": count(passed_through, "Closes "),
            "popup_visible": passed_through.find("Inspection actions") is not None,
        },
        {
            "step": "back_then_enter",
            "open_delta": count(reopened, "Opens ") - count(before_enter, "Opens "),
            "popup_visible": reopened.find("Inspection actions") is not None,
        },
    ]


def compare_popup_checkpoints(
    ui: Emulator, parity: Path, android: list[dict[str, str | int | bool]]
) -> None:
    with tempfile.TemporaryDirectory() as directory:
        desktop_path = Path(directory) / "desktop.json"
        subprocess.run(
            [
                parity,
                "anchored_popup_repositions_and_restores_focus_in_both_backends",
                "--exact",
            ],
            env={**os.environ, "XRACK_POPUP_REPLAY_REPORT": str(desktop_path)},
            check=True,
            timeout=45,
        )
        desktop = json.loads(desktop_path.read_text(encoding="utf-8"))
    report = {**desktop, "android": android}
    (ui.artifacts / "popup-replay-comparison.json").write_text(
        json.dumps(report, indent=2) + "\n", encoding="utf-8"
    )
    for backend, observed in desktop.items():
        assert observed == android, (
            f"{backend} and Android differ: {observed!r} != {android!r}"
        )
    print(
        "PASS desktop AOT, desktop interpreter and Android agree at the popup checkpoints",
        flush=True,
    )


def exercise_workshop_menu(ui: Emulator, initial: Screen) -> None:
    frame = coordinates(ui, initial)
    open_drawer_item(ui, initial, "Open commands")
    ready = ui.visible(
        "Command workshop",
        "Commands",
        "Mode detail",
        "Tool detail",
        "Calls 0",
        "Groups 0",
    )
    ui.tap(ready, "Commands")
    menu = ui.visible("Unavailable", "View mode")
    row_height = (
        (menu.require("View mode").y - menu.require("Unavailable").y)
        * menu.height
        / frame.scale
    )
    assert abs(row_height - 44) < 4, (
        f"A menu opened by touch has {row_height:.1f}px rows"
    )
    ui.tap(menu, "Unavailable")
    ui.tap(ui.visible("Unavailable", "View mode", "Calls 0"), "View mode")
    submenu = ui.visible("Detailed", "Compact")
    child_height = (
        (submenu.require("Compact").y - submenu.require("Detailed").y)
        * submenu.height
        / frame.scale
    )
    assert abs(child_height - 44) < 4, f"A touch submenu has {child_height:.1f}px rows"
    ui.tap(submenu, "Compact")
    selected = ui.visible(
        "Mode compact",
        "Tool compact",
        "Calls 1",
        "Groups 1",
        "Order ok",
        "Denied 0",
        "Detail tool",
    )
    ui.tap(selected, "Detail tool")
    ui.visible("Mode detail", "Tool detail", "Calls 2", "Groups 2", "Order ok")
    ui.call("shell", "input", "keycombination", "KEYCODE_CTRL_LEFT", "KEYCODE_2")
    ui.visible("Mode compact", "Tool compact", "Calls 3", "Groups 3", "Order ok")
    ui.call("shell", "input", "keycombination", "KEYCODE_CTRL_RIGHT", "KEYCODE_1")
    shortcut = ui.visible(
        "Mode detail", "Tool detail", "Calls 4", "Groups 4", "Order ok", "Lock modes"
    )
    ui.tap(shortcut, "Lock modes")
    locked = ui.visible("Modes locked", "Commands")
    ui.call("shell", "input", "keycombination", "KEYCODE_CTRL_RIGHT", "KEYCODE_2")
    ui.tap(locked, "Commands")
    ui.tap(ui.visible("View mode"), "View mode")
    disabled = ui.visible("Detailed", "Compact")
    ui.tap(disabled, "Detailed")
    ui.visible("Detailed", "Compact")
    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
    ui.wait_for(
        "Back closes just the submenu",
        lambda screen: (
            screen.find("View mode") is not None
            and screen.find("Detailed") is None
            and screen.find("Compact") is None
        ),
    )
    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
    closed = ui.wait_for(
        "the next Back closes the menu and keeps the workshop open",
        lambda screen: (
            all(
                screen.find(text)
                for text in (
                    "Command workshop",
                    "Mode detail",
                    "Calls 4",
                    "Groups 4",
                    "Order ok",
                    "Denied 0",
                )
            )
            and screen.find("View mode") is None
        ),
    )
    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
    ui.wait_for(
        "the last Back leaves the workshop",
        lambda screen: (
            screen.find("Route home") is not None
            and screen.find("Command workshop") is None
        ),
    )
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "touch-menu-submenu.png").write_bytes(submenu.png)
    (ui.artifacts / "touch-menu-complete.png").write_bytes(closed.png)


def exercise_focus_routes(ui: Emulator, initial: Screen) -> None:
    open_drawer_item(ui, initial, "Open focus")
    ready = ui.visible("Focus workshop", "Focus outside", "Enter fields", "Entries 0")
    enter_position = ready.require("Enter fields").y

    def at_rest(*texts: str) -> Screen:
        def settled(screen: Screen) -> bool:
            enter = screen.find("Enter fields")
            return (
                enter is not None
                and abs(enter.y - enter_position) < 0.012
                and all(screen.find(text) for text in texts)
            )

        return ui.wait_for(
            "focus workshop settles after the IME closes: " + ", ".join(texts),
            settled,
        )

    ui.tap(ready, "Enter fields")
    ui.visible("Focus code", "Code empty", "Memo empty", "Entries 1")
    keyboard_visible(ui, True)

    def press(key: str) -> None:
        ui.call("shell", "input", "keyevent", f"KEYCODE_{key}")

    press("X")
    ui.visible("Focus code", "Code empty", "Key blocked", "Blocks 1", "Routes 0")
    press("A")
    ui.visible("Focus code", "Code a", "Memo empty")
    press("TAB")
    ui.visible("Focus memo", "Entries 1")
    press("F2")
    routed = ui.visible("Focus memo", "F2 routed", "Blocks 1", "Routes 1", "Entries 1")
    for shift in ("KEYCODE_SHIFT_LEFT", "KEYCODE_SHIFT_RIGHT"):
        ui.call("shell", "input", "keycombination", shift, "KEYCODE_TAB")
        ui.visible("Focus code", "Entries 1")
        press("TAB")
        ui.visible("Focus memo", "Entries 1")
    ui.tap(ui.visible("Focus memo", "Leave fields"), "Leave fields")
    keyboard_visible(ui, False)
    press("F2")
    ui.tap(at_rest("Focus outside", "Entries 1"), "Enter fields")
    ui.visible("Focus memo", "Blocks 1", "Routes 1", "Entries 2")
    press("HOME")
    ui.wait_for(
        "Home leaves the focus workshop in the background",
        lambda screen: screen.find("Focus workshop") is None,
    )
    ui.call("shell", "am", "start", "-W", "-n", ACTIVITY)
    ui.visible("Focus workshop", "Focus memo", "Code a", "Memo empty")
    press("B")
    resumed = ui.visible("Focus memo", "Code a", "Memo b", "Disable memo")
    ui.tap(resumed, "Disable memo")
    ui.tap(at_rest("Focus outside", "Memo disabled"), "Enter fields")
    fallback = ui.visible("Focus code", "Code a", "Memo b", "Memo disabled")
    ui.tap(fallback, "Leave fields")
    ui.visible("Focus outside")
    keyboard_visible(ui, False)
    press("BACK")
    ui.wait_for(
        "Back returns from the focus workshop",
        lambda screen: (
            screen.find("Route home") is not None
            and screen.find("Focus workshop") is None
        ),
    )
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "focus-key-route.png").write_bytes(routed.png)
    (ui.artifacts / "focus-disabled-fallback.png").write_bytes(fallback.png)


def exercise_layout_workshop(ui: Emulator, initial: Screen) -> None:
    open_drawer_item(ui, initial, "Open layouts")

    def row(screen: Screen, *labels: str) -> None:
        items = [screen.require(text) for text in labels]
        assert max(item.y for item in items) - min(item.y for item in items) < 0.009, (
            f"Expected one visible row for {labels}"
        )
        assert all(left.x < right.x for left, right in zip(items, items[1:])), (
            f"Unexpected left-to-right position for {labels}"
        )

    def state(clicks: int, picked: str, *labels: str) -> Screen:
        return ui.visible(
            "Layout workshop", f"Layout clicks {clicks}", f"Picked {picked}", *labels
        )

    all_cells = (
        "Grid pump",
        "Grid valve",
        "Flow ash",
        "Flow bay",
        "Flow fir",
        "Flex red",
        "Flex blue",
        "Flex gold",
    )
    wide = state(0, "None", "Mirror rows", "Narrow rows", *all_cells)
    row(wide, "Grid pump", "Grid valve")
    row(wide, "Flow ash", "Flow bay", "Flow fir")
    row(wide, "Flex gold", "Flex red", "Flex blue")
    first_grid = wide.require("Grid pump")
    original_cell = round(first_grid.x * wide.width), round(first_grid.y * wide.height)
    ui.touch(*original_cell)
    ui.tap(state(1, "Grid pump", "Flex blue"), "Flex blue")
    state(2, "Flex blue")
    ui.call("shell", "input", "keyevent", "KEYCODE_TAB")
    ui.call("shell", "input", "keyevent", "KEYCODE_ENTER")
    ui.tap(state(3, "Flex red", "Mirror rows"), "Mirror rows")

    mirrored = state(3, "Flex red", "Use LTR", *all_cells)
    row(mirrored, "Grid valve", "Grid pump")
    row(mirrored, "Flow fir", "Flow bay", "Flow ash")
    row(mirrored, "Flex blue", "Flex red", "Flex gold")
    ui.touch(*original_cell)
    ui.tap(state(4, "Grid valve", "Narrow rows"), "Narrow rows")

    wrapped = state(4, "Grid valve", "Widen rows", *all_cells)
    row(wrapped, "Flow bay", "Flow ash")
    row(wrapped, "Flex blue", "Flex red")
    for leading, following in (("Flow ash", "Flow fir"), ("Flex blue", "Flex gold")):
        head = wrapped.require(leading)
        tail = wrapped.require(following)
        assert tail.y - head.y > 0.04, f"{following} did not wrap below {leading}"
        assert abs(tail.x - head.x) < 0.022, f"{following} lost its line alignment"
    ui.tap(wrapped, "Flow fir")
    ui.tap(state(5, "Flow fir", "Flex gold"), "Flex gold")
    ui.tap(state(6, "Flex gold", "Widen rows"), "Widen rows")
    restored = state(6, "Flex gold", "Narrow rows", *all_cells)
    row(restored, "Flow fir", "Flow bay", "Flow ash")
    row(restored, "Flex blue", "Flex red", "Flex gold")
    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
    ui.wait_for(
        "Back returns from the layout workshop",
        lambda screen: (
            screen.find("Route home") is not None
            and screen.find("Layout workshop") is None
        ),
    )
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "layout-wide-ltr.png").write_bytes(wide.png)
    (ui.artifacts / "layout-wrapped-rtl.png").write_bytes(wrapped.png)


def exercise_document_tabs(ui: Emulator, initial: Screen) -> None:
    frame = coordinates(ui, initial)
    open_drawer_item(ui, initial, "Open cards")
    ui.tap(ui.visible("Dynamic cards", "Open tabs"), "Open tabs")
    ui.artifacts.mkdir(parents=True, exist_ok=True)

    def position(screen: Screen, title: str) -> tuple[int, int]:
        label = screen.require(title)
        return round(label.x * screen.width), round(label.y * screen.height)

    def order(screen: Screen, *titles: str) -> None:
        locations = [screen.require(title).x for title in titles]
        assert all(left < right for left, right in zip(locations, locations[1:])), (
            titles
        )

    ready = ui.visible(
        "Document tabs",
        "Pump",
        "Valve",
        "Meter",
        "Active Valve",
        "Tab count 3",
        "Tab moves 0",
        "No move",
        "Add Valve",
    )
    order(ready, "Pump", "Valve", "Meter")
    ui.tap(ready, "Add Valve")
    before = ui.visible("Valve total 1", "Valve", "Meter", "Tab moves 0")
    start, end = position(before, "Valve"), position(before, "Meter")
    with drag_contact(ui, frame, start, end):
        holding = ui.visible("Active Valve", "Tab moves 0", "Valve total 1")
        (ui.artifacts / "tabs-during-drag.png").write_bytes(holding.png)
        ui.call("shell", "input", "keyevent", "KEYCODE_ESCAPE")
    unchanged = ui.visible(
        "Document tabs",
        "Pump",
        "Valve",
        "Meter",
        "Tab moves 0",
        "No move",
        "Valve total 1",
    )
    order(unchanged, "Pump", "Valve", "Meter")

    with drag_contact(
        ui, frame, position(unchanged, "Valve"), position(unchanged, "Meter")
    ):
        ui.visible("Document tabs", "Active Valve", "Tab moves 0", "Valve total 1")
        ui.call("shell", "input", "keyevent", "KEYCODE_HOME")
        ui.wait_for(
            "Android backgrounds the document tabs while Valve is held",
            lambda screen: (
                screen.find("Document tabs") is None
                and screen.find("XRack Navigation") is None
            ),
        )
    ui.call("shell", "am", "start", "-W", "-n", ACTIVITY)
    resumed = ui.wait_for(
        "the interrupted reorder leaves the original tabs and selection intact",
        lambda screen: (
            all(
                screen.find(title)
                for title in (
                    "Document tabs",
                    "Pump",
                    "Valve",
                    "Meter",
                    "Active Valve",
                    "Valve total 1",
                    "Tab count 3",
                    "Tab moves 0",
                    "No move",
                )
            )
            and ui.window_accepts_touch(screen, ACTIVITY)
        ),
        timeout=75,
    )
    order(resumed, "Pump", "Valve", "Meter")

    drag(ui, frame, position(resumed, "Valve"), position(resumed, "Meter"))
    reordered = ui.visible(
        "Active Valve",
        "Valve total 1",
        "Tab moves 1",
        "Moved 1 to 2",
        "Pump",
        "Meter",
        "Valve",
    )
    order(reordered, "Pump", "Meter", "Valve")
    meter_x, meter_y = position(reordered, "Meter")
    # The three headers fill the 298px row evenly. Meter's close glyph
    # is 56 logical pixels to the right of the center of its title.
    ui.touch(meter_x + round(56 * frame.scale), meter_y)
    background_closed = ui.wait_for(
        "closing a background tab leaves the Valve page and counter active",
        lambda screen: (
            all(
                screen.find(text)
                for text in (
                    "Closed Meter",
                    "Active Valve",
                    "Valve total 1",
                    "Tab count 2",
                    "Tab moves 1",
                )
            )
            and screen.find("Meter") is None
        ),
    )
    ui.call("shell", "input", "keyevent", "KEYCODE_SPACE")
    ui.tap(ui.visible("Valve total 2", "Valve"), "Valve")
    ui.call("shell", "input", "keyevent", "KEYCODE_FORWARD_DEL")
    final = ui.visible(
        "Closed Valve", "Active Pump", "Pump total 0", "Tab count 1", "Tab moves 1"
    )
    ui.tap(final, "Add Pump")
    ui.visible("Pump total 1", "Active Pump")
    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
    ui.wait_for(
        "Back returns to the cards workshop",
        lambda screen: (
            screen.find("Dynamic cards") is not None
            and screen.find("Document tabs") is None
        ),
    )
    (ui.artifacts / "tabs-reordered.png").write_bytes(reordered.png)
    (ui.artifacts / "tabs-background-closed.png").write_bytes(background_closed.png)
    (ui.artifacts / "tabs-current-closed.png").write_bytes(final.png)


def exercise_calendar_fields(ui: Emulator, initial: Screen) -> None:
    frame = coordinates(ui, initial)
    open_drawer_item(ui, initial, "Open layouts")
    ui.tap(ui.visible("Layout workshop", "Open fields"), "Open fields")
    ui.visible("Date and time fields", "Stored date 2024-02-28", "Stored time 12:30:41")

    def press(key: str) -> None:
        ui.call("shell", "input", "keyevent", f"KEYCODE_{key}")

    def touch_field(caption: str, *, picker: bool = False) -> None:
        # The time caption is about 30 logical pixels below the center of
        # the full screen after the keyboard's dismissal animation ends.
        screen = ui.wait_for(
            f"the fields are ready for {caption}",
            lambda screen: (
                screen.find(caption) is not None
                and (
                    not picker
                    or (
                        (bottom := screen.find("Visit time")) is not None
                        and abs((bottom.y - 0.5) * screen.height / frame.scale - 30)
                        < 24
                    )
                )
            ),
        )
        label = screen.require(caption)
        # Each imported field places its 100px picker button on the right.
        x = screen.width / 2 + 93 * frame.scale if picker else label.x * screen.width
        ui.touch(
            round(x),
            round(label.y * screen.height + 39 * frame.scale),
        )
        if not picker:
            keyboard_visible(ui, True)

    def confirm_picker() -> None:
        screen = ui.screen()
        # The portrait picker footer's accept button is the bottommost
        # primary-color control in the trailing half of the popup.
        points = tuple(
            (x, y)
            for y in range(screen.height // 2, screen.height * 7 // 8, 12)
            for x in range(screen.width * 2 // 3, screen.width * 15 // 16, 12)
        )
        primary = [
            point
            for point, color in zip(points, pixels_rgb(screen.png, points))
            if color == (52, 86, 202)
        ]
        assert primary, "The picker did not present an enabled confirm button"
        bottom = max(y for _, y in primary)
        candidates = [(x, y) for x, y in primary if bottom - 60 <= y <= bottom - 24]
        assert candidates, "The picker confirmation did not have a full touch area"
        ui.touch(
            round(sum(x for x, _ in candidates) / len(candidates)),
            round(sum(y for _, y in candidates) / len(candidates)),
        )

    def replace_draft(digits: str) -> None:
        ui.call("shell", "input", "keycombination", "KEYCODE_CTRL_LEFT", "KEYCODE_A")
        ui.type_text(digits)

    def hide_keyboard() -> None:
        if b"mInputShown=true" in ui.call("shell", "dumpsys", "input_method"):
            press("BACK")
        keyboard_visible(ui, False)

    def choose_next_minute() -> None:
        ui.tap(ui.visible("44", "45", "46"), "46")
        ui.wait_for(
            "minute 46 is selected in the picker",
            lambda screen: (
                (minute := screen.find("46")) is not None
                and (hour := screen.find("13")) is not None
                and abs(minute.y - hour.y) < 0.015
            ),
        )

    touch_field("Visit date")
    replace_draft("20240302")
    invalid_date = ui.visible("Date draft 2024-03-02", "Date blocked", "Date commits 0")
    press("ENTER")
    ui.visible(
        "Stored date 2024-02-28",
        "Date draft 2024-03-02",
        "Date blocked",
        "Date commits 0",
    )
    press("ESCAPE")
    ui.visible("Date draft 2024-02-28", "Date ready", "Date commits 0")
    replace_draft("20240227")
    ui.visible("Stored date 2024-02-28", "Date draft 2024-02-27", "Date ready")
    press("ENTER")
    ui.visible("Stored date 2024-02-27", "Date commits 1")
    hide_keyboard()
    touch_field("Visit date", picker=True)
    calendar = ui.visible("2024-02", "29")
    tap_leap_day(ui, calendar)
    confirm_picker()
    ui.visible("Stored date 2024-02-29", "Date draft 2024-02-29", "Date commits 2")

    touch_field("Visit time")
    replace_draft("2359")
    ui.visible("Time draft 23:59", "Time blocked", "Time commits 0")
    press("ENTER")
    ui.visible("Stored time 12:30:41", "Time draft 23:59", "Time commits 0")
    press("ESCAPE")
    ui.visible("Time draft 12:30", "Time ready", "Time commits 0")
    replace_draft("1345")
    ui.visible("Stored time 12:30:41", "Time draft 13:45", "Time ready")
    press("ENTER")
    ui.visible("Stored time 13:45:41", "Time commits 1")
    hide_keyboard()

    touch_field("Visit time", picker=True)
    choose_next_minute()
    press("BACK")
    ui.visible("Stored time 13:45:41", "Time draft 13:45", "Time commits 1")
    touch_field("Visit time", picker=True)
    choose_next_minute()
    confirm_picker()
    final = ui.visible(
        "Stored time 13:46:41", "Time draft 13:46", "Time commits 2", "Date commits 2"
    )
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "date-field-rejected.png").write_bytes(invalid_date.png)
    (ui.artifacts / "calendar-fields-committed.png").write_bytes(final.png)


def exercise(
    ui: Emulator,
    apk: Path,
    *,
    loader_only: bool = False,
    dynamic_cards_only: bool = False,
    combo_keyboard_apk: Path | None = None,
    search_results_only: bool = False,
    popup_window_only: bool = False,
    popup_parity: Path | None = None,
    command_menu_only: bool = False,
    focus_routes_only: bool = False,
    layouts_only: bool = False,
    document_tabs_only: bool = False,
    calendar_fields_only: bool = False,
) -> None:
    rotation = ui.call("shell", "cmd", "window", "user-rotation").decode().split()
    try:
        ui.call("shell", "cmd", "window", "user-rotation", "lock", "0")
        ui.call("install", "-r", str(apk))
        ui.call("shell", "am", "force-stop", PACKAGE)
        ui.call("shell", "am", "start", "-W", "-f", "0x10008000", "-n", ACTIVITY)
        initial = ui.visible(
            "XRack Navigation",
            "Mode Balanced",
            "Tab total 0",
            "Swipe total 0",
            "Route home",
            timeout=75,
        )
        if loader_only:
            exercise_loader(ui, initial)
            return
        if dynamic_cards_only:
            exercise_dynamic_cards(ui, initial)
            return
        if combo_keyboard_apk is not None:
            exercise_service_choice(ui, initial, combo_keyboard_apk)
            return
        if search_results_only:
            exercise_search_responses(ui, initial)
            return
        if popup_window_only:
            checkpoints = exercise_anchored_popup(ui, initial)
            if popup_parity is not None:
                compare_popup_checkpoints(ui, popup_parity, checkpoints)
            return
        if command_menu_only:
            exercise_workshop_menu(ui, initial)
            return
        if focus_routes_only:
            exercise_focus_routes(ui, initial)
            return
        if layouts_only:
            exercise_layout_workshop(ui, initial)
            return
        if document_tabs_only:
            exercise_document_tabs(ui, initial)
            return
        if calendar_fields_only:
            exercise_calendar_fields(ui, initial)
            return
        frame = coordinates(ui, initial)
        ui.tap(initial, "Fast")
        ui.visible("Mode Fast")
        ui.tap(ui.visible("Mode Fast", "Quiet"), "Quiet")
        time.sleep(0.6)
        ui.tap(ui.visible("Mode Fast", "Balanced"), "Balanced")
        ui.visible("Mode Balanced")

        ui.tap(ui.visible("Add Tab"), "Add Tab")
        ui.tap(ui.visible("Tab total 1", "Settings"), "Settings")
        ui.visible("Settings page")
        ui.tap(ui.visible("Settings page", "Disabled"), "Disabled")
        time.sleep(0.6)
        ui.tap(ui.visible("Settings page", "Overview"), "Overview")
        ui.visible("Tab total 1")

        ui.tap(ui.visible("Add Swipe"), "Add Swipe")
        ui.visible("Swipe total 1")
        drag(ui, frame, frame.point(325, 425), frame.point(80, 425))
        ui.tap(ui.visible("Card Beta", "Second card", "Lock swipes"), "Lock swipes")
        drag(ui, frame, frame.point(325, 425), frame.point(80, 425))
        ui.touch(*frame.point(308, 504))
        time.sleep(0.6)
        ui.tap(ui.visible("Card Beta", "Second card", "Lock swipes"), "Lock swipes")
        drag(ui, frame, frame.point(80, 425), frame.point(325, 425))
        ui.visible("Card Alpha", "Swipe total 1")
        ui.touch(*frame.point(308, 504))
        ui.visible("Card Gamma", "Third card")
        ui.touch(*frame.point(272, 504))
        ui.visible("Card Alpha", "Swipe total 1")

        ui.tap(ui.visible("Open details"), "Open details")
        review_x, _ = frame.point(290, 632)
        detail = ui.wait_for(
            "the detail page reaches its final position",
            lambda screen: (
                all(
                    screen.find(text)
                    for text in ("Route details", "Depth 2", "Add Detail")
                )
                and (review := screen.find("Review")) is not None
                and abs(review.x * frame.width - review_x) < 4 * frame.scale
            ),
        )
        ui.tap(detail, "Add Detail")
        ui.tap(ui.visible("Detail total 1", "Review"), "Review")
        ui.visible("Review page", "Depth 3")
        ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
        ui.wait_for(
            "the return transition settles on the detail page",
            lambda screen: (
                all(
                    screen.find(text)
                    for text in ("Route details", "Depth 2", "Detail total 1", "Review")
                )
                and screen.find("Review page") is None
                and screen.find("Back to detail") is None
            ),
        )

        drag(ui, frame, frame.point(4, 430), frame.point(230, 430))
        drawer = ui.visible("Navigation menu", "Drawer opens 1", "Show help")
        ui.artifacts.mkdir(parents=True, exist_ok=True)
        (ui.artifacts / "navigation-drawer.png").write_bytes(drawer.png)

        def drawer_closed(count: int) -> Screen:
            return ui.wait_for(
                f"drawer close {count} leaves the detail page visible",
                lambda screen: (
                    all(
                        screen.find(label)
                        for label in (
                            "Route details",
                            "Depth 2",
                            f"Drawer closes {count}",
                            "Open menu",
                        )
                    )
                    and screen.find("Navigation menu") is None
                    and screen.find("Navigation help") is None
                ),
            )

        ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
        ui.tap(drawer_closed(1), "Open menu")
        first_help = drawer.require("Show help")
        second_drawer = ui.wait_for(
            "the second drawer opening settles at its final position",
            lambda screen: (
                screen.find("Navigation menu") is not None
                and (help := screen.find("Show help")) is not None
                and abs(help.x - first_help.x) < 0.02
                and abs(help.y - first_help.y) < 0.01
            ),
        )
        ui.tap(second_drawer, "Show help")
        help_dialog = ui.visible("Navigation help", "Close help")
        (ui.artifacts / "navigation-help.png").write_bytes(help_dialog.png)
        ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
        ui.wait_for(
            "Back closes help while leaving the drawer open",
            lambda screen: (
                screen.find("Navigation menu") is not None
                and screen.find("Show help") is not None
                and screen.find("Navigation help") is None
                and screen.find("Close help") is None
            ),
        )
        ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
        drawer_closed(2)
        ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
        ui.visible("Route home", "Depth 1", "Tab total 1", "Swipe total 1")
        ui.call("shell", "input", "keyevent", "KEYCODE_HOME")
        ui.call("shell", "am", "start", "-W", "-n", ACTIVITY)
        final = ui.visible(
            "Mode Balanced", "Route home", "Tab total 1", "Swipe total 1"
        )
        (ui.artifacts / "navigation.png").write_bytes(final.png)
        print(
            "PASS navigation state, touch paging, drawer and native Back routing",
            flush=True,
        )
    finally:
        ui.call("shell", "cmd", "window", "user-rotation", *rotation)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Exercise Android navigation components"
    )
    parser.add_argument("--serial", required=True)
    parser.add_argument("--apk", required=True, type=Path)
    parser.add_argument("--adb", required=True, type=Path)
    scenario = parser.add_mutually_exclusive_group()
    scenario.add_argument("--loader-only", action="store_true")
    scenario.add_argument("--dynamic-cards-only", action="store_true")
    scenario.add_argument("--combo-ime-only", action="store_true")
    scenario.add_argument("--search-results-only", action="store_true")
    scenario.add_argument("--popup-window-only", action="store_true")
    scenario.add_argument("--command-menu-only", action="store_true")
    scenario.add_argument("--focus-routes-only", action="store_true")
    scenario.add_argument("--layouts-only", action="store_true")
    scenario.add_argument("--document-tabs-only", action="store_true")
    scenario.add_argument("--calendar-fields-only", action="store_true")
    parser.add_argument("--keyboard-apk", type=Path)
    parser.add_argument("--popup-parity", type=Path)
    args = parser.parse_args()
    adb = args.adb.resolve()
    if not adb.is_file() or not args.apk.is_file():
        parser.error("Both adb and the navigation APK must exist")
    if args.combo_ime_only and (
        args.keyboard_apk is None or not args.keyboard_apk.is_file()
    ):
        parser.error("The ComboBox IME test requires an existing --keyboard-apk")
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
        exercise(
            NavigationEmulator(adb, args.serial, recognize, artifacts),
            args.apk.resolve(),
            loader_only=args.loader_only,
            dynamic_cards_only=args.dynamic_cards_only,
            combo_keyboard_apk=args.keyboard_apk.resolve()
            if args.combo_ime_only
            else None,
            search_results_only=args.search_results_only,
            popup_window_only=args.popup_window_only,
            popup_parity=args.popup_parity.resolve(strict=True)
            if args.popup_parity
            else None,
            command_menu_only=args.command_menu_only,
            focus_routes_only=args.focus_routes_only,
            layouts_only=args.layouts_only,
            document_tabs_only=args.document_tabs_only,
            calendar_fields_only=args.calendar_fields_only,
        )


if __name__ == "__main__":
    main()
