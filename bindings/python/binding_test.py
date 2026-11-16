import runpy
import sys
import tempfile
import threading
import unittest
from pathlib import Path

import xrack
from xrack.stubgen import generate

HOST_VALUES_UI = """
export struct SearchRow { label: string, color: color, direction: LayoutDirection }

export component HostValues inherits Window {
    in-out property <color> ink: #123456;
    in-out property <brush> fill: #234567;
    in-out property <length> offset: 12px;
    in-out property <LayoutDirection> direction: LayoutDirection.left-to-right;
    in-out property <Date> date;
    in-out property <Time> time;
    in-out property <Font> font;
    in-out property <styled-text> label: @markdown("**XRack**");
    in-out property <image> icon: @image-url("");
    in-out property <[SearchRow]> rows: [];
    in-out property <[TouchPoint]> contacts;
    in-out property <[string]> names: ["one"];
    out property <int> count: Model.count(root.names);
    callback inspect-key(KeyEvent) -> EventResult;
    callback recolor(color) -> color;
}
"""


class BindingTest(unittest.TestCase):
    def setUp(self) -> None:
        self.program = xrack.load_file(sys.argv[1])
        self.ui = self.program.create("AsyncDemo")

    def test_load_properties_and_callback_round_trip(self) -> None:
        self.assertEqual(self.program.components(), ["AsyncDemo"])
        self.ui.set("query", "汉字")
        self.ui.on("search", lambda query: self.ui.set("results", [query]))
        self.ui.invoke("search", self.ui.get("query"))
        self.assertEqual(self.ui.get("results"), ["汉字"])
        self.assertEqual(self.program.create("AsyncDemo").get("results"), [])
        with self.assertRaises(TypeError):
            self.ui.set("selection", True)
        with self.assertRaises(TypeError):
            self.ui.invoke("search")
        with self.assertRaises(ValueError):
            self.program.create("Missing")

    def test_ui_methods_reject_worker_threads(self) -> None:
        errors: list[str] = []

        def attempt() -> None:
            try:
                self.ui.set("query", "worker")
            except RuntimeError as error:
                errors.append(str(error))

        worker = threading.Thread(target=attempt)
        worker.start()
        worker.join()
        self.assertEqual(len(errors), 1)
        self.assertEqual(self.ui.get("query"), "XRack")

    def test_int_lists_round_trip_through_values_and_callbacks(self) -> None:
        source = """
            export component App inherits Window {
                in-out property <[int]> values: [1, 2];
                out property <int> first: Model.at(root.values, 0, -1);
                pure callback echo([int]) -> [int];
                echo(keys) => { keys }
            }
        """
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "int-lists.xr"
            path.write_text(source, encoding="utf-8")
            ui = xrack.load_file(path).create("App")
        self.assertEqual(ui.get("values"), [1, 2])
        ui.set("values", [7, -3])
        self.assertEqual(ui.get("first"), 7)
        self.assertEqual(ui.invoke("echo", [8, 9]), [8, 9])
        self.assertEqual(ui.invoke("echo", []), [])
        ui.set("values", [])
        self.assertEqual(ui.get("first"), -1)
        with self.assertRaises(TypeError):
            ui.set("values", [True])

    def test_async_handler_requires_void_callback(self) -> None:
        async def label(value: str) -> str:
            return value

        with self.assertRaisesRegex(TypeError, "void"):
            self.ui.on("result-label", label)
        with self.assertRaisesRegex(TypeError, "void"):
            self.ui.on_request("result-label", label, lambda _: None)

    def test_native_value_round_trip(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "values.xr"
            path.write_text(HOST_VALUES_UI, encoding="utf-8")
            ui = xrack.load_file(path).create("HostValues")
            ink = xrack.Color.from_hex("#123456")
            self.assertEqual(ui.get("ink"), ink)
            self.assertEqual(ink.rgba, (0x12, 0x34, 0x56, 255))
            accent = xrack.Color(200, 100, 25)
            brush = xrack.Brush.linear(
                45, [xrack.GradientStop(ink, 0.0), xrack.GradientStop(accent, 1.0)]
            )
            properties: tuple[tuple[str, xrack.Value], ...] = (
                ("fill", brush),
                ("offset", xrack.Length(48)),
                ("date", xrack.Date(2024, 2, 29)),
                ("time", xrack.Time(11, 52)),
                ("font", xrack.Font("serif", xrack.Length(15), 500, True)),
                ("label", xrack.StyledText.from_markdown("**bold**")),
                ("direction", xrack.LayoutDirection.RightToLeft),
            )
            for name, value in properties:
                ui.set(name, value)
                self.assertEqual(ui.get(name), value)
            with self.assertRaises(TypeError):
                ui.set("offset", 4.0)
            ui.reset("offset")
            self.assertEqual(ui.get("offset"), xrack.Length(12))

            icon = path.with_suffix(".svg")
            icon.write_text(
                '<svg xmlns="http://www.w3.org/2000/svg" width="2" height="3"/>',
                encoding="utf-8",
            )
            ui.set("icon", xrack.Image.from_file(icon))
            image = ui.get("icon")
            assert isinstance(image, xrack.Image)
            self.assertEqual(image.size, (2, 3))

            row = xrack.Record(
                "SearchRow", {"label": "typed", "color": ink, "direction": ui.get("direction")}
            )
            ui.set("rows", xrack.RecordList("SearchRow", [row]))
            rows = ui.get("rows")
            assert isinstance(rows, xrack.RecordList)
            self.assertEqual(rows[-1]["color"], ink)
            self.assertEqual(rows[0].fields(), row.fields())
            invalid = xrack.Record(
                "SearchRow", {"label": 9, "color": ink, "direction": ui.get("direction")}
            )
            with self.assertRaisesRegex(RuntimeError, r"\.label: expected String"):
                ui.set("rows", xrack.RecordList("SearchRow", [invalid]))
            record_model = ui.record_model("rows")
            record_model.append(row)
            with self.assertRaisesRegex(RuntimeError, r"\.label: expected String"):
                record_model[0] = invalid
            self.assertEqual(record_model[0]["label"], "typed")
            record_model[0] = xrack.Record(
                "SearchRow", {"label": "updated", "color": ink, "direction": ui.get("direction")}
            )
            next_rows = ui.get("rows")
            assert isinstance(next_rows, xrack.RecordList)
            self.assertEqual(next_rows[0]["label"], "updated")
            self.assertEqual(rows[0]["label"], "typed")
            del record_model[-1]
            self.assertEqual(len(record_model), 1)

            point = xrack.TouchPoint(
                1, xrack.Length(4), xrack.Length(8), xrack.Length(1), xrack.Length(2), True
            )
            ui.set("contacts", xrack.TouchPoints([point]))
            contacts = ui.get("contacts")
            assert isinstance(contacts, xrack.TouchPoints)
            self.assertEqual(contacts.snapshot(), [point])

            def inspect_key(event: xrack.KeyEvent) -> xrack.EventResult:
                self.assertEqual(event.key, "Enter")
                self.assertTrue(event.modifiers.shift)
                return xrack.EventResult.Accept

            ui.on("inspect-key", inspect_key)
            event = xrack.KeyEvent(
                "Enter", "", xrack.KeyModifiers(False, True, False, False, False), False
            )
            self.assertEqual(ui.invoke("inspect-key", event), xrack.EventResult.Accept)
            ui.on("recolor", lambda color: color)
            self.assertEqual(ui.invoke("recolor", accent), accent)

            model = ui.string_model("names")
            model.append("two")
            model[0] = "updated"
            self.assertEqual(ui.get("names"), ["updated", "two"])
            self.assertEqual(ui.get("count"), 2)
            del model[-1]
            self.assertEqual(ui.get("count"), 1)

    def test_generate_typed_component(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            runtime, stub = generate(Path(sys.argv[1]), Path(directory) / "async_ui.pyi")
            declarations = stub.read_text(encoding="utf-8")
            self.assertIn("def query(self) -> str", declarations)
            self.assertIn("def results(self) -> list[str]", declarations)
            self.assertIn(
                "def on_search(self, handler: Callable[[str], None | Awaitable[None]])",
                declarations,
            )
            app = runpy.run_path(str(runtime))["AsyncDemo"]()
            app.query = "typed"
            app.on_search(lambda query: setattr(app, "results", [query]))
            app.search(app.query)
            self.assertEqual(app.results, ["typed"])


if __name__ == "__main__":
    unittest.main(argv=sys.argv[:1])
