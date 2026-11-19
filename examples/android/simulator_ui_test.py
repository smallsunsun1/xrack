import argparse
import os
import re
import shlex
import struct
import subprocess
import tempfile
import time
import xml.etree.ElementTree as ET
import zlib
from collections.abc import Iterator
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path
from typing import Callable


PACKAGE = "dev.xrack.android_demo"
ACTIVITY = f"{PACKAGE}/dev.xrack.platform.XRackActivity"


def normalize(text: str) -> str:
    return "".join(character for character in text.casefold() if character.isalnum())


def pixel_rgb(png: bytes, x: int, y: int) -> tuple[int, int, int]:
    return pixels_rgb(png, ((x, y),))[0]


def pixels_rgb(
    png: bytes, points: tuple[tuple[int, int], ...]
) -> tuple[tuple[int, int, int], ...]:
    """Read pixels from the emulator's non-interlaced 8-bit RGB/RGBA PNG."""
    width, height, depth, color, compression, filtering, interlace = struct.unpack_from(
        ">IIBBBBB", png, 16
    )
    if depth != 8 or color not in (2, 6) or compression or filtering or interlace:
        raise AssertionError("Unsupported Android screenshot PNG format")
    if not all(0 <= x < width and 0 <= y < height for x, y in points):
        raise AssertionError(f"Pixel coordinates {points} are outside {width}x{height}")
    channels = 4 if color == 6 else 3
    chunks: list[bytes] = []
    offset = 8
    while offset < len(png):
        length = struct.unpack_from(">I", png, offset)[0]
        if png[offset + 4 : offset + 8] == b"IDAT":
            chunks.append(png[offset + 8 : offset + 8 + length])
        offset += 12 + length
    raw = zlib.decompress(b"".join(chunks))
    stride = width * channels
    previous = bytearray(stride)
    sampled: dict[tuple[int, int], tuple[int, int, int]] = {}
    for row_index in range(max(y for _, y in points) + 1):
        offset = row_index * (stride + 1)
        method = raw[offset]
        row = bytearray(raw[offset + 1 : offset + 1 + stride])
        if method > 4:
            raise AssertionError(f"Unknown PNG filter {method}")
        if method:
            for index in range(stride):
                left = row[index - channels] if index >= channels else 0
                above = previous[index]
                corner = previous[index - channels] if index >= channels else 0
                if method == 1:
                    prediction = left
                elif method == 2:
                    prediction = above
                elif method == 3:
                    prediction = (left + above) // 2
                else:
                    p = left + above - corner
                    distances = [abs(p - left), abs(p - above), abs(p - corner)]
                    prediction = (left, above, corner)[distances.index(min(distances))]
                row[index] = (row[index] + prediction) & 255
        previous = row
        for x, y in points:
            if row_index == y:
                pixel = x * channels
                sampled[x, y] = row[pixel], row[pixel + 1], row[pixel + 2]
    return tuple(sampled[point] for point in points)


@dataclass(frozen=True)
class Label:
    x: float
    y: float
    text: str


@dataclass(frozen=True)
class Screen:
    png: bytes
    width: int
    height: int
    labels: list[Label]

    def find(self, text: str) -> Label | None:
        wanted = normalize(text)
        return next(
            (label for label in self.labels if normalize(label.text) == wanted), None
        )

    def require(self, text: str) -> Label:
        label = self.find(text)
        if label is None:
            raise AssertionError(f"Text disappeared from the screen: {text}")
        return label


class Emulator:
    def __init__(
        self, adb: Path, serial: str, recognize: Path, artifacts: Path
    ) -> None:
        self.adb = adb
        self.serial = serial
        self.recognize = recognize
        self.artifacts = artifacts

    def call(self, *args: str) -> bytes:
        return subprocess.check_output([self.adb, "-s", self.serial, *args], timeout=45)

    def type_text(self, text: str) -> None:
        # ADB timestamps every event in one `input text` command up front.
        # Generate each character separately so slow frames cannot make later
        # characters expire in Android's input dispatcher.
        for character in text:
            argument = "%s" if character == " " else character
            self.call("shell", "input", "text", shlex.quote(argument))

    def screen(self) -> Screen:
        png = self.call("exec-out", "screencap", "-p")
        if not png.startswith(b"\x89PNG\r\n\x1a\n"):
            raise RuntimeError("ADB did not return a PNG screenshot")
        width, height = struct.unpack_from(">II", png, 16)
        output = subprocess.check_output([self.recognize], input=png, timeout=20)
        labels: list[Label] = []
        for line in output.decode().splitlines():
            x, y, text = line.split("\t", 2)
            labels.append(Label(float(x), float(y), text))
        return Screen(png, width, height, labels)

    def wait_for(
        self,
        description: str,
        matches: Callable[[Screen], bool],
        *,
        timeout: float = 35,
    ) -> Screen:
        deadline = time.monotonic() + timeout
        while True:
            screen = self.screen()
            if matches(screen):
                print(f"PASS {description}", flush=True)
                return screen
            if time.monotonic() >= deadline:
                self.artifacts.mkdir(parents=True, exist_ok=True)
                (self.artifacts / "last-screen.png").write_bytes(screen.png)
                rendered = ", ".join(label.text for label in screen.labels)
                raise AssertionError(
                    f"Timed out waiting for {description}. Recognized: {rendered}"
                )
            time.sleep(0.4)

    def visible(self, *texts: str, timeout: float = 35) -> Screen:
        return self.wait_for(
            ", ".join(texts),
            lambda screen: all(screen.find(text) for text in texts),
            timeout=timeout,
        )

    def tap(self, screen: Screen, text: str, *, hold_ms: int = 0) -> None:
        label = screen.require(text)
        self.touch(
            round(label.x * screen.width), round(label.y * screen.height), hold_ms
        )

    def touch(self, x: int, y: int, hold_ms: int = 0) -> None:
        column, row = str(x), str(y)
        if hold_ms:
            self.call("shell", "input", "swipe", column, row, column, row, str(hold_ms))
        else:
            self.call("shell", "input", "tap", column, row)

    def native_position(
        self, text: str, *, resource_id: str | None = None
    ) -> tuple[int, int]:
        _, point = self.native_position_one_of((text, resource_id))
        return point

    def native_position_one_of(
        self, *targets: tuple[str, str | None]
    ) -> tuple[str, tuple[int, int]]:
        dump = f"/data/local/tmp/xrack-ui-{os.getpid()}.xml"
        deadline = time.monotonic() + 45
        try:
            while time.monotonic() < deadline:
                try:
                    response = self.call("shell", "uiautomator", "dump", dump)
                except subprocess.CalledProcessError as error:
                    if error.returncode != 137:
                        raise
                    time.sleep(0.2)
                    continue
                if b"dumped to" not in response:
                    continue
                tree = ET.fromstring(self.call("exec-out", "cat", dump))
                for text, resource_id in targets:
                    expected = text.casefold()
                    for node in tree.iter("node"):
                        if (
                            resource_id is not None
                            and node.get("resource-id") != resource_id
                        ):
                            continue
                        if expected not in (
                            node.get("text", "").casefold(),
                            node.get("content-desc", "").casefold(),
                        ):
                            continue
                        x1, y1, x2, y2 = map(
                            int, re.findall(r"-?\d+", node.attrib["bounds"])
                        )
                        if x2 > x1 and y2 > y1:
                            print(f"PASS Android UI {text}", flush=True)
                            return text, ((x1 + x2) // 2, (y1 + y2) // 2)
        finally:
            self.call("shell", "rm", "-f", dump)
        picture = self.screen()
        self.artifacts.mkdir(parents=True, exist_ok=True)
        (self.artifacts / "last-screen.png").write_bytes(picture.png)
        raise AssertionError(
            f"Android UI did not show {' or '.join(text for text, _ in targets)}"
        )

    def tap_native(
        self, text: str, *, resource_id: str | None = None, hold_ms: int = 0
    ) -> None:
        self.touch(*self.native_position(text, resource_id=resource_id), hold_ms)

    def cancel_picker(self) -> None:
        deadline = time.monotonic() + 20
        sent_back = False
        while time.monotonic() < deadline:
            activities = self.call(
                "shell", "dumpsys", "activity", "activities"
            ).decode()
            resumed = next(
                (
                    line
                    for line in activities.splitlines()
                    if "topResumedActivity=" in line
                ),
                "",
            )
            if f" {PACKAGE}/" in resumed:
                return
            if not sent_back or ".picker.PickActivity" in resumed:
                # DocumentsUI can use Back to leave a directory before closing.
                # Check current focus as well, so delayed lifecycle reporting
                # cannot cause a second Back to be sent to XRack.
                focused = re.search(
                    r"FocusedWindows:\s+displayId=0, name='([^']+)'",
                    self.call("shell", "dumpsys", "input").decode(),
                )
                if focused is not None and (
                    focused[1].endswith(".picker.PickActivity")
                    or (not sent_back and "ChooserActivity" in focused[1])
                ):
                    self.call("shell", "input", "keyevent", "KEYCODE_BACK")
                    sent_back = True
            time.sleep(0.6)
        raise AssertionError(
            "The system picker did not return to the application after cancellation"
        )

    def window_accepts_touch(
        self, screen: Screen, activity: str, *, full_screen: bool = True
    ) -> bool:
        # Android can show a saved frame before the Activity can receive touch.
        dispatcher = (
            self.call("shell", "dumpsys", "input")
            .decode()
            .split("Input Dispatcher State at time of last ANR:", 1)[0]
        )
        focus = re.search(r"FocusedWindows:\s+displayId=0, name='([^']+)'", dispatcher)
        if focus is None or not focus[1].endswith(f" {activity}"):
            return False
        for line in dispatcher.splitlines():
            if f"name='{focus[1]}', id=" not in line or "inputConfig=0x0," not in line:
                continue
            region = re.search(
                r"touchableRegion=\[(-?\d+),(-?\d+)\]\[(-?\d+),(-?\d+)\],", line
            )
            if region is None:
                return False
            left, top, right, bottom = map(int, region.groups())
            if full_screen:
                return (left, top, right, bottom) == (0, 0, screen.width, screen.height)
            return left < right and top < bottom
        return False

    def send_touch(self, *events: str) -> None:
        response = self.call("emu", "event", "send", *events, "EV_SYN:0:0")
        if response.strip() != b"OK":
            raise RuntimeError(
                "ADB did not accept the touch event on the emulator console"
            )

    @contextmanager
    def press_contact(self, point: tuple[float, float]) -> Iterator[None]:
        # The emulator touch device uses a 15-bit absolute axis. Queue movement
        # independently of app rendering, which can stall adb shell input swipe.
        x, y = [round(32767 * value) for value in point]
        self.send_touch(
            "EV_ABS:ABS_MT_SLOT:0",
            "EV_ABS:ABS_MT_TRACKING_ID:1",
            f"EV_ABS:ABS_MT_POSITION_X:{x}",
            f"EV_ABS:ABS_MT_POSITION_Y:{y}",
            "EV_ABS:ABS_MT_PRESSURE:600",
            "EV_KEY:BTN_TOUCH:1",
        )
        try:
            yield
        finally:
            self.send_touch(
                "EV_ABS:ABS_MT_TRACKING_ID:-1",
                "EV_KEY:BTN_TOUCH:0",
            )

    def move_contact(
        self, first: tuple[float, float], last: tuple[float, float]
    ) -> None:
        x0, y0 = first
        x1, y1 = last
        for step in range(1, 5):
            time.sleep(0.06)
            x = round(32767 * (x0 + (x1 - x0) * step / 4))
            y = round(32767 * (y0 + (y1 - y0) * step / 4))
            self.send_touch(
                f"EV_ABS:ABS_MT_POSITION_X:{x}",
                f"EV_ABS:ABS_MT_POSITION_Y:{y}",
            )

    @contextmanager
    def drag_contact(
        self, first: tuple[float, float], last: tuple[float, float]
    ) -> Iterator[None]:
        with self.press_contact(first):
            self.move_contact(first, last)
            yield

    def scroll_list(self, direction: int) -> None:
        start, end = (0.88, 0.53) if direction > 0 else (0.53, 0.88)
        with self.drag_contact((0.5, start), (0.5, end)):
            pass


def exercise_row_swipes(ui: Emulator) -> None:
    first_task = "Check pump pressure"
    neighbor = "Verify door interlock"
    initial = ui.visible(first_task, neighbor, "Open 9", "Done 3", timeout=75)
    title = initial.require(first_task)

    def closed(description: str) -> Screen:
        return ui.wait_for(
            description,
            lambda screen: (
                (row := screen.find(first_task)) is not None
                and abs(row.x - title.x) < 0.015
                and screen.find("Open 9") is not None
                and screen.find("Done 3") is not None
                and screen.find("Complete") is None
                and screen.find("Mark complete") is None
            ),
        )

    def opened(action: str, *state: str) -> Screen:
        # An open panel shifts the task body under the left clip. Match the
        # unique visible suffix and require the action on that same row.
        def matches(screen: Screen) -> bool:
            button = screen.find(action)
            return (
                button is not None
                and button.x > 0.70
                and all(screen.find(text) for text in state)
                and any(
                    normalize(row.text).endswith("umppressure")
                    and abs(button.y - row.y) < 0.035
                    for row in screen.labels
                )
            )

        return ui.wait_for(
            f"{action} is exposed on the pump task",
            matches,
        )

    def reveal(screen: Screen) -> None:
        y = screen.require(first_task).y
        with ui.drag_contact((0.66, y), (0.35, y)):
            pass

    with ui.drag_contact((0.66, title.y), (0.59, title.y)):
        ui.wait_for(
            "a short horizontal drag previews the pump row",
            lambda screen: (
                (row := screen.find(first_task)) is not None
                and row.x < title.x - 0.035
                and screen.find("Open 9") is not None
            ),
        )
    restored = closed("releasing before half width restores the row")
    y = restored.require(first_task).y
    with ui.drag_contact((0.66, y), (0.35, y)):
        opened("Complete", "Open 9", "Done 3")
        ui.call("shell", "input", "keyevent", "KEYCODE_HOME")
        ui.wait_for(
            "Android backgrounds the task list while the swipe is held",
            lambda screen: screen.find("Round progress") is None,
        )
    ui.call("shell", "am", "start", "-W", "-n", ACTIVITY)
    ui.wait_for(
        "the task list can receive touch after returning to the foreground",
        lambda screen: (
            screen.find(first_task) is not None
            and ui.window_accepts_touch(screen, ACTIVITY)
        ),
        timeout=75,
    )
    reveal(closed("the interrupted swipe keeps the task and restores the closed row"))
    complete = opened("Complete", "Open 9", "Done 3")
    button = complete.require("Complete")
    # Start inside the exposed panel at its inner edge so the closing drag
    # can travel far enough without crossing the display edge.
    with ui.drag_contact((button.x - 0.09, button.y), (0.97, button.y)):
        pass
    reveal(closed("a reverse swipe closes the action without completing the task"))
    opened("Complete", "Open 9", "Done 3")
    ui.call("shell", "input", "keyevent", "--longpress", "KEYCODE_BACK")
    reveal(closed("Android Back closes the focused row without changing the model"))

    exposed = opened("Complete", "Open 9", "Done 3")
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "swipe-complete.png").write_bytes(exposed.png)
    button = exposed.require("Complete")
    with ui.drag_contact((button.x, button.y), (button.x, button.y - 0.25)):
        pass
    ui.wait_for(
        "vertical drag on the action scrolls the list without activating it",
        lambda screen: (
            screen.find(first_task) is None
            and screen.find("Complete") is None
            and screen.find("Open 9") is not None
            and screen.find("Done 3") is not None
            and any(
                screen.find(task) is not None
                for task in (
                    "Confirm sensor calibration",
                    "Inspect return valve",
                    "Review calibration logs",
                )
            )
        ),
    )
    ui.scroll_list(-1)
    ui.tap(opened("Complete", "Open 9", "Done 3"), "Complete")
    completed = ui.wait_for(
        "completing removes only the pump task and leaves its neighbor closed",
        lambda screen: (
            screen.find("Open 8") is not None
            and screen.find("Done 4") is not None
            and screen.find(neighbor) is not None
            and screen.find(first_task) is None
            and screen.find("Complete") is None
        ),
    )
    ui.tap(completed, "Done 4")
    reveal(ui.visible(first_task, "Record meter reading", "Done 4"))
    reopened = opened("Reopen", "Open 8", "Done 4")
    (ui.artifacts / "swipe-reopen.png").write_bytes(reopened.png)
    ui.tap(reopened, "Reopen")
    ui.tap(ui.visible("Open 9", "Done 3", "Record meter reading"), "Open 9")
    closed("reopening restores the pump task with its action closed")


def exercise_native_dialogs(ui: Emulator) -> None:
    ui.tap(ui.visible("Open 9", "Notes", timeout=75), "Notes")
    placeholder = "Record findings and follow-up steps"
    ui.tap(ui.visible("Field notes", placeholder), placeholder)
    ui.type_text("Original")
    ui.call("shell", "input", "keyevent", "KEYCODE_ENTER")
    ui.visible("Original")
    ui.tap(ui.visible("Tasks"), "Tasks")
    ui.tap(ui.visible("Notes", "Open 9"), "Notes")
    ui.tap(ui.visible("Original", "Import"), "Import")
    ui.native_position("Show roots")
    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
    ui.visible("Import canceled", "Original", timeout=75)

    with tempfile.TemporaryDirectory(prefix="xrack-dialog-") as temporary:
        files: list[str] = []
        try:
            for index, (extension, content) in enumerate(
                (
                    ("txt", "Android first note"),
                    ("md", "Android second note"),
                    ("bin", "Excluded document"),
                )
            ):
                path = Path(temporary) / f"{Path(temporary).name}-{index}.{extension}"
                path.write_text(content)
                remote = f"/sdcard/Download/{path.name}"
                files.append(remote)
                ui.call("push", str(path), remote)

            buttons = {
                "Yes": "android:id/button1",
                "No": "android:id/button2",
                "Cancel": "android:id/button3",
            }
            for confirmation in ("No", "Cancel", "Back", "Yes"):
                ui.tap(ui.visible("Import"), "Import")
                ui.tap_native("Show roots")
                ui.native_position("Open from")
                ui.tap_native("Downloads", resource_id="android:id/title")
                ui.native_position("Files in Downloads")
                if confirmation == "No":
                    # The system picker displays incompatible files but must not select them.
                    ui.tap_native(Path(files[2]).name)
                if confirmation in ("No", "Yes"):
                    ui.tap_native(Path(files[0]).name, hold_ms=800)
                    ui.tap_native(Path(files[1]).name)
                    ui.native_position("2 selected")
                    ui.tap_native("Select")
                    selection = "2 selected files"
                else:
                    ui.tap_native(Path(files[0]).name)
                    selection = "1 selected file"
                ui.native_position("Replace current notes?")
                ui.native_position(
                    f"{selection}\n\nChoosing Yes replaces the text in the editor.",
                    resource_id="android:id/message",
                )
                if confirmation == "Back":
                    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
                else:
                    ui.tap_native(confirmation, resource_id=buttons[confirmation])
                if confirmation == "No":
                    ui.visible("Current notes kept", "Original", timeout=75)
                elif confirmation in ("Cancel", "Back"):
                    ui.visible("Replacement canceled", "Original", timeout=75)
                else:
                    ui.visible(
                        "2 notes imported",
                        "Android first note",
                        "Android second note",
                        timeout=75,
                    )
        finally:
            if files:
                ui.call("shell", "rm", "-f", *files)


def exercise_storage(ui: Emulator) -> None:
    ui.tap(ui.visible("Open 9", "Notes", timeout=75), "Notes")
    ui.tap(ui.visible("Folder"), "Folder")
    ui.native_position("Use this folder")
    ui.cancel_picker()
    ui.visible("Folder import canceled", timeout=75)

    def select_folder(name: str) -> None:
        ui.tap(ui.visible("Folder"), "Folder")
        opened = f"Files in {name}"
        location, point = ui.native_position_one_of(
            (opened, None), (name, "android:id/title"), ("Documents", None)
        )
        if location == "Documents":
            ui.touch(*point)
            ui.tap_native(name, resource_id="android:id/title")
        elif location == name:
            ui.touch(*point)
        ui.native_position(opened)
        ui.tap_native("Use this folder")
        ui.tap_native("Allow")

    with tempfile.TemporaryDirectory(prefix="xrack-storage-") as temporary:
        directory = Path(temporary)
        remote = f"/sdcard/Documents/{directory.name}"
        output_name = f"xr-{directory.name.removeprefix('xrack-storage-')}.txt"
        exported = f"/sdcard/Download/{output_name}"
        expected = "Android folder note\n巡检通过 🚰\n\nSecond folder note"
        first = directory / "first-巡检.md"
        second = directory / "second.txt"
        try:
            ui.call("shell", "mkdir", "-p", f"{remote}/nested.txt")
            for path, destination, content in (
                (first, first.name, "Android folder note\n巡检通过 🚰"),
                (second, second.name, "Second folder note"),
                (directory / "ignored.bin", "ignored.bin", "Ignored content"),
                (directory / "nested.md", "nested.txt/nested.md", "Nested content"),
            ):
                path.write_text(content, encoding="utf-8")
                ui.call("push", str(path), f"{remote}/{destination}")
            select_folder(directory.name)
            imported = ui.visible(
                "2 imported from folder",
                "Android folder note",
                "Second folder note",
                timeout=75,
            )
            ui.tap(imported, "Save to Files")
            ui.native_position("inspection-notes.txt")
            ui.cancel_picker()
            ui.visible("Save canceled", "Android folder note", timeout=75)
            ui.tap(ui.visible("Save to Files"), "Save to Files")
            ui.tap_native("Show roots")
            ui.native_position("Save to")
            ui.tap_native("Downloads", resource_id="android:id/title")
            ui.tap_native("inspection-notes.txt")
            ui.call(
                "shell", "input", "keycombination", "KEYCODE_CTRL_LEFT", "KEYCODE_A"
            )
            ui.type_text(output_name)
            ui.native_position(output_name)
            ui.tap_native("Save")
            ui.visible(
                "Saved to Files",
                "Android folder note",
                "Second folder note",
                timeout=75,
            )
            actual = ui.call("exec-out", "cat", exported).decode()
            if actual != expected:
                raise AssertionError(
                    f"The exported text differs from the imported notes: {actual!r}"
                )
            print("PASS exported file contains exactly the imported notes", flush=True)

            first.write_text("Incomplete batch")
            second.write_bytes(b"\xff")
            for path in (first, second):
                ui.call("push", str(path), f"{remote}/{path.name}")
            select_folder(directory.name)
            ui.wait_for(
                "an invalid UTF-8 note rejects the entire folder import",
                lambda screen: (
                    screen.find("Android folder note") is not None
                    and screen.find("Second folder note") is not None
                    and any(
                        "invalid utf-8" in label.text.lower() for label in screen.labels
                    )
                    and screen.find("Incomplete batch") is None
                ),
                timeout=75,
            )

            # Each file fits on its own. Their combined bytes and separator do not.
            for path in (first, second):
                path.write_bytes(b"x" * (32 * 1024))
                ui.call("push", str(path), f"{remote}/{path.name}")
            select_folder(directory.name)
            oversized = ui.wait_for(
                "an oversized folder batch leaves the notes intact",
                lambda screen: (
                    screen.find("Android folder note") is not None
                    and screen.find("Second folder note") is not None
                    and "exceedstherequestedbytelimit"
                    in normalize(" ".join(label.text for label in screen.labels))
                    and screen.find("Folder") is not None
                    and screen.find("Save to Files") is not None
                ),
                timeout=75,
            )
            ui.artifacts.mkdir(parents=True, exist_ok=True)
            (ui.artifacts / "storage-rejected-note.png").write_bytes(oversized.png)

            ui.tap(ui.visible("Android folder note"), "Android folder note")
            ui.call(
                "shell", "input", "keycombination", "KEYCODE_CTRL_LEFT", "KEYCODE_A"
            )
            ui.type_text("New draft")
            draft = ui.wait_for(
                "the editor displays the replacement draft",
                lambda screen: (
                    screen.find("Tasks") is not None
                    and any(
                        normalize(label.text).startswith("newdraf")
                        for label in screen.labels
                    )
                ),
            )
            ui.tap(draft, "Tasks")
            ui.tap(ui.visible("Notes", "Open 9"), "Notes")
            ui.tap(ui.visible("New draft", "Import"), "Import")
            ui.tap_native("Show roots")
            ui.tap_native("Downloads", resource_id="android:id/title")
            ui.tap_native(output_name, resource_id="android:id/title")
            ui.native_position("Replace current notes?")
            ui.tap_native("Yes", resource_id="android:id/button1")
            ui.visible(
                "Note imported", "Android folder note", "Second folder note", timeout=75
            )
        finally:
            ui.call("shell", "rm", "-rf", remote)
            ui.call("shell", "rm", "-f", exported)


def exercise_sharing(ui: Emulator, receiver: Path) -> None:
    ui.call("install", "-r", str(receiver))
    fixture = f"xrack-share-{os.getpid()}.txt"
    remote = f"/sdcard/Download/{fixture}"
    shared_text = "Android share note\n巡检通过 🚰\nSecond line"
    try:
        with tempfile.TemporaryDirectory(prefix="xrack-share-") as temporary:
            note = Path(temporary) / fixture
            note.write_text(shared_text)
            ui.call("push", str(note), remote)
        ui.tap(ui.visible("Open 9", "Notes", timeout=75), "Notes")
        ui.tap(ui.visible("Import"), "Import")
        ui.tap_native("Show roots")
        ui.native_position("Open from")
        ui.tap_native("Downloads", resource_id="android:id/title")
        ui.tap_native(fixture, resource_id="android:id/title")
        ui.native_position("Import notes?")
        ui.tap_native("Yes", resource_id="android:id/button1")
        ui.visible(
            "Note imported", "Android share note", "Second line", "Share", timeout=75
        )

        def cancel() -> None:
            ui.tap(ui.visible("Share"), "Share")
            ui.native_position("XRack Test Receiver")
            ui.cancel_picker()
            ui.visible(
                "Share canceled", "Android share note", "Second line", timeout=75
            )

        def choose(expected_text: str, *note_lines: str) -> None:
            ui.tap(ui.visible("Share"), "Share")
            ui.tap_native("XRack Test Receiver")
            ui.native_position("Received note")
            ui.native_position("text/plain")
            ui.native_position(expected_text)
            ui.tap_native("Return to Pocket Tasks")
            ui.visible("Share target selected", *note_lines, timeout=75)

        cancel()
        choose(shared_text, "Android share note", "Second line")
        cancel()
        ui.tap(ui.visible("Second line"), "Second line")
        ui.call("shell", "input", "keycombination", "KEYCODE_CTRL_LEFT", "KEYCODE_A")
        revised = "Fresh draft"
        ui.type_text(revised)
        ui.tap(ui.visible(revised, "Tasks"), "Tasks")
        ui.tap(ui.visible("Notes", "Open 9"), "Notes")
        ui.visible(revised, "Share")
        choose(revised, revised)
        print(
            "PASS repeated shares support cancellation and send the current note",
            flush=True,
        )
    finally:
        ui.call("shell", "rm", "-f", remote)
        ui.call("uninstall", "dev.xrack.share_test")


@contextmanager
def test_input_method(ui: Emulator, keyboard_apk: Path) -> Iterator[None]:
    keyboard = "dev.xrack.share_test/.TestInputMethodService"
    previous = (
        ui.call("shell", "settings", "get", "secure", "default_input_method")
        .decode()
        .strip()
    )
    ui.call("install", "-r", str(keyboard_apk))
    try:
        deadline = time.monotonic() + 15
        while (
            keyboard
            not in ui.call("shell", "ime", "list", "-a", "-s").decode().splitlines()
        ):
            if time.monotonic() >= deadline:
                raise AssertionError(
                    "Android did not register the installed test input method"
                )
            time.sleep(0.2)
        ui.call("shell", "ime", "enable", keyboard)
        ui.call("shell", "ime", "set", keyboard)
        yield
    finally:
        ui.call("shell", "ime", "set", previous)
        ui.call("shell", "ime", "disable", keyboard)
        ui.call("uninstall", "dev.xrack.share_test")


def exercise_ime(ui: Emulator, keyboard_apk: Path) -> None:
    with test_input_method(ui, keyboard_apk):
        ui.tap(ui.visible("Open 9", "Notes", timeout=75), "Notes")
        placeholder = "Record findings and follow-up steps"
        empty = ui.visible(placeholder)
        ui.tap(empty, placeholder)
        ui.visible("Compose ni")
        ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
        ui.wait_for(
            "keyboard is hidden",
            lambda screen: (
                screen.find("Field notes") is not None
                and screen.find("Compose ni") is None
            ),
        )
        ui.tap(empty, placeholder)
        ui.visible("Compose ni")
        ui.tap(ui.visible("Compose ni"), "Compose ni")
        ui.wait_for(
            "preedit is rendered",
            lambda screen: any(
                normalize(label.text).startswith("ni") for label in screen.labels
            ),
        )
        ui.tap(ui.visible("Backspace key"), "Backspace key")
        ui.visible("Editor buffer n")
        ui.tap(ui.visible("Capture"), "Capture")
        ui.tap(ui.visible("Tasks"), "Tasks")
        ui.tap(ui.visible("Notes"), "Notes")
        ui.tap(ui.visible(placeholder), placeholder)
        ui.tap(ui.visible("Old commit"), "Old commit")
        ui.visible("Old commit sent")
        ui.tap(ui.visible("Tasks"), "Tasks")
        ui.tap(ui.visible("Notes"), "Notes")
        ui.tap(ui.visible(placeholder), placeholder)
        ui.tap(ui.visible("Compose ni"), "Compose ni")
        ui.tap(ui.visible("Commit Unicode"), "Commit Unicode")
        ui.tap(ui.visible("Reverse select"), "Reverse select")
        ui.tap(ui.visible("Compose ni"), "Compose ni")
        ui.tap(ui.visible("Commit Unicode"), "Commit Unicode")
        ui.tap(ui.visible("Delete code"), "Delete code")
        ui.tap(ui.visible("New line"), "New line")
        ui.wait_for(
            "committed text is rendered",
            lambda screen: any(
                normalize(label.text).startswith("world") for label in screen.labels
            ),
        )
        ui.tap(ui.visible("Share"), "Share")
        ui.tap_native("XRack Test Receiver")
        ui.native_position("你好\nWorld")
        ui.tap_native("Return to Pocket Tasks")
        returned = ui.wait_for(
            "the share result is visible and Android routes touch to Pocket Tasks",
            lambda screen: (
                screen.find("Share target selected") is not None
                and screen.find("World") is not None
                and screen.find("Readings") is not None
                and ui.window_accepts_touch(screen, ACTIVITY)
            ),
            timeout=75,
        )
        ui.tap(returned, "Readings")
        ui.tap(ui.visible("Code"), "Code")
        ui.tap(ui.visible("Commit number"), "Commit number")
        ui.tap(ui.visible("Submit"), "Submit")
        ui.visible("Code 47")
        print(
            "PASS composition cancellation, stale connection rejection, Unicode commit and code point deletion",
            flush=True,
        )


def exercise_multiline_ime(ui: Emulator, keyboard_apk: Path) -> None:
    with test_input_method(ui, keyboard_apk):
        placeholder = "Record findings and follow-up steps"
        ui.tap(ui.visible("Open 9", "Notes", timeout=75), "Notes")
        ui.tap(ui.visible("Field notes", placeholder), placeholder)
        ui.visible("Return key")

        def press(key: str) -> None:
            ui.tap(ui.visible(key), key)

        def read_buffer_prefix(value: str) -> None:
            press("Read buffer")
            ui.wait_for(
                value,
                lambda screen: any(
                    normalize(label.text).startswith(normalize(value))
                    for label in screen.labels
                ),
            )

        ui.type_text("A")
        press("Commit Unicode")
        press("Return key")
        read_buffer_prefix("At end with 6 chars")
        ui.type_text("BZ")
        read_buffer_prefix("At end with 8 chars")
        press("Select inner")
        read_buffer_prefix("Selected 6 chars")
        selection = ui.screen()

        press("Backspace key")
        press("Read buffer")
        deleted = ui.visible("Before A after Z", "Tasks")
        ui.tap(deleted, "Tasks")
        ui.tap(ui.visible("Open 9", "Notes"), "Notes")
        returned = ui.visible("Field notes", "Routine", "Share")
        density = re.findall(
            r"(?:Physical|Override) density:\s*(\d+)",
            ui.call("shell", "wm", "density").decode(),
        )
        assert density, "Android did not report its display density"
        category = returned.require("Routine")
        # The note starts immediately below the category and read-only row.
        ui.touch(
            returned.width // 2,
            round(category.y * returned.height + 55 * int(density[-1]) / 160),
        )
        ui.visible("Return key")
        ui.call(
            "shell", "input", "keycombination", "KEYCODE_CTRL_LEFT", "KEYCODE_MOVE_END"
        )
        press("Read buffer")
        ui.visible("At end with 2 chars AZ")
        ui.artifacts.mkdir(parents=True, exist_ok=True)
        (ui.artifacts / "ime-multiline-selection.png").write_bytes(selection.png)
        (ui.artifacts / "ime-multiline-deleted.png").write_bytes(deleted.png)


def exercise_emoji(ui: Emulator, keyboard_apk: Path) -> None:
    sdk = int(ui.call("shell", "getprop", "ro.build.version.sdk"))
    assert sdk >= 31, "This test requires Android Canvas.drawGlyphs"
    densities = re.findall(
        r"(?:Physical|Override) density:\s*(\d+)",
        ui.call("shell", "wm", "density").decode(),
    )
    assert densities, "Android did not report its display density"
    scale = int(densities[-1]) / 160

    def pump_ink(
        screen: Screen, left: float, right: float, top: int, bottom: int
    ) -> bool:
        # U+1F6B0 in the system Noto face has a saturated blue background.
        # Only sample XRack content, above the native keyboard.
        points = tuple(
            (x, y)
            for y in range(max(0, top), min(screen.height, bottom), 3)
            for x in range(round(left * screen.width), round(right * screen.width), 3)
        )
        return (
            sum(b - r > 60 and b - g > 25 for r, g, b in pixels_rgb(screen.png, points))
            >= 12
        )

    def editor_emoji(screen: Screen) -> bool:
        if (read_only := screen.find("Read-only")) is None:
            return False
        row = read_only.y * screen.height
        return pump_ink(
            screen, 0.1, 0.52, round(row + 33 * scale), round(row + 88 * scale)
        )

    def task_emoji(screen: Screen) -> bool:
        if screen.find("Open 10") is None:
            return False
        for label in screen.labels:
            if normalize(label.text).startswith("sample"):
                row = label.y * screen.height
                return pump_ink(
                    screen, 0.1, 0.6, round(row - 14 * scale), round(row + 14 * scale)
                )
        return False

    with test_input_method(ui, keyboard_apk):
        ui.tap(ui.visible("Open 9", "Notes", timeout=75), "Notes")
        placeholder = "Record findings and follow-up steps"
        ui.tap(ui.visible("Field notes", placeholder), placeholder)
        ui.tap(ui.visible("Commit Unicode"), "Commit Unicode")
        editor = ui.wait_for(
            "XRack paints the color glyph in the notes editor", editor_emoji
        )

        ui.tap(editor, "Tasks")
        ui.tap(ui.visible("Open 9", "New task"), "New task")
        ui.visible("Commit Unicode")
        ui.type_text("Sample ")
        ui.tap(ui.visible("Commit Unicode"), "Commit Unicode")
        ui.tap(ui.visible("Submit"), "Submit")
        task = ui.wait_for(
            "XRack paints the color glyph in the new task label", task_emoji
        )
        ui.artifacts.mkdir(parents=True, exist_ok=True)
        (ui.artifacts / "emoji-notes.png").write_bytes(editor.png)
        (ui.artifacts / "emoji-task-label.png").write_bytes(task.png)


def exercise_contact_ime(ui: Emulator, keyboard_apk: Path) -> None:
    with test_input_method(ui, keyboard_apk):
        ui.tap(ui.visible("Open 9", "Readings", timeout=75), "Readings")
        readings = ui.visible("Code", "Reading")
        if readings.find("Contact details") is None:
            ui.scroll_list(1)
        ui.tap(ui.visible("Contact details"), "Contact details")
        ui.visible(
            "Inspection contact", "Telephone", "Website URL", "Email keyboard Next"
        )
        ui.tap(ui.visible("Submit"), "Submit")
        invalid = ui.visible("Submit sent", "Email keyboard Next", "Telephone")
        assert invalid.find("Telephone keyboard Next") is None

        ui.type_text("lab@x")
        ui.tap(ui.visible("Capture"), "Capture")
        ui.tap(ui.visible("Submit"), "Submit")
        emailed = ui.visible("Email: lab@x", "Telephone keyboard Next")
        ui.tap(emailed, "Old commit")
        ui.visible("Old commit sent", "Telephone keyboard Next")
        ui.tap(ui.visible("Commit number"), "Commit number")
        ui.visible("Commit number sent")
        ui.tap(ui.visible("Submit"), "Submit")
        still_phone = ui.visible(
            "Submit sent", "Telephone keyboard Next", "Email: lab@x"
        )
        assert still_phone.find("URL keyboard Done") is None

        ui.tap(still_phone, "Commit number")
        ui.visible("Commit number sent")
        ui.tap(ui.visible("Submit"), "Submit")
        ui.visible("Telephone: 4747", "URL keyboard Done")
        ui.type_text("x.test")
        ui.tap(ui.visible("Submit"), "Submit")
        saved = ui.wait_for(
            "Done submits the website and hides the keyboard",
            lambda screen: (
                screen.find("Inspection contact") is not None
                and screen.find("Website: x.test") is not None
                and screen.find("URL keyboard Done") is None
                and screen.find("Submit") is None
                and b"mInputShown=false" in ui.call("shell", "dumpsys", "input_method")
            ),
        )
        ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
        ui.tap(ui.visible("Contact details"), "Contact details")
        ui.visible("Inspection contact", "Website: x.test", "4747")
        ui.artifacts.mkdir(parents=True, exist_ok=True)
        (ui.artifacts / "contact-telephone-next.png").write_bytes(emailed.png)
        (ui.artifacts / "contact-website-done.png").write_bytes(saved.png)


def exercise_numeric_ime(ui: Emulator, keyboard_apk: Path) -> None:
    with test_input_method(ui, keyboard_apk):
        ui.tap(ui.visible("Open 9", "Readings", timeout=75), "Readings")
        density = re.findall(
            r"density:\s*(\d+)", ui.call("shell", "wm", "density").decode()
        )
        assert density, "The emulator did not report its display density"
        scale = int(density[-1]) / 160

        def state(samples: int, pressure: float, offset: float) -> Screen:
            def matches(screen: Screen) -> bool:
                for label in screen.labels:
                    text = label.text.casefold().replace("−", "-")
                    if "samples" not in text or "bar" not in text:
                        continue
                    text = re.sub(r"\bo(?=\s*(?:samples|bar|°))", "0", text)
                    # Vision occasionally renders the summary's minus as an underscore.
                    text = re.sub(r"_(?=\d)", "-", text)
                    numbers = re.findall(r"-?\d+(?:[.,]\d+)?", text)
                    if tuple(float(value.replace(",", ".")) for value in numbers) == (
                        samples,
                        pressure,
                        offset,
                    ):
                        return True
                return False

            return ui.wait_for(
                f"{samples} samples, {pressure} bar, {offset} degrees", matches
            )

        def focus(screen: Screen, caption: str, keyboard: str) -> None:
            label = screen.require(caption)
            ui.touch(
                round(label.x * screen.width),
                round(label.y * screen.height + 32 * scale),
            )
            ui.visible(keyboard, "Submit")

        def replace_draft(button: str) -> None:
            ui.call(
                "shell", "input", "keycombination", "KEYCODE_CTRL_LEFT", "KEYCODE_A"
            )
            ui.tap(ui.visible(button), button)
            ui.visible(f"{button} sent")

        def buffer_at_end(value: str) -> Screen:
            ui.tap(ui.visible("Read buffer"), "Read buffer")
            ui.visible("Read buffer sent")
            # Keep the separator and sign in this assertion, unlike Screen.find.
            expected = f"At end with {len(value)} chars {value}"
            expected_without_spaces = expected.replace(" ", "")
            return ui.wait_for(
                expected,
                lambda screen: any(
                    label.text.replace("−", "-").replace(" ", "")
                    == expected_without_spaces
                    for label in screen.labels
                ),
            )

        def done(samples: int, pressure: float, offset: float) -> Screen:
            ui.tap(ui.visible("Submit"), "Submit")
            state(samples, pressure, offset)
            return ui.wait_for(
                "Done hides the numeric input method",
                lambda screen: (
                    screen.find("Submit") is None
                    and b"mInputShown=false"
                    in ui.call("shell", "dumpsys", "input_method")
                ),
            )

        focus(
            ui.visible("Samples", "Pressure", "Offset"),
            "Samples",
            "Integer keyboard Done",
        )
        replace_draft("Commit number")
        buffer_at_end("47")
        state(0, 0.0, 0.0)
        counted = done(12, 0.0, 0.0)

        focus(counted, "Pressure", "Signed decimal keyboard Done")
        replace_draft("Dot decimal")
        decimal_draft = buffer_at_end("3,756")
        state(12, 0.0, 0.0)
        done(12, 3.76, 0.0)

        focus(state(12, 3.76, 0.0), "Pressure", "Signed decimal keyboard Done")
        replace_draft("Comma decimal")
        buffer_at_end("4,25")
        pressure_draft = state(12, 3.76, 0.0)
        pressure = pressure_draft.require("Pressure")
        offset = pressure_draft.require("Offset")
        # The decrease arrow is near the right edge of the Pressure field,
        # 42 logical pixels below its caption.
        ui.touch(
            round((pressure.x + (offset.x - pressure.x) * 0.62) * pressure_draft.width),
            round(pressure.y * pressure_draft.height + 42 * scale),
        )
        stepped = state(12, 3.25, 0.0)
        focus(stepped, "Offset", "Signed decimal keyboard Done")
        replace_draft("Negative")
        buffer_at_end("-2.44")
        ui.tap(ui.visible("Commit Unicode"), "Commit Unicode")
        ui.visible("Commit Unicode sent")
        buffer_at_end("-2.44")
        state(12, 3.25, 0.0)
        done(12, 3.25, -2.4)

        ui.artifacts.mkdir(parents=True, exist_ok=True)
        (ui.artifacts / "decimal-draft.png").write_bytes(decimal_draft.png)
        (ui.artifacts / "numeric-readings.png").write_bytes(state(12, 3.25, -2.4).png)


def leap_day_cell(screen: Screen) -> tuple[Label, float]:
    # February's grid contains January 29 as well as leap day. Use the last row.
    matches = [label for label in screen.labels if normalize(label.text) == "29"]
    if not matches:
        raise AssertionError("The February calendar did not render leap day")
    leap = max(matches, key=lambda label: label.y)
    tuesday = screen.require("27")
    return leap, (leap.x - tuesday.x) / 2


def tap_leap_day(ui: Emulator, screen: Screen) -> None:
    leap, _ = leap_day_cell(screen)
    ui.touch(round(leap.x * screen.width), round(leap.y * screen.height))


def exercise_pickers(ui: Emulator) -> None:
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    task = "Check pump pressure"
    ui.tap(ui.visible("Open 9", task, timeout=75), task, hold_ms=1600)
    ui.visible("Mark complete")
    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
    tasks = ui.wait_for(
        "Back closes the context menu without completing the task",
        lambda screen: (
            screen.find("Mark complete") is None
            and screen.find(task) is not None
            and screen.find("Open 9") is not None
        ),
    )
    ui.tap(tasks, "Readings")
    readings = ui.wait_for(
        "the Readings page is ready to open the date picker",
        lambda screen: (
            screen.find("2024-02-28") is not None
            and screen.find("12:30:41") is not None
            and ui.window_accepts_touch(screen, ACTIVITY)
        ),
    )
    ui.tap(readings, "2024-02-28")
    calendar = ui.visible("Inspection date", "2024-02", "Save date")
    (ui.artifacts / "date-picker.png").write_bytes(calendar.png)
    leap, spacing = leap_day_cell(calendar)
    month = calendar.require("2024-02")
    left = round((leap.x - 3 * spacing) * calendar.width)
    right = round((leap.x + 3 * spacing) * calendar.width)
    header = round(month.y * calendar.height)
    ui.touch(left, header)
    ui.visible("2024-02")
    ui.touch(right, header)
    ui.visible("2024-03")
    ui.touch(right, header)
    ui.visible("2024-04")
    ui.touch(right, header)
    ui.visible("2024-04")
    # Moving back must reach March, proving the extra Next did not enter May.
    ui.touch(left, header)
    ui.visible("2024-03")
    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
    ui.visible("Date unchanged", "2024-02-28")

    ui.tap(ui.visible("2024-02-28"), "2024-02-28")
    calendar = ui.visible("Inspection date", "2024-02", "Save date")
    tap_leap_day(ui, calendar)
    ui.tap(ui.visible("Cancel", "Save date"), "Cancel")
    ui.visible("Date unchanged", "2024-02-28")

    ui.tap(ui.visible("2024-02-28"), "2024-02-28")
    calendar = ui.visible("Inspection date", "2024-02", "Save date")
    leap, spacing = leap_day_cell(calendar)
    # March 2 is Saturday in the leap-day row and is disabled by the example.
    ui.touch(
        round((leap.x + 2 * spacing) * calendar.width), round(leap.y * calendar.height)
    )
    ui.tap(ui.visible("Save date"), "Save date")
    ui.visible("Date saved", "2024-02-28")

    ui.tap(ui.visible("2024-02-28"), "2024-02-28")
    calendar = ui.visible("Inspection date", "2024-02", "Save date")
    tap_leap_day(ui, calendar)
    ui.tap(ui.visible("Save date"), "Save date")
    ui.visible("Date saved", "2024-02-29")

    ui.tap(ui.visible("12:30:41"), "12:30:41")
    time_picker = ui.visible("Inspection time", "31", "AM", "Save time")
    (ui.artifacts / "time-picker.png").write_bytes(time_picker.png)
    ui.tap(time_picker, "31")
    ui.tap(ui.visible("AM", "Save time"), "AM")
    # 12 AM is outside the allowed 08:00–18:00 range. Saving must stay disabled.
    ui.tap(ui.visible("Save time"), "Save time")
    time.sleep(0.6)
    ui.visible("Inspection time", "Cancel")
    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
    ui.visible("Time unchanged", "12:30:41")

    ui.tap(ui.visible("12:30:41"), "12:30:41")
    ui.tap(ui.visible("Inspection time", "31", "Save time"), "31")
    ui.tap(ui.visible("Save time"), "Save time")
    ui.visible("Time saved", "12:31:41")

    def draft_minute_is_selected(screen: Screen) -> bool:
        minute = screen.find("32")
        seconds = screen.find("41")
        return (
            minute is not None
            and seconds is not None
            and abs(minute.y - seconds.y) < 0.015
            and screen.find("Save time") is not None
        )

    def rotate_pending_time(portrait: Screen) -> Screen:
        ui.tap(portrait, "12:31:41")
        ui.tap(ui.visible("Inspection time", "32", "Save time"), "32")
        ui.wait_for("minute 32 is selected before rotation", draft_minute_is_selected)
        ui.call("shell", "cmd", "window", "user-rotation", "lock", "1")
        # The input dispatcher can report the region in pre-rotation coordinates.
        return ui.wait_for(
            "the time draft is selected and interactive in landscape",
            lambda screen: (
                screen.width > screen.height
                and draft_minute_is_selected(screen)
                and ui.window_accepts_touch(screen, ACTIVITY, full_screen=False)
            ),
        )

    landscape = rotate_pending_time(ui.visible("12:31:41"))
    (ui.artifacts / "time-picker-landscape.png").write_bytes(landscape.png)
    ui.call("shell", "input", "keyevent", "KEYCODE_BACK")
    ui.call("shell", "cmd", "window", "user-rotation", "lock", "0")
    canceled = ui.wait_for(
        "canceling the landscape picker preserves the saved date and time in portrait",
        lambda screen: (
            screen.width < screen.height
            and all(
                screen.find(label)
                for label in ("Time unchanged", "12:31:41", "2024-02-29")
            )
            and ui.window_accepts_touch(screen, ACTIVITY, full_screen=False)
        ),
    )
    ui.tap(rotate_pending_time(canceled), "Save time")
    ui.visible("Time saved", "12:32:41", "2024-02-29")
    ui.call("shell", "cmd", "window", "user-rotation", "lock", "0")
    final = ui.wait_for(
        "the saved time and its original seconds survive the return to portrait",
        lambda screen: (
            screen.width < screen.height
            and all(
                screen.find(label) for label in ("Time saved", "12:32:41", "2024-02-29")
            )
        ),
    )
    ui.artifacts.mkdir(parents=True, exist_ok=True)
    (ui.artifacts / "pickers.png").write_bytes(final.png)


def exercise(
    ui: Emulator,
    apk: Path,
    dialogs_only: bool,
    storage_only: bool,
    receiver: Path | None,
    ime_only: bool,
    pickers_only: bool = False,
    row_swipes_only: bool = False,
    contact_ime_only: bool = False,
    numeric_ime_only: bool = False,
    multiline_ime_only: bool = False,
    emoji_only: bool = False,
) -> None:
    rotation = ui.call("shell", "cmd", "window", "user-rotation").decode().split()
    try:
        ui.call("shell", "cmd", "window", "user-rotation", "lock", "0")
        ui.call("install", "-r", str(apk))
        ui.call("shell", "am", "force-stop", PACKAGE)
        # Clear a picker activity left in this app's task by an interrupted test.
        ui.call("shell", "am", "start", "-W", "-f", "0x10008000", "-n", ACTIVITY)
        if pickers_only:
            exercise_pickers(ui)
            return
        if row_swipes_only:
            exercise_row_swipes(ui)
            return
        if dialogs_only:
            exercise_native_dialogs(ui)
            return
        if storage_only:
            exercise_storage(ui)
            return
        if receiver is not None:
            if emoji_only:
                exercise_emoji(ui, receiver)
            elif multiline_ime_only:
                exercise_multiline_ime(ui, receiver)
            elif contact_ime_only:
                exercise_contact_ime(ui, receiver)
            elif numeric_ime_only:
                exercise_numeric_ime(ui, receiver)
            elif ime_only:
                exercise_ime(ui, receiver)
            else:
                exercise_sharing(ui, receiver)
            return
        first_task = "Check pump pressure"
        ui.visible("Open 9", "Done 3", first_task, timeout=75)
        ui.scroll_list(1)
        ui.wait_for(
            "scrolling replaces the first visible task",
            lambda screen: (
                screen.find("Open 9") is not None
                and screen.find(first_task) is None
                and screen.find("Mark complete") is None
            ),
        )
        ui.scroll_list(-1)
        initial = ui.visible(first_task, "Open 9")

        task = initial.require(first_task)
        with ui.press_contact((task.x, task.y)):
            ui.visible("Mark complete")
        ui.tap(ui.visible("Mark complete"), "Mark complete")
        ui.visible("Open 8", "Done 4")
        ui.call("shell", "cmd", "window", "user-rotation", "lock", "1")
        wide = ui.wait_for(
            "landscape shows the list beside the task entry and preserves task counts",
            lambda screen: (
                screen.width > screen.height
                and screen.find("Done 4") is not None
                and (entry := screen.find("New task")) is not None
                and (open_tab := screen.find("Open 8")) is not None
                and open_tab.x > entry.x
                and open_tab.y < entry.y
            ),
        )
        ui.tap(wide, "Done 4")
        ui.visible("Done 4", first_task)
        ui.call("shell", "input", "keyevent", "KEYCODE_HOME")
        ui.call("shell", "am", "start", "-W", "-n", ACTIVITY)
        ui.visible("Done 4", first_task)
        ui.call("shell", "cmd", "window", "user-rotation", "lock", "0")
        completed = ui.wait_for(
            "the completed item is still selected in portrait after resuming",
            lambda screen: (
                screen.height > screen.width and screen.find(first_task) is not None
            ),
        )
        ui.tap(completed, first_task)
        reopened = ui.visible("Open 9", "Done 3")
        ui.tap(reopened, "Open 9")

        entry = ui.visible("New task", "Add", first_task)
        ui.tap(entry, "New task")
        ui.visible("123")
        ui.type_text("Android inspection")
        typed = ui.visible("Android inspection", "Add")
        ui.tap(typed, "Add")
        added = ui.visible("Open 10", "Done 3", "Android inspection")
        ui.tap(added, "Readings")
        readings = ui.visible("Samples", "Pressure", "Offset", "Code", "Reading")
        ui.tap(readings, "Code")
        ui.type_text("479")
        ui.call("shell", "input", "keyevent", "KEYCODE_DEL")
        ui.call("shell", "input", "keyevent", "KEYCODE_ENTER")
        submitted = ui.visible("Code 47")
        ui.tap(submitted, "Notes")
        notes = ui.visible(
            "Field notes", "Routine", "Read-only", "Record findings and follow-up steps"
        )
        ui.tap(notes, "Record findings and follow-up steps")
        ui.type_text("Android share note")
        ui.call("shell", "input", "keyevent", "KEYCODE_ENTER")
        ui.type_text("Recorded")
        written = ui.visible("Android share note", "Read-only")
        ui.tap(written, "Read-only")
        locked = ui.visible("Android share note", "Recorded", "Tasks")
        ui.tap(locked, "Android share note")
        ui.type_text("Unexpected")
        ui.tap(ui.visible("Tasks"), "Tasks")
        tasks = ui.visible("Android inspection")
        ui.tap(tasks, "Notes")
        locked = ui.visible("Android share note", "Recorded", "Read-only")

        ui.tap(locked, "Recorded", hold_ms=1600)
        ui.tap(ui.visible("Copy"), "Copy")
        ui.tap(ui.visible("Tasks"), "Tasks")
        ui.tap(ui.visible("New task"), "New task", hold_ms=1600)
        ui.tap(ui.visible("Paste"), "Paste")
        ui.tap(ui.visible("Round progress"), "Round progress")
        pasted = ui.visible("Recorded", "Add")

        ui.tap(pasted, "Recorded", hold_ms=1600)
        ui.tap(ui.visible("Cut"), "Cut")
        ui.tap(ui.visible("Round progress"), "Round progress")
        ui.tap(ui.visible("New task"), "New task", hold_ms=1600)
        ui.tap(ui.visible("Paste"), "Paste")
        ui.tap(ui.visible("Round progress"), "Round progress")
        ui.tap(ui.visible("Recorded", "Add"), "Add")
        ui.visible("Open 11", "Recorded")
    finally:
        ui.call("shell", "cmd", "window", "user-rotation", *rotation)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Drive Pocket Tasks on a running Android emulator"
    )
    parser.add_argument("--serial", required=True)
    parser.add_argument("--apk", required=True, type=Path)
    parser.add_argument("--adb", required=True, type=Path)
    scenario = parser.add_mutually_exclusive_group()
    scenario.add_argument("--dialogs-only", action="store_true")
    scenario.add_argument("--storage-only", action="store_true")
    scenario.add_argument("--pickers-only", action="store_true")
    scenario.add_argument("--row-swipes-only", action="store_true")
    scenario.add_argument("--share-receiver-apk", type=Path)
    parser.add_argument("--ime-only", action="store_true")
    parser.add_argument("--contact-ime-only", action="store_true")
    parser.add_argument("--numeric-ime-only", action="store_true")
    parser.add_argument("--multiline-ime-only", action="store_true")
    parser.add_argument("--emoji-only", action="store_true")
    args = parser.parse_args()
    adb = args.adb.resolve()
    if not adb.is_file() or not args.apk.is_file():
        parser.error(f"Both adb ({adb}) and the APK ({args.apk}) must exist")
    if args.share_receiver_apk is not None and not args.share_receiver_apk.is_file():
        parser.error(
            f"The share receiver APK does not exist: {args.share_receiver_apk}"
        )
    if (
        args.ime_only
        or args.contact_ime_only
        or args.numeric_ime_only
        or args.multiline_ime_only
        or args.emoji_only
    ) and args.share_receiver_apk is None:
        parser.error("The IME test requires --share-receiver-apk")
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
            Emulator(adb, args.serial, recognize, artifacts),
            args.apk.resolve(),
            args.dialogs_only,
            args.storage_only,
            args.share_receiver_apk,
            args.ime_only,
            args.pickers_only,
            args.row_swipes_only,
            args.contact_ime_only,
            args.numeric_ime_only,
            args.multiline_ime_only,
            args.emoji_only,
        )


if __name__ == "__main__":
    main()
