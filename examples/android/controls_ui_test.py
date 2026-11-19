import argparse
import os
import re
import subprocess
import tempfile
import time
from collections.abc import Iterator
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path

from examples.android.simulator_ui_test import (
    Emulator,
    Label,
    Screen,
    pixel_rgb,
    pixels_rgb,
    test_input_method,
)


PACKAGE = "dev.xrack.android_controls"
ACTIVITY = f"{PACKAGE}/dev.xrack.platform.XRackActivity"


class ControlsEmulator(Emulator):
    def screen(self) -> Screen:
        screen = super().screen()
        labels: list[Label] = []
        for label in screen.labels:
            # Vision's fast recognizer confuses isolated digits with O, I or l.
            # Limit the correction to the fixture's integer status captions.
            match = re.fullmatch(
                r"(Changes|Releases|Background|Accepted|Rejected"
                r"|Wheel index|Wheel commits|Card code|Card commits"
                r"|Zoom|Angle|Starts|Ends|Cancels|Taps"
                r"|Dial|Dial releases|Released at|Bulk edits|Item edits"
                r"|Folds|Groups|Saves|Presses|Holds|Steps|Step cancels|Step releases"
                r"|Confirmations|Commands|Pairs|Paired after|Other taps"
                r"|Range edits|Range releases|Finishes) ([0-9OoIl]+)",
                label.text,
            )
            text = label.text
            if match is not None:
                text = (
                    match[1] + " " + match[2].translate(str.maketrans("OoIl", "0011"))
                )
            # Vision can include the adjacent checkmark or switch in the caption.
            member = re.fullmatch(
                r"(?:.{1,4}\s+)?((?:Labels|Manifest|Compress|Delivery) (?:on|off)"
                r"|Review (?:on|off|mixed)|Advanced target)",
                text,
            )
            if member is not None:
                text = member[1]
            labels.append(Label(label.x, label.y, text))
        return Screen(screen.png, screen.width, screen.height, labels)


@dataclass(frozen=True)
class Surface:
    width: int
    height: int
    scale: float
    top: float

    def point(self, x: float, y: float) -> tuple[int, int]:
        return round(x * self.scale), round(self.top + y * self.scale)

    def slider(self, fraction: float, *, interval: bool = False) -> tuple[int, int]:
        # The shared SliderGeometry reserves eight logical pixels per endpoint.
        x = 32 + fraction * 326
        return self.point(x, 250 if interval else 114)

    def touch_position(self, point: tuple[int, int]) -> tuple[str, str]:
        return (
            f"EV_ABS:ABS_MT_POSITION_X:{round(point[0] / self.width * 32767)}",
            f"EV_ABS:ABS_MT_POSITION_Y:{round(point[1] / self.height * 32767)}",
        )


def surface(ui: Emulator, screen: Screen) -> Surface:
    density = ui.call("shell", "wm", "density").decode()
    values = re.findall(r"(?:Physical|Override) density:\s*(\d+)", density)
    if not values:
        raise AssertionError(f"Android did not report its display density: {density}")
    scale = int(values[-1]) / 160
    label = screen.require("Volume 25")
    return Surface(
        screen.width, screen.height, scale, label.y * screen.height - 76 * scale
    )


@contextmanager
def pressed_contact(
    ui: Emulator, frame: Surface, slot: int, point: tuple[int, int]
) -> Iterator[None]:
    slot_event = f"EV_ABS:ABS_MT_SLOT:{slot}"
    ui.send_touch(
        slot_event,
        f"EV_ABS:ABS_MT_TRACKING_ID:{slot + 1}",
        *frame.touch_position(point),
        "EV_ABS:ABS_MT_PRESSURE:600",
        *(["EV_KEY:BTN_TOUCH:1"] if slot == 0 else []),
    )
    try:
        yield
    finally:
        ui.send_touch(
            slot_event,
            "EV_ABS:ABS_MT_TRACKING_ID:-1",
            *(["EV_KEY:BTN_TOUCH:0"] if slot == 0 else []),
        )


@contextmanager
def drag_contact(
    ui: Emulator, frame: Surface, start: tuple[int, int], end: tuple[int, int]
) -> Iterator[None]:
    with pressed_contact(ui, frame, 0, start):
        for step in range(1, 5):
            time.sleep(0.08)
            point = (
                start[0] + round((end[0] - start[0]) * step / 4),
                start[1] + round((end[1] - start[1]) * step / 4),
            )
            ui.send_touch(*frame.touch_position(point))
        yield


def drag(
    ui: Emulator, frame: Surface, start: tuple[int, int], end: tuple[int, int]
) -> None:
    with drag_contact(ui, frame, start, end):
        pass


def selection_wheels(ui: Emulator, frame: Surface) -> None:
    def state(
        index: int, wheel_commits: int, code: int, card_commits: int, *labels: str
    ) -> Screen:
        return ui.visible(
            "Selection wheels",
            f"Wheel index {index}",
            f"Wheel commits {wheel_commits}",
            f"Card code {code}",
            f"Card commits {card_commits}",
            "Wheel idle",
            *labels,
        )

    def endpoints(
        screen: Screen, label: str, dx: float, dy: float
    ) -> tuple[tuple[int, int], tuple[int, int]]:
        hit = screen.require(label)
        x, y = hit.x * screen.width, hit.y * screen.height
        return (
            (round(x), round(y)),
            (round(x + dx * frame.scale), round(y + dy * frame.scale)),
        )

    ui.tap(ui.visible("Open wheels"), "Open wheels")
    initial = state(0, 0, 20, 0, "Cedar", "Buoy", "Wrap wheel")
    drag(ui, frame, *endpoints(initial, "Cedar", 0, 36))
    ui.tap(state(0, 0, 20, 0, "Wrap wheel"), "Wrap wheel")
    wrap = state(0, 0, 20, 0, "Cedar", "Clamp wheel")
    drag(ui, frame, *endpoints(wrap, "Cedar", 0, 36))
    wrapped = state(4, 1, 20, 0, "Willow")
    drag(ui, frame, *endpoints(wrapped, "Willow", 0, -36))
    start = state(0, 2, 20, 0, "Buoy")
    drag(ui, frame, *endpoints(start, "Buoy", -80, 0))
    ui.tap(state(0, 2, 30, 1, "Mirror cards"), "Mirror cards")
    mirrored = state(0, 2, 30, 1, "Reef", "Unmirror cards")
    drag(ui, frame, *endpoints(mirrored, "Reef", 80, 0))
    enabled = state(0, 2, 40, 2, "Cedar", "Pier", "Lock wheels")
    ui.tap(enabled, "Lock wheels")
    state(0, 2, 40, 2, "Unlock wheels")
    drag(ui, frame, *endpoints(enabled, "Cedar", 0, -36))
    drag(ui, frame, *endpoints(enabled, "Pier", 80, 0))
    ui.tap(state(0, 2, 40, 2, "Unlock wheels"), "Unlock wheels")
    unlocked = state(0, 2, 40, 2, "Cedar", "Lock wheels")
    with drag_contact(ui, frame, *endpoints(unlocked, "Cedar", 0, -36)):
        ui.visible("Dragging wheel", "Wheel index 0", "Wheel commits 2")
        ui.call("shell", "input", "keyevent", "KEYCODE_HOME")
        ui.wait_for(
            "the app enters the background before the touch is released",
            lambda screen: (
                screen.find("Selection wheels") is None
                and screen.find("Wheel commits 2") is None
            ),
        )
    ui.call("shell", "am", "start", "-W", "-n", ACTIVITY)
    resumed = state(0, 2, 40, 2, "Cedar")
    drag(ui, frame, *endpoints(resumed, "Cedar", 0, -36))
    final = state(1, 3, 40, 2, "Maple", "Pier")
    (ui.artifacts / "selection-wheels.png").write_bytes(final.png)
    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
    ui.wait_for(
        "Back closes the wheel dialog",
        lambda screen: bool(
            screen.find("Open wheels")
            and screen.find("Lock inputs")
            and not screen.find("Selection wheels")
        ),
    )


def touch_transforms(ui: Emulator, frame: Surface) -> None:
    ui.tap(ui.visible("Open gestures", "Lock inputs"), "Open gestures")
    initial = ui.visible("Touch transforms", "Gesture idle", "Tap pad", "Taps 0")
    ui.tap(initial, "Tap pad")
    ready = ui.visible("Gesture idle", "Taps 1", "Starts 0", "Zoom 100", "Angle 0")
    pin = ready.require("Tap pad")
    first = (round(pin.x * ready.width), round(pin.y * ready.height))
    second = (first[0] + round(60 * frame.scale), first[1])

    def move_second_down(distance: float) -> None:
        ui.send_touch(
            "EV_ABS:ABS_MT_SLOT:1",
            *frame.touch_position((first[0], first[1] + round(distance * frame.scale))),
        )

    with pressed_contact(ui, frame, 0, first):
        with pressed_contact(ui, frame, 1, second):
            ui.visible("Gesture active", "Starts 1", "Ends 0", "Cancels 0", "Taps 1")
            move_second_down(90)
            active = ui.visible("Gesture active", "Zoom 150", "Angle 90", "Taps 1")
            (ui.artifacts / "gesture-active.png").write_bytes(active.png)
        ui.visible("Gesture idle", "Starts 1", "Ends 1", "Cancels 0", "Taps 1")

    with pressed_contact(ui, frame, 0, first):
        with pressed_contact(ui, frame, 1, second):
            ui.visible("Gesture active", "Starts 2", "Ends 1", "Taps 1")
            move_second_down(60)
            ui.visible("Gesture active", "Zoom 150", "Angle 180")
            ui.call("shell", "input", "keyevent", "KEYCODE_HOME")
            ui.wait_for(
                "the app leaves the foreground with both contacts held",
                lambda screen: (
                    screen.find("Touch transforms") is None
                    and screen.find("Gesture active") is None
                ),
            )
    ui.call("shell", "am", "start", "-W", "-n", ACTIVITY)
    resumed = ui.visible(
        "Gesture idle",
        "Starts 2",
        "Ends 1",
        "Cancels 1",
        "Taps 1",
        "Zoom 150",
        "Angle 90",
    )
    ui.tap(resumed, "Tap pad")
    final = ui.visible(
        "Gesture idle",
        "Starts 2",
        "Ends 1",
        "Cancels 1",
        "Taps 2",
        "Zoom 150",
        "Angle 90",
    )
    (ui.artifacts / "gesture-resumed.png").write_bytes(final.png)
    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
    ui.wait_for(
        "Back closes the touch transform dialog",
        lambda screen: bool(
            screen.find("Open gestures")
            and screen.find("Lock inputs")
            and not screen.find("Touch transforms")
        ),
    )


def keyboard_visible(ui: Emulator, expected: bool) -> None:
    deadline = time.monotonic() + 10
    while True:
        state = ui.call("shell", "dumpsys", "input_method").decode()
        match = re.search(r"\bmInputShown=(true|false)\b", state)
        if match is not None and (match[1] == "true") == expected:
            print(f"PASS input method visible={expected}", flush=True)
            return
        if time.monotonic() >= deadline:
            raise AssertionError(f"The input method did not become visible={expected}")
        time.sleep(0.2)


def exercise_dial(ui: Emulator, frame: Surface) -> None:
    ui.tap(ui.visible("Open dial", "Lock inputs"), "Open dial")
    initial = ui.visible("Dial controls", "Dial 75", "Mode circular", "Dial releases 0")
    label = initial.require("Dial 75")
    center = (round(label.x * frame.width), round(label.y * frame.height))

    def point(dx: float, dy: float) -> tuple[int, int]:
        return (
            center[0] + round(dx * frame.scale),
            center[1] + round(dy * frame.scale),
        )

    def move(dx: float, dy: float) -> None:
        # Let Android sample each position of the held contact separately.
        time.sleep(0.08)
        ui.send_touch(*frame.touch_position(point(dx, dy)))

    def state(
        value: int, releases: int, *texts: str, last_release: int | None = None
    ) -> Screen:
        # Device coordinates come from the painted text rather than the layout.
        # Allow two points for OCR bounds and touch coordinate quantization.
        def matches(screen: Screen) -> bool:
            measured = next(
                (v for v in range(value - 2, value + 3) if screen.find(f"Dial {v}")),
                None,
            )
            if measured is None or not screen.find(f"Dial releases {releases}"):
                return False
            if last_release == value and not screen.find(f"Released at {measured}"):
                return False
            if (
                last_release is not None
                and last_release != value
                and not any(
                    screen.find(f"Released at {v}")
                    for v in range(last_release - 2, last_release + 3)
                )
            ):
                return False
            return all(screen.find(text) for text in texts)

        description = ", ".join((f"Dial near {value}", f"{releases} releases", *texts))
        return ui.wait_for(description, matches)

    with pressed_contact(ui, frame, 0, point(56, 0)):
        state(25, 0)
        move(0, -56)
        ui.wait_for(
            "circular drag reaches the wrap boundary before continuing",
            lambda screen: (
                screen.find("Dial releases 0") is not None
                and any(screen.find(f"Dial {v}") for v in (0, 1, 2, 98, 99, 100))
            ),
        )
        move(-56, 0)
        state(75, 0)
    ui.tap(state(75, 1, "Horizontal", last_release=75), "Horizontal")

    state(75, 1, "Mode horizontal")
    with pressed_contact(ui, frame, 0, center):
        move(0, 30)
        # The pointer goes beyond the dial face, but this contact owns the drag.
        move(84, 30)
        state(35, 1)
    ui.tap(state(35, 2, "Vertical", last_release=35), "Vertical")
    state(35, 2, "Mode vertical")
    with pressed_contact(ui, frame, 0, center):
        move(30, 0)
        move(30, -70)
        state(85, 2)
    ui.tap(state(85, 3, "Lock dial", last_release=85), "Lock dial")

    state(85, 3, "Unlock dial")
    drag(ui, frame, center, point(0, 56))
    ui.tap(state(85, 3, "Unlock dial"), "Unlock dial")
    state(85, 3, "Lock dial")
    with drag_contact(ui, frame, center, point(0, 21)):
        state(70, 3, last_release=85)
        ui.call("shell", "input", "keyevent", "KEYCODE_HOME")
        ui.wait_for(
            "the dial leaves the foreground before its touch lifts",
            lambda screen: (
                screen.find("Dial controls") is None
                and screen.find("XRack Controls") is None
            ),
        )
    ui.call("shell", "am", "start", "-W", "-n", ACTIVITY)
    resumed = state(70, 3, "Dial controls", "Mode vertical", last_release=85)
    # A task preview can show the previous dial before input reaches the
    # resumed window. Confirm an idempotent mode change before the next drag.
    for attempt in range(3):
        ui.tap(resumed, "Horizontal")
        try:
            ui.visible("Mode horizontal", "Dial releases 3", timeout=5)
            break
        except AssertionError:
            if attempt == 2:
                raise
    state(70, 3, "Mode horizontal", last_release=85)
    drag(ui, frame, center, point(-42, 0))
    final = state(40, 4, last_release=40)
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "dial.png").write_bytes(final.png)
    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
    ui.wait_for(
        "Back closes the dial dialog",
        lambda screen: (
            screen.find("Open dial") is not None
            and screen.find("Lock inputs") is not None
            and screen.find("Dial controls") is None
        ),
    )


def exercise_range_handles(ui: Emulator, initial: Screen, frame: Surface) -> None:
    ui.tap(initial, "Open dial")
    ui.tap(ui.visible("Dial controls", "Range"), "Range")

    def state(
        low: int, high: int, edits: int, *labels: str, releases: int | None = None
    ) -> Screen:
        return ui.visible(
            "Range handles",
            f"Band {low} to {high}",
            f"Range edits {edits}",
            f"Range releases {edits if releases is None else releases}",
            "Callbacks ordered",
            *labels,
        )

    idle = state(50, 50, 0, "Preview idle", "Drag the handles")
    caption = idle.require("Drag the handles")
    # The heading and track are centered in the panel. The track center is
    # 59 logical pixels below the heading, and the 30px handle leaves 268px of travel.
    y = round(caption.y * idle.height + 59 * frame.scale)

    def point(position: float) -> tuple[int, int]:
        return round(caption.x * idle.width + (position - 0.5) * 268 * frame.scale), y

    def handles(screen: Screen, lower: float, upper: float) -> Screen:
        def matches(candidate: Screen) -> bool:
            return pixels_rgb(candidate.png, (point(lower), point(upper))) == (
                (178, 79, 61),
                (32, 121, 104),
            )

        if matches(screen):
            return screen
        return ui.wait_for(
            f"lower and upper handles at {lower} and {upper}", matches, timeout=8
        )

    with pressed_contact(ui, frame, 0, point(0.5)):
        x, row = point(0.5)
        ui.send_touch(
            *frame.touch_position(
                (x + round(3 * frame.scale), row + round(5 * frame.scale))
            )
        )
        state(50, 50, 0, "Preview idle")
        ui.send_touch(*frame.touch_position(point(0.28)))
        preview = handles(state(50, 50, 0, "Preview lower"), 0.28, 0.5)
    committed = state(30, 50, 1, "Preview idle", "Mirror range")
    handles(committed, 0.3, 0.5)

    ui.tap(committed, "Mirror range")
    mirrored = state(30, 50, 1, "Preview idle", "RTL range")
    handles(mirrored, 0.7, 0.5)
    with drag_contact(ui, frame, point(0.7), point(0.82)):
        handles(state(30, 50, 1, "Preview lower"), 0.82, 0.5)
        ui.call("shell", "input", "keyevent", "KEYCODE_HOME")
        ui.wait_for(
            "the window leaves the foreground while the range preview is held",
            lambda screen: screen.find("Range handles") is None,
        )
    ui.call("shell", "am", "start", "-W", "-n", ACTIVITY)
    restored = state(30, 50, 1, "Preview idle", "Reset range")
    restored = handles(restored, 0.7, 0.5)
    ui.tap(restored, "Reset range")
    state(50, 50, 1, "Preview idle", "RTL range")
    with drag_contact(ui, frame, point(0.5), point(0.28)):
        handles(state(50, 50, 1, "Preview upper"), 0.5, 0.28)
    final = handles(state(50, 70, 2, "Preview idle", "RTL range"), 0.5, 0.3)
    with pressed_contact(ui, frame, 0, point(0.3)):
        time.sleep(0.25)
    state(50, 70, 2, "Preview idle", releases=3)

    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "range-preview.png").write_bytes(preview.png)
    (ui.artifacts / "range-restored.png").write_bytes(restored.png)
    (ui.artifacts / "range-rtl.png").write_bytes(final.png)
    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
    ui.wait_for(
        "Back closes only the range dialog",
        lambda screen: (
            screen.find("Dial controls") is not None
            and screen.find("Range handles") is None
        ),
    )


def exercise_export_group(ui: Emulator, initial: Screen, frame: Surface) -> None:
    ui.tap(initial, "Open options")
    palette = {
        "None selected": (142, 156, 174),
        "Some selected": (170, 98, 29),
        "All selected": (39, 132, 95),
    }

    def point(screen: Screen, summary: str, x: float, y: float) -> tuple[int, int]:
        label = screen.require(summary)
        # The 28dp summary row is centered in the 338dp dialog with 16dp padding.
        origin_x = label.x * screen.width - 153 * frame.scale
        origin_y = label.y * screen.height - 14 * frame.scale
        return round(origin_x + x * frame.scale), round(origin_y + y * frame.scale)

    def state(summary: str, bulk: int, edits: int, *labels: str) -> Screen:
        screen = ui.visible(
            "Export options",
            summary,
            f"Bulk edits {bulk}",
            f"Item edits {edits}",
            "Order intact",
            *labels,
        )
        actual = pixel_rgb(screen.png, *point(screen, summary, 18, 60))
        expected = palette[summary]
        assert all(abs(a - b) <= 8 for a, b in zip(actual, expected)), (
            f"Expected the {summary} indicator to paint {expected}, got {actual}"
        )
        return screen

    mixed = state("Some selected", 0, 0, "Manifest off", "Reveal review")
    assert mixed.find("Review off") is None and mixed.find("Review on") is None
    ui.tap(mixed, "Manifest off")
    unchanged = state("Some selected", 0, 0, "Manifest off")
    # Touch the custom indicator to exercise the owning CheckBox's event path.
    ui.touch(*point(unchanged, "Some selected", 18, 60))
    all_selected = state(
        "All selected", 1, 0, "Manifest on", "Compress on", "Reveal review"
    )
    assert all_selected.find("Review on") is None
    ui.tap(all_selected, "Reveal review")
    before_drag = state("All selected", 1, 0, "Review on", "Compress on")

    thumb_right = point(before_drag, "All selected", 33, 164)
    thumb_left = point(before_drag, "All selected", 15, 164)
    thumb_points = (thumb_left, thumb_right)
    painted = pixels_rgb(before_drag.png, thumb_points)
    assert painted[0] != painted[1], (
        "The switch track and thumb must be distinguishable"
    )

    def preview_matches(screen: Screen) -> bool:
        return (
            all(
                screen.find(label)
                for label in (
                    "All selected",
                    "Compress on",
                    "Bulk edits 1",
                    "Item edits 0",
                )
            )
            and pixels_rgb(screen.png, thumb_points) == painted[::-1]
        )

    with drag_contact(
        ui, frame, thumb_right, point(before_drag, "All selected", 7, 164)
    ):
        preview = ui.wait_for(
            "the switch thumb moves while the bound value and group stay unchanged",
            preview_matches,
            timeout=5,
        )
    committed = state("Some selected", 1, 1, "Compress off", "Switch reported off")
    ui.tap(committed, "Select all")
    ui.tap(state("All selected", 2, 1, "Compress on"), "Select all")
    cleared = state(
        "None selected",
        3,
        1,
        "Labels off",
        "Manifest off",
        "Compress off",
        "Review off",
    )
    ui.tap(cleared, "Review off")
    ui.tap(state("Some selected", 3, 2, "Review mixed"), "Review mixed")
    ui.tap(state("Some selected", 3, 3, "Review on", "Lock group"), "Lock group")
    locked = ui.visible(
        "Unlock group",
        "Select all",
        "Labels off",
        "Review on",
        "Some selected",
        "Bulk edits 3",
        "Item edits 3",
    )
    for label in ("Select all", "Labels off", "Review on"):
        ui.tap(locked, label)
    ui.tap(
        ui.visible(
            "Unlock group", "Some selected", "Review on", "Bulk edits 3", "Item edits 3"
        ),
        "Unlock group",
    )
    ui.tap(state("Some selected", 3, 3, "Review on", "Lock group"), "Review on")
    final = state("None selected", 3, 4, "Review off")
    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
    ui.wait_for(
        "Back closes the export options",
        lambda screen: (
            screen.find("Open options") is not None
            and screen.find("Export options") is None
        ),
    )
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "export-group-switch-preview.png").write_bytes(preview.png)
    (ui.artifacts / "export-group-cleared.png").write_bytes(final.png)


def exercise_connection_profile(ui: Emulator, initial: Screen, frame: Surface) -> None:
    ui.tap(initial, "Open profile")

    def press(key: str) -> None:
        ui.call("shell", "input", "keyevent", f"KEYCODE_{key}")

    def draft_gap(screen: Screen, draft: str) -> float:
        title = screen.require("Connection profile")
        return (screen.require(draft).y - title.y) * screen.height / frame.scale

    def folded(folds: int, focus: str, draft: str) -> Screen:
        return ui.wait_for(
            "advanced controls are folded and input has returned to the header",
            lambda screen: (
                all(
                    screen.find(label)
                    for label in (focus, draft, f"Folds {folds}", "Saves 0")
                )
                and screen.find("Apply target") is None
                and screen.find("Server target") is None
            ),
        )

    closed = ui.visible(
        "Connection profile",
        "Advanced target",
        "Delivery on",
        "Draft empty",
        "Folds 0",
        "Groups 0",
        "Saves 0",
    )
    assert closed.find("Apply target") is None
    collapsed_gap = draft_gap(closed, "Draft empty")
    restored_title_y = closed.require("Connection profile").y
    ui.tap(closed, "Advanced target")
    expanded = ui.visible(
        "Connection profile", "Server target", "Apply target", "Folds 1", "Draft empty"
    )
    open_gap = draft_gap(expanded, "Draft empty")
    assert open_gap - collapsed_gap > 80, (
        "The expander did not make room for its contents"
    )
    ui.tap(expanded, "Server target")
    keyboard_visible(ui, True)
    ui.type_text("devhost")
    ui.tap(
        ui.visible("Draft devhost", "Editing target", "Folds 1", "Fold target"),
        "Fold target",
    )
    keyboard_visible(ui, False)
    collapsed = folded(1, "Details focus", "Draft devhost")
    assert abs(draft_gap(collapsed, "Draft devhost") - collapsed_gap) < 4

    press("ENTER")
    reopened = ui.visible("Apply target", "Details focus", "Draft devhost", "Folds 2")
    apply = reopened.require("Apply target")
    with ui.press_contact((apply.x, apply.y)):
        press("F8")
        folded(2, "Details focus", "Draft devhost")
    folded(2, "Details focus", "Draft devhost")
    press("ENTER")
    reopened = ui.visible("Apply target", "Draft devhost", "Folds 3", "Saves 0")

    apply = reopened.require("Apply target")
    # The field and button are 44dp high with an 8dp vertical gap.
    ui.touch(
        round(apply.x * frame.width), round(apply.y * frame.height - 52 * frame.scale)
    )
    keyboard_visible(ui, True)
    ui.tap(
        ui.visible("Editing target", "Draft devhost", "Disable delivery"),
        "Disable delivery",
    )
    keyboard_visible(ui, False)
    # Android can report the IME hidden before the dialog recenters.
    disabled = ui.wait_for(
        "delivery is disabled and the dialog has returned to its keyboard-free position",
        lambda screen: (
            (title := screen.find("Connection profile")) is not None
            and abs(title.y - restored_title_y) * screen.height < 6 * frame.scale
            and all(
                screen.find(label) is not None
                for label in (
                    "Delivery off",
                    "Delivery focus",
                    "Apply target",
                    "Draft devhost",
                    "Folds 3",
                    "Groups 0",
                    "Saves 0",
                )
            )
        ),
    )
    assert abs(draft_gap(disabled, "Draft devhost") - open_gap) < 4, (
        "Disabling the group moved its content"
    )
    ui.tap(disabled, "Apply target")
    ui.visible("Delivery focus", "Delivery off", "Groups 0", "Saves 0")
    press("SPACE")
    enabled = ui.visible(
        "Delivery on",
        "Delivery focus",
        "Apply target",
        "Draft devhost",
        "Groups 1",
        "Folds 3",
        "Saves 0",
    )
    ui.tap(enabled, "Apply target")
    saved = ui.visible("Draft devhost", "Saves 1", "Groups 1", "Folds 3")
    press("BACK")
    ui.wait_for(
        "Back leaves the connection profile",
        lambda screen: (
            screen.find("Open profile") is not None
            and screen.find("Connection profile") is None
        ),
    )
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "connection-folded-draft.png").write_bytes(collapsed.png)
    (ui.artifacts / "connection-disabled-group.png").write_bytes(disabled.png)
    (ui.artifacts / "connection-saved.png").write_bytes(saved.png)


def exercise_button_holds(ui: Emulator, initial: Screen, frame: Surface) -> None:
    ui.tap(initial, "Open holds")
    # Slightly longer than the six-second hold interval in the fixture.
    past_hold_deadline = 6.1

    def state(
        presses: int, releases: int, cancels: int, taps: int, holds: int, *labels: str
    ) -> Screen:
        return ui.visible(
            "Button holds",
            "Order intact",
            f"Presses {presses}",
            f"Releases {releases}",
            f"Cancels {cancels}",
            f"Taps {taps}",
            f"Holds {holds}",
            *labels,
        )

    def point(screen: Screen, caption: str) -> tuple[float, float]:
        label = screen.require(caption)
        return label.x, label.y

    idle = state(0, 0, 0, 0, 0, "Inspect control", "Inspection idle")
    ui.tap(idle, "Inspect control")
    tapped = state(1, 1, 0, 1, 0, "Inspect control", "Inspection idle")
    with ui.press_contact(point(tapped, "Inspect control")):
        held = state(2, 1, 0, 1, 1, "Inspection held")
        # Keep holding for another full interval to detect an accidental repeat.
        time.sleep(past_hold_deadline)
        state(2, 1, 0, 1, 1, "Inspection held")
    released = state(2, 2, 0, 1, 1, "Inspect control", "Inspection idle")

    x, y = point(released, "Inspect control")
    start = round(x * frame.width), round(y * frame.height)
    outside = start[0], start[1] + round(68 * frame.scale)
    with drag_contact(ui, frame, start, outside):
        state(3, 2, 1, 1, 1, "Inspection idle")
        time.sleep(past_hold_deadline)
        state(3, 2, 1, 1, 1, "Inspection idle")
    canceled = state(3, 2, 1, 1, 1, "Inspect control", "Inspection idle")

    started = time.monotonic()
    with ui.press_contact(point(canceled, "Inspect control")):
        state(4, 2, 1, 1, 1, "Inspection held")
        ui.call("shell", "input", "keyevent", "KEYCODE_HOME")
        ui.wait_for(
            "the app enters the background before releasing the button",
            lambda screen: screen.find("Button holds") is None,
        )
        print(f"Launcher visible after {time.monotonic() - started:.2f}s", flush=True)
        time.sleep(past_hold_deadline)
    ui.call("shell", "am", "start", "-W", "-n", ACTIVITY)
    returned = state(4, 2, 2, 1, 1, "Inspection idle", "Repeat step", "Steps 0")

    def repeat_capped() -> Screen:
        return state(
            4,
            2,
            2,
            1,
            1,
            "Repeat stopped",
            "Steps 3",
            "Step saw press",
            "Step cancels 1",
            "Step releases 0",
        )

    with ui.press_contact(point(returned, "Repeat step")):
        capped = repeat_capped()
        time.sleep(0.65)
        repeat_capped()
    ui.tap(repeat_capped(), "Repeat stopped")
    final = repeat_capped()
    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
    ui.wait_for(
        "Back closes the button holds dialog",
        lambda screen: (
            screen.find("Open holds") is not None
            and screen.find("Button holds") is None
        ),
    )
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "button-held-once.png").write_bytes(held.png)
    (ui.artifacts / "button-repeat-capped.png").write_bytes(capped.png)
    (ui.artifacts / "button-holds-complete.png").write_bytes(final.png)


def exercise_delay_confirmation(ui: Emulator, initial: Screen, frame: Surface) -> None:
    ui.tap(initial, "Open holds")
    ui.tap(ui.visible("Button holds", "Open verify"), "Open verify")
    tint = (36, 126, 100)
    delay_seconds = 8.2
    past_deadline = delay_seconds + 0.1
    ui.artifacts.mkdir(parents=True, exist_ok=True)

    def point(screen: Screen, caption: str) -> tuple[float, float]:
        label = screen.require(caption)
        return label.x, label.y

    def paint(png: bytes, anchor: tuple[float, float]) -> tuple[bool, bool]:
        x, y = anchor
        probes = tuple(
            (
                round(x * frame.width + dx * frame.scale),
                round(y * frame.height - 16 * frame.scale),
            )
            for dx in (-98, 98)
        )
        left, right = pixels_rgb(png, probes)
        return left == tint, right == tint

    def state(count: int, *labels: str) -> Screen:
        screen = ui.visible("Hold confirmation", f"Confirmations {count}", *labels)
        assert screen.find("Order error") is None
        return screen

    def capture_pending() -> bytes:
        # Keep OCR and pixel decoding outside the timed input sequence.
        time.sleep(4.6)
        return ui.call("exec-out", "screencap", "-p")

    ready = state(0, "Hold to verify", "Ready to hold", "Scroll guide")
    initial_anchor = point(ready, "Hold to verify")
    anchor = initial_anchor
    assert paint(ready.png, anchor) == (False, False)
    ui.tap(ready, "Hold to verify")
    state(0, "Ready to hold")
    started = time.monotonic()
    with ui.press_contact(anchor):
        filling = capture_pending()
    duration = time.monotonic() - started
    (ui.artifacts / "delay-filling.png").write_bytes(filling)
    assert duration < delay_seconds, f"The early release was sent at {duration:.2f}s"
    assert paint(filling, anchor) == (True, False), (
        "The held button did not paint a partial fill"
    )
    print(
        f"PASS released a partially filled confirmation at {duration:.2f}s", flush=True
    )
    canceled = ui.wait_for(
        "early release clears the fill without confirming",
        lambda screen: (
            screen.find("Ready to hold") is not None
            and screen.find("Confirmations 0") is not None
            and paint(screen.png, anchor) == (False, False)
        ),
    )
    started = time.monotonic()
    with ui.press_contact(point(canceled, "Hold to verify")):
        before_background = capture_pending()
        ui.call("shell", "input", "keyevent", "KEYCODE_HOME")
        ui.wait_for(
            "Android backgrounds the confirmation while the button is held",
            lambda screen: (
                screen.find("Hold confirmation") is None
                and screen.find("XRack Controls") is None
            ),
        )
        time.sleep(max(0.0, started + past_deadline - time.monotonic()))
    assert paint(before_background, anchor) == (True, False), (
        "No progress was painted before backgrounding"
    )
    ui.call("shell", "am", "start", "-W", "-n", ACTIVITY)
    resumed = ui.wait_for(
        "returning after the old deadline clears the fill and keeps confirmations at zero",
        lambda screen: (
            all(
                screen.find(label)
                for label in ("Hold to verify", "Ready to hold", "Confirmations 0")
            )
            and paint(screen.png, point(screen, "Hold to verify")) == (False, False)
            and ui.window_accepts_touch(screen, ACTIVITY)
        ),
        timeout=75,
    )
    anchor = point(resumed, "Hold to verify")
    with ui.press_contact(anchor):
        before_scroll = capture_pending()
        ui.move_contact(
            anchor, (anchor[0], anchor[1] - 125 * frame.scale / frame.height)
        )
        ui.visible("Archived choices", "Confirmations 0", "Ready to hold")
        time.sleep(past_deadline)
    assert paint(before_scroll, anchor) == (True, False), (
        "No progress was painted before scrolling"
    )
    scrolled = state(0, "Archived choices", "Ready to hold")
    assert scrolled.find("Hold to verify") is None
    archive = point(scrolled, "Archived choices")
    with ui.drag_contact(
        archive, (archive[0], archive[1] + 195 * frame.scale / frame.height)
    ):
        pass
    top = ui.wait_for(
        "the scroll view returns to the confirmation button",
        lambda screen: (
            screen.find("Hold to verify") is not None
            and abs(point(screen, "Hold to verify")[1] - initial_anchor[1]) < 0.009
            and screen.find("Confirmations 0") is not None
        ),
    )
    anchor = point(top, "Hold to verify")

    ui.tap(top, "Schedule lock")
    started = time.monotonic()
    pending_frames: list[bytes] = []
    with ui.press_contact(anchor):
        # Sample several frames while the timer runs. Presentation can lag
        # behind the touch event on the emulator.
        for elapsed in (3.0, 4.6, 5.8):
            time.sleep(max(0.0, started + elapsed - time.monotonic()))
            pending_frames.append(ui.call("exec-out", "screencap", "-p"))
        locked = state(0, "Input locked", "Unlock check", "Hold to verify")
        time.sleep(past_deadline)
    pending_samples = [paint(png, anchor) for png in pending_frames]
    assert (True, False) in pending_samples, (
        f"No partial fill before locking: {pending_samples}"
    )
    assert paint(locked.png, anchor) == (False, False)
    ui.tap(state(0, "Input locked", "Hold to verify"), "Hold to verify")
    ui.tap(state(0, "Input locked", "Unlock check"), "Unlock check")
    unlocked = state(0, "Ready to hold", "Hold to verify")
    anchor = point(unlocked, "Hold to verify")
    assert paint(unlocked.png, anchor) == (False, False)
    with ui.press_contact(anchor):
        confirmed = state(1, "Confirmed", "Confirmed state")
        assert paint(confirmed.png, anchor) == (True, True)
    after_release = state(1, "Confirmed", "Confirmed state")
    assert paint(after_release.png, anchor) == (True, True)
    ui.tap(after_release, "Confirmed")
    reset = state(1, "Ready to hold", "Hold to verify")
    assert paint(reset.png, anchor) == (False, False)
    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
    ui.wait_for(
        "Back closes only the confirmation dialog",
        lambda screen: (
            screen.find("Button holds") is not None
            and screen.find("Hold confirmation") is None
        ),
    )
    (ui.artifacts / "delay-locked.png").write_bytes(locked.png)
    (ui.artifacts / "delay-confirmed.png").write_bytes(confirmed.png)


def exercise_click_routes(ui: Emulator, initial: Screen, frame: Surface) -> None:
    ui.tap(initial, "Open holds")
    ui.tap(ui.visible("Button holds", "Open clicks"), "Open clicks")

    def state(commands: int, pairs: int, *labels: str) -> Screen:
        screen = ui.visible(
            "Click routes", f"Commands {commands}", f"Pairs {pairs}", *labels
        )
        assert screen.find("Order error") is None
        return screen

    def point(screen: Screen, caption: str) -> tuple[float, float]:
        label = screen.require(caption)
        return label.x, label.y

    def rapid_taps(*points: tuple[float, float]) -> None:
        releases: list[float] = []
        for index, contact in enumerate(points):
            if index:
                time.sleep(0.040)
            with ui.press_contact(contact):
                time.sleep(0.045)
            releases.append(time.monotonic())
        # The pairing window measures release-to-release, not the first press.
        span = releases[-1] - releases[0]
        assert span < 0.45, f"The touch releases span {span:.3f}s"

    idle = state(0, 0, "Pair target", "Other target", "Paired after 0")
    center = point(idle, "Pair target")
    rapid_taps(center, center)
    paired = state(2, 1, "Paired after 2", "Presses 2", "Releases 2", "Cancels 0")
    delta = 12 * frame.scale / frame.width
    rapid_taps((center[0] - delta, center[1]), (center[0] + delta, center[1]))
    apart = state(4, 1, "Paired after 2", "Other target", "Other taps 0")
    rapid_taps(center, point(apart, "Other target"), center)
    separate = state(6, 1, "Paired after 2", "Other taps 1", "Program click")

    ui.tap(separate, "Program click")
    synchronous = state(7, 1, "Presses 7", "Releases 7", "Cancels 0", "Animate twice")
    ui.tap(synchronous, "Animate twice")
    animated = state(8, 1, "Presses 8", "Releases 8", "Cancels 0", "Cancel queued")
    ui.tap(animated, "Cancel queued")
    canceled = state(8, 1, "Presses 9", "Releases 8", "Cancels 1", "Unlock target")
    ui.tap(canceled, "Pair target")
    ui.tap(state(8, 1, "Presses 9", "Cancels 1", "Unlock target"), "Unlock target")
    ready = state(8, 1, "Pair target", "Lock target")
    center = point(ready, "Pair target")
    rapid_taps(center, center)
    state(10, 2, "Paired after 10", "Presses 11", "Releases 10", "Cancels 1")
    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
    ui.wait_for(
        "Back returns to the button holds dialog",
        lambda screen: (
            screen.find("Button holds") is not None
            and screen.find("Click routes") is None
        ),
    )
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "click-paired.png").write_bytes(paired.png)
    (ui.artifacts / "click-animation-canceled.png").write_bytes(canceled.png)


def exercise_tag_format(ui: Emulator, initial: Screen, keyboard_apk: Path) -> None:
    def key(label: str) -> None:
        ui.tap(ui.visible(label), label)
        ui.visible(f"{label} sent")

    def read_buffer(report: str) -> None:
        key("Read buffer")
        # Vision can read the digit after the mask separator as a letter.
        spellings = (
            report,
            report.replace("AB-1", "AB-I"),
            report.replace("AB-1", "AB-l"),
        )
        ui.wait_for(report, lambda screen: any(screen.find(text) for text in spellings))

    with test_input_method(ui, keyboard_apk):
        ui.tap(initial, "Open dial")
        ui.tap(ui.visible("Dial controls", "Tags"), "Tags")
        ui.visible(
            "Format asset tag", "AB1", "Tag empty", "Tag incomplete", "Compose ni"
        )
        # Traverse between the two fields so the keyboard may reflow the
        # dialog without changing the target of a tap.
        ui.call("shell", "input", "keyevent", "KEYCODE_TAB")
        keyboard_visible(ui, False)
        for letter in ("A", "C"):
            ui.call(
                "shell",
                "input",
                "keycombination",
                "KEYCODE_CTRL_LEFT",
                f"KEYCODE_{letter}",
            )
        ui.call("shell", "input", "keycombination", "KEYCODE_SHIFT_LEFT", "KEYCODE_TAB")
        keyboard_visible(ui, True)
        ui.call("shell", "input", "keycombination", "KEYCODE_CTRL_LEFT", "KEYCODE_V")
        ui.visible("Draft AB-1", "Tag incomplete")
        read_buffer("At end with 4 chars AB-1")

        key("Submit")
        ui.visible("Draft AB-1", "Tag incomplete", "Saves 0", "Finishes 0")
        keyboard_visible(ui, True)
        key("Compose ni")
        read_buffer("At end with 6 chars AB-1ni")
        ui.visible("Draft AB-1", "Tag incomplete")
        key("Commit Unicode")
        read_buffer("At end with 4 chars AB-1")
        ui.visible("Draft AB-1", "Tag incomplete", "Tag unsaved")

        key("Commit number")
        ui.visible("Draft AB-147", "Tag ready")
        read_buffer("At end with 6 chars AB-147")
        key("Reverse select")
        read_buffer("Selected 6 chars AB-147")
        key("Commit Unicode")
        read_buffer("Selected 6 chars AB-147")
        ui.visible("Draft AB-147", "Tag ready", "Saves 0", "Finishes 0")

        # Done hides the keyboard, so its "sent" status can disappear before OCR.
        ui.tap(ui.visible("Submit"), "Submit")
        result = ("Saved AB-147", "Finished after save", "Saves 1", "Finishes 1")
        ui.visible(*result)
        keyboard_visible(ui, False)
        saved = ui.wait_for(
            "the formatted tag is saved once after the keyboard closes",
            lambda screen: (
                not screen.find("Compose ni")
                and all(screen.find(text) for text in result)
            ),
        )
        ui.artifacts.mkdir(parents=True, exist_ok=True)
        (ui.artifacts / "tag-format-saved.png").write_bytes(saved.png)
        ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
        ui.wait_for(
            "Back returns to the dial controls",
            lambda screen: (
                screen.find("Dial controls") is not None
                and screen.find("Format asset tag") is None
            ),
        )


def exercise_password_panel(ui: Emulator, initial: Screen) -> None:
    ui.tap(initial, "Open holds")
    ui.tap(ui.visible("Button holds", "Open privacy"), "Open privacy")

    def ctrl(letter: str) -> None:
        ui.call(
            "shell", "input", "keycombination", "KEYCODE_CTRL_LEFT", f"KEYCODE_{letter}"
        )

    def shows(screen: Screen, value: str) -> bool:
        # Vision occasionally includes the input caret in the recognized text.
        spellings = {value, value + "i", value + "l", value + "|"}
        return any(
            label.text.casefold().replace(" ", "") in spellings
            for label in screen.labels
        )

    def masked(screen: Screen) -> bool:
        return shows(screen, "xxxxx") and not shows(screen, "moss7")

    ui.tap(ui.visible("Password privacy", "Probe sentinel", "checkpad"), "checkpad")
    keyboard_visible(ui, True)
    ctrl("A")
    ctrl("C")
    ui.tap(ui.visible("Clear probe", "Probe sentinel"), "Clear probe")
    ui.visible("Probe empty")
    ctrl("V")
    ui.tap(ui.visible("Probe sentinel", "Clear probe"), "Clear probe")
    ui.tap(ui.visible("Probe empty", "Test passphrase"), "Test passphrase")
    ui.type_text("moss7")
    ui.wait_for(
        "the password binding receives the full value and the control paints five Xs",
        lambda screen: screen.find("Bound match") is not None and masked(screen),
    )

    ctrl("A")
    ctrl("C")
    ctrl("X")
    ui.tap(ui.visible("Bound match", "Reveal value"), "Reveal value")
    ui.wait_for(
        "reveal paints the bound value without changing it",
        lambda screen: (
            screen.find("Bound match") is not None
            and screen.find("Mask value") is not None
            and shows(screen, "moss7")
        ),
    )
    ctrl("A")
    ctrl("C")
    ctrl("X")
    ui.tap(ui.visible("Bound match", "Mask value"), "Mask value")
    ui.tap(ui.visible("Reveal value", "Paste proof"), "Paste proof")
    ctrl("V")
    final = ui.wait_for(
        "copy and cut preserve the password and clipboard in both visibility modes",
        lambda screen: (
            screen.find("Bound match") is not None
            and screen.find("Probe sentinel") is not None
            and masked(screen)
        ),
    )
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "password-masked.png").write_bytes(final.png)
    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
    keyboard_visible(ui, False)
    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
    ui.wait_for(
        "Back closes the password dialog after dismissing the keyboard",
        lambda screen: (
            screen.find("Button holds") is not None
            and screen.find("Password privacy") is None
        ),
    )


def exercise(
    ui: Emulator,
    apk: Path,
    *,
    dial_only: bool = False,
    range_handles_only: bool = False,
    export_group_only: bool = False,
    connection_only: bool = False,
    button_holds_only: bool = False,
    delay_confirmation_only: bool = False,
    click_routes_only: bool = False,
    password_only: bool = False,
    format_keyboard: Path | None = None,
) -> None:
    rotation = ui.call("shell", "cmd", "window", "user-rotation").decode().split()
    try:
        ui.call("shell", "cmd", "window", "user-rotation", "lock", "0")
        ui.call("install", "-r", str(apk))
        ui.call("shell", "am", "force-stop", PACKAGE)
        ui.call("shell", "am", "start", "-W", "-f", "0x10008000", "-n", ACTIVITY)
        initial = ui.visible(
            "XRack Controls",
            "Volume 25",
            "Range 20 - 75",
            "Count 2",
            "Gain 1.5",
            "Priority Normal",
            "Changes 0",
            "Releases 0",
            timeout=75,
        )
        frame = surface(ui, initial)
        if dial_only:
            exercise_dial(ui, frame)
            return
        if range_handles_only:
            exercise_range_handles(ui, initial, frame)
            return
        if export_group_only:
            exercise_export_group(ui, initial, frame)
            return
        if connection_only:
            exercise_connection_profile(ui, initial, frame)
            return
        if button_holds_only:
            exercise_button_holds(ui, initial, frame)
            return
        if delay_confirmation_only:
            exercise_delay_confirmation(ui, initial, frame)
            return
        if click_routes_only:
            exercise_click_routes(ui, initial, frame)
            return
        if password_only:
            exercise_password_panel(ui, initial)
            return
        if format_keyboard is not None:
            exercise_tag_format(ui, initial, format_keyboard)
            return
        drag(ui, frame, frame.slider(0.25), frame.slider(0.75))
        moved = ui.visible("Volume 75", "Releases 1")
        changes = next(
            label.text for label in moved.labels if label.text.startswith("Changes ")
        )
        if int(changes.split()[-1]) < 1:
            raise AssertionError("Moving the slider did not fire its changed callback")
        ui.tap(moved, "Mute")
        drag(ui, frame, frame.slider(0.75), frame.slider(0.10))
        time.sleep(0.6)
        muted = ui.visible("Volume 75", "Releases 1", changes)
        ui.tap(muted, "Mute")

        drag(
            ui,
            frame,
            frame.slider(0.20, interval=True),
            frame.slider(0.40, interval=True),
        )
        ui.visible("Range 40 - 75")
        ui.touch(*frame.point(154, 379))
        ui.visible("Count 3")
        ui.touch(*frame.point(334, 401))
        ui.visible("Gain 1", "Count 3", "Priority Normal")
        keyboard_visible(ui, False)

        # Stepping must not suppress a later tap on the editable text body.
        ui.touch(*frame.point(80, 390))
        keyboard_visible(ui, True)
        ui.call("shell", "input", "keyevent", "KEYCODE_MOVE_END")
        ui.call("shell", "input", "keyevent", "KEYCODE_DEL")
        ui.type_text("4")
        ui.call("shell", "input", "keyevent", "KEYCODE_ENTER")
        ui.visible("Count 4")
        ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
        keyboard_visible(ui, False)
        ui.touch(*frame.point(154, 401))
        ui.visible("Count 3")
        keyboard_visible(ui, False)
        # Open the ComboBox through its arrow and choose a real popup row.
        ui.touch(*frame.point(frame.width / frame.scale - 38, 488))
        ui.tap(ui.visible("Normal", "Urgent", "Deferred"), "Urgent")
        selected = ui.visible("Priority Urgent")
        ui.touch(*frame.point(frame.width / frame.scale - 38, 488))
        ui.visible("Normal", "Deferred")
        ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
        selected = ui.wait_for(
            "Back dismisses the choice without changing its value",
            lambda screen: (
                screen.find("Priority Urgent") is not None
                and screen.find("Deferred") is None
            ),
        )
        ui.tap(selected, "Lock inputs")
        ui.touch(*frame.point(154, 379))
        ui.touch(*frame.point(334, 401))
        drag(
            ui,
            frame,
            frame.slider(0.40, interval=True),
            frame.slider(0.60, interval=True),
        )
        ui.touch(*frame.point(frame.width / frame.scale - 38, 488))
        time.sleep(0.6)
        locked = ui.visible("Count 3", "Gain 1", "Range 40 - 75", "Priority Urgent")
        if locked.find("Normal") or locked.find("Deferred"):
            raise AssertionError("The disabled ComboBox opened its popup")
        ui.tap(locked, "Lock inputs")

        ui.tap(ui.visible("Open dialog"), "Open dialog")
        modal = ui.visible("Confirm settings", "Apply", "Cancel")
        # The background action lies below the dialog, outside its modal body.
        # Dismiss through the scrim and verify this touch was not forwarded.
        ui.tap(modal, "Background 0")
        ui.visible("Rejected 1", "Accepted 0", "Background 0")
        ui.tap(ui.visible("Open dialog"), "Open dialog")
        ui.tap(ui.visible("Confirm settings", "Apply"), "Apply")
        ui.visible("Accepted 1", "Rejected 1", "Background 0")
        ui.tap(ui.visible("Open dialog"), "Open dialog")
        ui.tap(ui.visible("Confirm settings", "Cancel"), "Cancel")
        ui.visible("Accepted 1", "Rejected 2", "Background 0")
        ui.tap(ui.visible("Open dialog"), "Open dialog")
        ui.visible("Confirm settings", "Cancel")
        ui.call("shell", "input", "keyevent", "--longpress", "KEYCODE_BACK")
        ui.wait_for(
            "Back rejects the dialog once",
            lambda screen: (
                screen.find("Rejected 3") is not None
                and screen.find("Confirm settings") is None
                and screen.find("Background 0") is not None
            ),
        )
        ui.tap(ui.visible("Background 0"), "Background 0")
        final = ui.visible(
            "Background 1", "Volume 75", "Range 40 - 75", "Count 3", "Priority Urgent"
        )
        ui.artifacts.mkdir(parents=True, exist_ok=True)
        (ui.artifacts / "controls.png").write_bytes(final.png)
        selection_wheels(ui, frame)
        touch_transforms(ui, frame)
        print(
            "Android controls: drag, disable, step, popup and modal routing passed",
            flush=True,
        )
    finally:
        ui.call("shell", "cmd", "window", "user-rotation", *rotation)


def main() -> None:
    parser = argparse.ArgumentParser(description="Exercise XRack controls on Android")
    parser.add_argument("--serial", required=True)
    parser.add_argument("--apk", required=True, type=Path)
    parser.add_argument("--adb", required=True, type=Path)
    scenario = parser.add_mutually_exclusive_group()
    scenario.add_argument("--dial-only", action="store_true")
    scenario.add_argument("--range-handles-only", action="store_true")
    scenario.add_argument("--export-group-only", action="store_true")
    scenario.add_argument("--connection-only", action="store_true")
    scenario.add_argument("--button-holds-only", action="store_true")
    scenario.add_argument("--delay-confirmation-only", action="store_true")
    scenario.add_argument("--click-routes-only", action="store_true")
    scenario.add_argument("--password-only", action="store_true")
    scenario.add_argument("--tag-format-only", action="store_true")
    parser.add_argument("--keyboard-apk", type=Path)
    args = parser.parse_args()
    adb = args.adb.resolve()
    if not adb.is_file() or not args.apk.is_file():
        parser.error("Both adb and the controls APK must exist")
    if args.tag_format_only and (
        args.keyboard_apk is None or not args.keyboard_apk.is_file()
    ):
        parser.error("The tag format test requires an existing --keyboard-apk")
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
            ControlsEmulator(adb, args.serial, recognize, artifacts),
            args.apk.resolve(),
            dial_only=args.dial_only,
            range_handles_only=args.range_handles_only,
            export_group_only=args.export_group_only,
            connection_only=args.connection_only,
            button_holds_only=args.button_holds_only,
            delay_confirmation_only=args.delay_confirmation_only,
            click_routes_only=args.click_routes_only,
            password_only=args.password_only,
            format_keyboard=args.keyboard_apk if args.tag_format_only else None,
        )


if __name__ == "__main__":
    main()
