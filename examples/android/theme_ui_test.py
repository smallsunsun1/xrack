import argparse
import math
import os
import re
import subprocess
import tempfile
import time
from pathlib import Path

from examples.android.controls_ui_test import Surface, drag, keyboard_visible
from examples.android.navigation_ui_test import (
    ACTIVITY,
    PACKAGE,
    NavigationEmulator,
    coordinates,
    open_drawer_item,
)
from examples.android.simulator_ui_test import (
    Emulator,
    Label,
    Screen,
    pixel_rgb,
    pixels_rgb,
)


class ThemeEmulator(NavigationEmulator):
    def screen(self) -> Screen:
        screen = super().screen()
        labels: list[Label] = []
        for label in screen.labels:
            text = label.text
            if re.fullmatch(r"[0-9OoIl]+(?:[.,][0-9OoIl]+)?", text) and any(
                c.isdigit() for c in text
            ):
                text = text.translate(str.maketrans("OoIl,", "0011."))
            status = re.fullmatch(r"(Frame|Frames|Finished|Reports) ([0-9OoIl]+)", text)
            if status is not None:
                text = (
                    status[1] + " " + status[2].translate(str.maketrans("OoIl", "0011"))
                )
            samples = re.fullmatch(r"Samples ([0-4OoIl]) of 4", text)
            if samples is not None:
                count = samples[1].translate(str.maketrans("OoIl", "0011"))
                text = f"Samples {count} of 4"
            labels.append(Label(label.x, label.y, text))
        return Screen(screen.png, screen.width, screen.height, labels)


def keyboard_top(ui: Emulator) -> int:
    dump = ui.call("shell", "dumpsys", "window", "windows").decode()
    for window in dump.split("Window #"):
        if "ty=INPUT_METHOD" in window and "mViewVisibility=0x0" in window:
            frame = re.search(r"\bframe=\[-?\d+,(-?\d+)\]\[-?\d+,-?\d+\]", window)
            if frame is not None:
                # The IME window includes a transparent area above its keys.
                content = re.search(r"mGivenVisibleInsets=\[-?\d+,(\d+)\]", window)
                return int(frame[1]) + (int(content[1]) if content is not None else 0)
    raise AssertionError("The visible Android input method has no window frame")


def above_keyboard(ui: Emulator, frame: Surface, title: str, save: str) -> Screen:
    keyboard_visible(ui, True)
    top = keyboard_top(ui)
    return ui.wait_for(
        "picker actions remain above the keyboard",
        lambda screen: (
            screen.find(title) is not None
            and (label := screen.find(save)) is not None
            and label.y * screen.height + 22 * frame.scale < top
        ),
    )


def hide_keyboard(ui: Emulator) -> None:
    state = ui.call("shell", "dumpsys", "input_method").decode()
    if "mInputShown=true" in state:
        ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
        keyboard_visible(ui, False)


def replace_selected(ui: Emulator, screen: Screen, label: Label, value: str) -> None:
    ui.touch(
        round(label.x * screen.width), round(label.y * screen.height), hold_ms=1600
    )
    ui.tap(ui.visible("Select all"), "Select all")
    ui.type_text(value)


def set_hex(ui: Emulator, frame: Surface, value: str) -> None:
    screen = above_keyboard(ui, frame, "Paint color", "Save color")
    label = next(label for label in screen.labels if label.text.startswith("#"))
    replace_selected(ui, screen, label, value)


def button_fill(screen: Screen, frame: Surface, caption: str) -> tuple[int, int, int]:
    label = screen.require(caption)
    return pixel_rgb(
        screen.png,
        round(label.x * screen.width),
        round(label.y * screen.height - 16 * frame.scale),
    )


def saved_color(screen: Screen, frame: Surface, expected: tuple[int, int, int]) -> bool:
    color = pixel_rgb(screen.png, *frame.point(326, 204))
    return all(abs(actual - wanted) <= 4 for actual, wanted in zip(color, expected))


def family(ui: Emulator) -> None:
    ui.tap(ui.visible("Text font", "sans-serif"), "sans-serif")
    ui.call("shell", "input", "keyevent", "KEYCODE_R")
    ui.visible("Roboto")
    ui.call("shell", "input", "keyevent", "KEYCODE_ENTER")
    ui.visible("Text font", "Family", "Roboto", "Save font")


def number(ui: Emulator, frame: Surface, caption: str, value: str) -> None:
    screen = ui.visible("Text font", caption)
    label = screen.require(caption)
    x = label.x * screen.width + 80 * frame.scale
    ui.touch(round(x), round(label.y * screen.height))
    screen = above_keyboard(ui, frame, "Text font", "Save font")
    label = screen.require(caption)
    editor = Label(
        (label.x * screen.width + 80 * frame.scale) / screen.width, label.y, caption
    )
    replace_selected(ui, screen, editor, value)
    hide_keyboard(ui)
    ui.call("shell", "input", "keyevent", "KEYCODE_ENTER")


def exercise_images(ui: Emulator, initial: Screen, frame: Surface) -> None:
    open_drawer_item(ui, initial, "Open images")
    coral = (252, 136, 109)
    yellow = (255, 209, 119)
    blue = (81, 108, 254)
    teal = (57, 214, 188)

    def paints(
        screen: Screen,
        control: str,
        expected: tuple[tuple[int, int, tuple[int, int, int]], ...],
    ) -> bool:
        label = screen.find(control)
        if label is None:
            return False
        # Each square is centered over its button. Its top is 182 logical pixels
        # above the center of the button's label.
        points = tuple(
            (
                round(label.x * screen.width + (x - 70) * frame.scale),
                round(label.y * screen.height + (y - 182) * frame.scale),
            )
            for x, y, _ in expected
        )
        return all(
            all(abs(actual - wanted) <= 4 for actual, wanted in zip(rgb, color))
            for rgb, (_, _, color) in zip(pixels_rgb(screen.png, points), expected)
        )

    contained = ui.wait_for(
        "the cropped PNG row keeps all four bands and its contain margins",
        lambda screen: (
            screen.find("Image workshop") is not None
            and screen.find("Atlas 8 by 8") is not None
            and paints(
                screen,
                "Use cover",
                (
                    (20, 15, (218, 226, 235)),
                    (15, 70, coral),
                    (52, 70, yellow),
                    (88, 70, blue),
                    (124, 70, teal),
                ),
            )
        ),
    )
    ui.tap(contained, "Use cover")
    covered = ui.wait_for(
        "cover fills the square with the centered two bands",
        lambda screen: paints(
            screen, "Use contain", ((22, 15, yellow), (114, 15, blue))
        ),
    )
    ui.tap(covered, "Bottom row")
    cropped = ui.wait_for(
        "changing the source rectangle renders the lower atlas row",
        lambda screen: (
            screen.find("Top row") is not None
            and paints(
                screen,
                "Use contain",
                ((22, 15, (255, 132, 198)), (114, 15, (25, 39, 68))),
            )
        ),
    )
    ui.tap(cropped, "Pause pulse")
    ui.tap(ui.visible("Resume pulse", "Play pulse"), "Play pulse")
    first = ((22, 70, coral), (118, 70, coral))
    final = ((22, 70, coral), (118, 70, blue))
    ui.visible("Stop pulse", "Resume pulse", "Frame 0", "Frames 3", "Finished 0")
    time.sleep(1.15)
    paused = ui.wait_for(
        "a paused GIF stays on its first frame for longer than one loop",
        lambda screen: (
            all(screen.find(text) for text in ("Frame 0", "Finished 0", "Resume pulse"))
            and paints(screen, "Stop pulse", first)
        ),
    )
    ui.tap(paused, "Resume pulse")
    finished = ui.wait_for(
        "one GIF loop completes and paints the final composited frame",
        lambda screen: (
            screen.find("Frame 2") is not None
            and screen.find("Finished 1") is not None
            and paints(screen, "Stop pulse", final)
        ),
    )
    ui.tap(finished, "Stop pulse")
    stopped = ui.wait_for(
        "stopping resets to the first frame without another completion",
        lambda screen: (
            screen.find("Frame 0") is not None
            and screen.find("Finished 1") is not None
            and paints(screen, "Play pulse", first)
        ),
    )
    ui.tap(stopped, "Play pulse")
    replayed = ui.wait_for(
        "a fresh playback completes once more",
        lambda screen: (
            screen.find("Frame 2") is not None
            and screen.find("Finished 2") is not None
            and paints(screen, "Stop pulse", final)
        ),
    )
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "images-contained.png").write_bytes(contained.png)
    (ui.artifacts / "images-finished.png").write_bytes(replayed.png)


def exercise_scan(ui: Emulator, initial: Screen, frame: Surface) -> None:
    open_drawer_item(ui, initial, "Open scan")
    fill = (27, 135, 91)
    track = (204, 216, 228)
    busy = (196, 65, 89)

    def matches(pixel: tuple[int, int, int], color: tuple[int, int, int]) -> bool:
        return all(
            abs(actual - expected) <= 8 for actual, expected in zip(pixel, color)
        )

    def sample(
        screen: Screen, completed: int
    ) -> tuple[tuple[tuple[int, int, int], ...], tuple[bool, ...]]:
        anchor = screen.require(f"Samples {completed} of 4")
        # Dialog content is 338dp minus 16dp padding on either side.
        # The status row is centered in that area and 28dp high.
        left = anchor.x * screen.width - 153 * frame.scale
        top = anchor.y * screen.height - 14 * frame.scale

        def point(x: float, y: float) -> tuple[int, int]:
            return round(left + x * frame.scale), round(top + y * frame.scale)

        points = (point(78, 52), point(222, 52), point(104, 104), point(64, 104))
        ring = tuple(
            point(214 + 20 * math.cos(angle), 104 + 20 * math.sin(angle))
            for angle in (step * math.tau / 12 for step in range(12))
        )
        colors = pixels_rgb(screen.png, points + ring)
        return colors[:4], tuple(matches(pixel, busy) for pixel in colors[4:])

    def expect_indicators(
        screen: Screen, completed: int, expected: tuple[tuple[int, int, int], ...]
    ) -> None:
        colors, busy_pixels = sample(screen, completed)
        assert all(matches(pixel, shade) for pixel, shade in zip(colors, expected)), (
            f"Expected progress and spinner colors {expected}, got {colors}"
        )
        assert not any(busy_pixels), "An idle BusyIndicator is still painted"

    ready = ui.visible("Scan workshop", "Scan ready", "Samples 0 of 4", "Start scan")
    expect_indicators(ready, 0, (track, track, track, track))
    ui.tap(ready, "Start scan")
    halfway = ui.visible("Scan paused", "Samples 2 of 4", "Continue scan")
    expect_indicators(halfway, 2, (fill, track, fill, track))
    time.sleep(1.4)
    ui.tap(
        ui.visible("Scan paused", "Samples 2 of 4", "Continue scan"), "Continue scan"
    )
    completed = ui.visible(
        "Scan complete", "Samples 4 of 4", "Queue report", "Reports 0"
    )
    expect_indicators(completed, 4, (fill, fill, fill, fill))

    ui.tap(completed, "Queue report")
    queued = ui.visible("Samples 4 of 4", "Reports 0", "Report queued")
    _, phase = sample(queued, 4)
    assert any(phase), "A running BusyIndicator did not paint its arc"

    def moved(screen: Screen) -> bool:
        if (
            screen.find("Report queued") is None
            or screen.find("Samples 4 of 4") is None
        ):
            return False
        _, next_phase = sample(screen, 4)
        return any(next_phase) and next_phase != phase

    spinning = ui.wait_for("busy arc rotates with no further input", moved, timeout=5)
    saved = ui.visible("Samples 4 of 4", "Reports 1", "Report saved", "Queue report")
    expect_indicators(saved, 4, (fill, fill, fill, fill))

    ui.tap(saved, "Queue report")
    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
    home = ui.wait_for(
        "Back hides the scan workshop with a report queued",
        lambda screen: (
            screen.find("Route home") is not None
            and screen.find("Scan workshop") is None
        ),
    )
    time.sleep(7)
    open_drawer_item(ui, home, "Open scan")
    ui.visible("Scan workshop", "Samples 4 of 4", "Reports 1", "Report queued")
    reactivated = ui.visible("Samples 4 of 4", "Reports 2", "Report saved")
    expect_indicators(reactivated, 4, (fill, fill, fill, fill))
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "scan-halfway.png").write_bytes(halfway.png)
    (ui.artifacts / "scan-report-pending.png").write_bytes(spinning.png)
    (ui.artifacts / "scan-report-complete.png").write_bytes(reactivated.png)


def exercise(
    ui: Emulator, apk: Path, *, images_only: bool = False, scan_only: bool = False
) -> None:
    rotation = ui.call("shell", "cmd", "window", "user-rotation").decode().split()
    try:
        ui.call("shell", "cmd", "window", "user-rotation", "lock", "0")
        ui.call("install", "-r", str(apk))
        ui.call("shell", "am", "force-stop", PACKAGE)
        ui.call("shell", "am", "start", "-W", "-f", "0x10008000", "-n", ACTIVITY)
        initial = ui.visible("Mode Balanced", "Settings", timeout=75)
        frame = coordinates(ui, initial)
        if images_only:
            exercise_images(ui, initial, frame)
            return
        if scan_only:
            exercise_scan(ui, initial, frame)
            return
        ui.tap(initial, "Settings")
        ui.wait_for(
            "initial paint reaches the native surface",
            lambda screen: (
                screen.find("Pick color") is not None
                and saved_color(screen, frame, (64, 128, 192))
            ),
        )
        ui.tap(ui.visible("Pick color"), "Pick color")
        shown = above_keyboard(ui, frame, "Paint color", "Save color")
        enabled = button_fill(shown, frame, "Save color")
        set_hex(ui, frame, "#12")
        invalid = ui.wait_for(
            "invalid hex disables confirmation",
            lambda screen: (
                screen.find("Save color") is not None
                and button_fill(screen, frame, "Save color") != enabled
            ),
        )
        ui.tap(invalid, "Save color")
        ui.visible("Paint color")
        set_hex(ui, frame, "#204060FF")
        ui.wait_for(
            "valid hex enables confirmation",
            lambda screen: (
                screen.find("Save color") is not None
                and button_fill(screen, frame, "Save color") == enabled
            ),
        )
        hide_keyboard(ui)
        color = ui.visible("Paint color", "Save color", "Opacity")
        hex_label = next(label for label in color.labels if label.text.startswith("#"))
        preview_point = (round(100 * frame.scale), round(hex_label.y * color.height))
        before_drag = pixel_rgb(color.png, *preview_point)
        # The final body row is the 132px color plane, followed by a 12px gap.
        save = color.require("Save color")
        y = save.y * color.height - 100 * frame.scale
        drag(
            ui,
            frame,
            (round(80 * frame.scale), round(y - 24 * frame.scale)),
            (round(250 * frame.scale), round(y + 24 * frame.scale)),
        )
        ui.wait_for(
            "two-axis touch updates the color preview",
            lambda screen: (
                screen.find("Paint color") is not None
                and pixel_rgb(screen.png, *preview_point) != before_drag
            ),
        )
        ui.tap(ui.visible("Cancel", "Save color"), "Cancel")
        ui.wait_for(
            "cancel keeps the saved color",
            lambda screen: (
                screen.find("Color unchanged") is not None
                and saved_color(screen, frame, (64, 128, 192))
            ),
        )

        ui.tap(ui.visible("Pick color"), "Pick color")
        set_hex(ui, frame, "#204060FF")
        ui.tap(ui.visible("Paint color", "Save color"), "Save color")
        committed = ui.wait_for(
            "the accepted color updates native pixels",
            lambda screen: (
                screen.find("Color saved") is not None
                and saved_color(screen, frame, (32, 64, 96))
            ),
        )
        ui.artifacts.mkdir(parents=True, exist_ok=True)
        (ui.artifacts / "theme-color.png").write_bytes(committed.png)
        hide_keyboard(ui)

        ui.tap(ui.visible("Pick font"), "Pick font")
        family(ui)
        ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
        ui.visible("Font unchanged")
        ui.tap(ui.visible("Pick font"), "Pick font")
        family(ui)
        number(ui, frame, "Size (px)", "24.0")
        number(ui, frame, "Weight", "500")
        ui.tap(ui.visible("Regular", "Save font"), "Regular")
        ui.tap(ui.visible("Italic"), "Italic")
        font = ui.visible("Roboto", "24.0", "500", "Italic", "Save font")
        (ui.artifacts / "theme-font.png").write_bytes(font.png)
        ui.tap(font, "Save font")
        ui.visible("Font saved Roboto")

        ui.tap(ui.visible("Pick color"), "Pick color")
        above_keyboard(ui, frame, "Paint color", "Save color")
        hide_keyboard(ui)
        ui.call("shell", "cmd", "window", "user-rotation", "lock", "1")
        landscape = ui.wait_for(
            "landscape keeps color actions visible",
            lambda screen: (
                screen.width > screen.height
                and screen.find("Paint color") is not None
                and screen.find("Save color") is not None
                and screen.find("Cancel") is not None
            ),
        )
        (ui.artifacts / "theme-landscape.png").write_bytes(landscape.png)
        ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
        ui.call("shell", "cmd", "window", "user-rotation", "lock", "0")
        final = ui.wait_for(
            "saved paint and font survive popup cancellation",
            lambda screen: (
                screen.find("Color unchanged") is not None
                and saved_color(screen, frame, (32, 64, 96))
            ),
        )
        (ui.artifacts / "theme-result.png").write_bytes(final.png)
        print(
            "PASS color validation, native pixels, font catalog and constrained picker layout",
            flush=True,
        )
    finally:
        ui.call("shell", "cmd", "window", "user-rotation", *rotation)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Exercise Android pickers, images and progress indicators"
    )
    parser.add_argument("--serial", required=True)
    parser.add_argument("--apk", required=True, type=Path)
    parser.add_argument("--adb", required=True, type=Path)
    scenario = parser.add_mutually_exclusive_group()
    scenario.add_argument("--images-only", action="store_true")
    scenario.add_argument("--scan-only", action="store_true")
    args = parser.parse_args()
    adb = args.adb.resolve()
    if not adb.is_file() or not args.apk.is_file():
        parser.error("Both adb and the navigation APK must exist")
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
            ThemeEmulator(adb, args.serial, recognize, artifacts),
            args.apk.resolve(),
            images_only=args.images_only,
            scan_only=args.scan_only,
        )


if __name__ == "__main__":
    main()
