import runpy
import struct
import sys
import tempfile
import threading
import unittest
from pathlib import Path

import xrack
from xrack.stubgen import generate

HOST_VALUES_UI = """
export struct SearchRow { label: string, color: color, direction: LayoutDirection }
struct BoxActor {
    id: string, mesh: string, x: float, y: float, z: float,
    scale-x: float, scale-y: float, scale-z: float, yaw: float, pitch: float, roll: float,
    color: color, trigger: bool,
}

export component HostValues inherits Window {
    in-out property <color> ink: #123456;
    in-out property <brush> fill: #234567;
    in-out property <length> offset: 12px;
    in-out property <point3d> world: Point3D.new(1, 2, 0);
    out property <projectedpoint3d> projection <- Scene3D.project(
        0, 0, 8, Point2D.new(200px, 200px), root.world);
    property <[BoxActor]> scene: [BoxActor {
        id: "cube", mesh: "primitive:box", x: 0, y: 0, z: 0,
        scale-x: 1, scale-y: 1, scale-z: 1, yaw: 0, pitch: 0, roll: 0,
        color: #ffffff, trigger: false,
    }];
    out property <hit3d> hit <- Scene3D.ray-hit(
        root.scene, Point3D.new(0, 0, 3), Vector3D.new(0, 0, -2), 5);
    out property <[hit3d]> hits <- Scene3D.ray-hit-all(
        root.scene, Point3D.new(0, 0, 3), Vector3D.new(0, 0, -2), 5);
    property <[BoxActor]> zones: [BoxActor {
        id: "zone", mesh: "primitive:box", x: 0, y: 0, z: 0,
        scale-x: 1, scale-y: 1, scale-z: 1, yaw: 0, pitch: 0, roll: 0,
        color: #ffffff, trigger: true,
    }];
    out property <[hit3d]> crossings <- Scene3D.capsule-trigger-crossings(
        root.zones, Point3D.new(-2, 0, 0), Point3D.new(2, 0, 0), 0.25, 0.5);
    out property <walkresult3d> movement <- Scene3D.capsule-walk-result(
        root.zones, Point3D.new(-2, 0, 0), Vector3D.new(4, 0, 0), 0.25, 0.5, 0, 0, 45);
    out property <walkresult3d> collision <- Scene3D.capsule-walk-with-contacts(
        root.scene, Point3D.new(-2, 0, 0), Vector3D.new(2, 0, 0), 0.25, 0.5, 0, 0, 45);
    out property <walkresult3d> standing <- Scene3D.capsule-walk-with-contacts(
        root.scene, Point3D.new(0, 1, 0), Vector3D.new(0, 0, 0), 0.25, 0.5, 0, 0, 45);
    in-out property <walkresult3d> saved-movement;
    in-out property <[hit3d]> saved-hits: [];
    in-out property <LayoutDirection> direction: LayoutDirection.left-to-right;
    in-out property <ButtonDisplay> presentation;
    in-out property <InputType> input-mode: InputType.email;
    in-out property <InputPurpose> input-hint: InputPurpose.username;
    in-out property <KeyboardCapitalization> keyboard-case: KeyboardCapitalization.words;
    in-out property <ReturnKey> enter-action: ReturnKey.next;
    in-out property <AccessibleLive> status-priority: AccessibleLive.polite;
    in-out property <SceneToneMap> tone;
    in-out property <Date> date;
    in-out property <Time> time;
    in-out property <Font> font;
    in-out property <styled-text> label: @markdown("**XRack**");
    in-out property <image> icon: @image-url("");
    in-out property <texturedata2d> pixel-data;
    in-out property <[SearchRow]> rows: [];
    in-out property <[TouchPoint]> contacts;
    in-out property <[string]> names: ["one"];
    out property <int> count <- Model.count(root.names);
    callback inspect-key(KeyEvent) -> EventResult;
    callback recolor(color) -> color;
    callback reflect-hits([hit3d]) -> [hit3d];
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

    def test_runtime_mesh_asset_round_trip(self) -> None:
        source = """
            export component App inherits Window {
                in property <meshasset3d> loaded;
                out property <bounds3d> bounds <- root.loaded.bounds;
                out property <meshnode3d> absent <- Scene3D.mesh-node(root.loaded, 0);
            }
        """
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            main = root / "app.xr"
            main.write_text(source, encoding="utf-8")
            mesh = root / "panel.obj"
            mesh.write_text("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n", encoding="utf-8")
            ui = xrack.load_file(main).create("App")
            self.assertEqual(ui.get("loaded"), xrack.MeshAsset3D())
            prepared = xrack.load_mesh_file(mesh, lightmap_uv=True)
            self.assertTrue(prepared.loaded)
            ui.set("loaded", prepared)
            self.assertEqual(ui.get("loaded"), prepared)
            self.assertTrue(prepared.bounds.valid)
            self.assertEqual(prepared.bounds.min, xrack.Point3D(0, 0, 0))
            self.assertEqual(prepared.bounds.max, xrack.Point3D(1, 1, 0))
            self.assertEqual(ui.get("bounds"), prepared.bounds)
            self.assertFalse(xrack.Bounds3D().valid)
            self.assertFalse(prepared.node_at(0).valid)
            self.assertEqual(ui.get("absent"), xrack.MeshNode3D())
            self.assertEqual(xrack.SceneModel3D().node("missing", "Pivot"), xrack.MeshNode3D())
            self.assertFalse(xrack.SceneModel3D().node_at("missing", 0).valid)
            self.assertEqual(prepared.nodes("mesh"), [])
            with self.assertRaises(TypeError):
                ui.set("loaded", str(mesh))
            with self.assertRaises(ValueError):
                xrack.load_mesh_file(root / "missing.obj")

    def test_scene_node_uses_the_resolved_physics_pose(self) -> None:
        source = """
            struct Shape { shape: string, size: vector3d }
            struct Body { id: string, body-kind: string, position: point3d, shapes: [Shape] }
            struct Actor {
                id: string, mesh: string, physics-body: string, parent: string, parent-node: string,
                x: float, y: float, z: float,
                scale-x: float, scale-y: float, scale-z: float,
                yaw: float, pitch: float, roll: float, color: color,
            }
            export component App inherits Window {
                property <physicsworld3d> world <- Physics3D.new(Vector3D.new(0, 0, 0), 0.04);
                property <[Actor]> actors <- [
                    Actor {
                        id: "rig", mesh: @mesh-url("rig.gltf"), physics-body: "body",
                        parent: "", parent-node: "",
                        x: 0, y: 0, z: 0, scale-x: 1, scale-y: 1, scale-z: 1,
                        yaw: 0, pitch: 0, roll: 0, color: #ffffff,
                    },
                    Actor {
                        id: "attachment", parent: "rig", parent-node: "/Anchor",
                        mesh: "primitive:box", physics-body: "",
                        x: 2, y: 0, z: 0, scale-x: 1, scale-y: 1, scale-z: 1,
                        yaw: 0, pitch: 0, roll: 0, color: #ffffff,
                    },
                ];
                out property <scenemodel3d> scene <- Scene3D.prepare(root.actors, root.world);
                out property <meshnode3d> mount <- Scene3D.scene-node(root.scene, "rig", "/Anchor");
                callback install();
                install() => {
                    Physics3D.sync-bodies(root.world, [
                        Body {
                            id: "body", body-kind: "fixed", position: Point3D.new(4, -3, 1),
                            shapes: [Shape { shape: "box", size: Vector3D.new(1, 1, 1) }],
                        },
                    ]);
                }
            }
        """
        camera = """
            {
                "asset": {"version": "2.0"},
                "scene": 0,
                "scenes": [{"nodes": [0]}],
                "nodes": [{"name": "Anchor", "camera": 0, "translation": [1, 2, 3]}],
                "cameras": [
                    {"type": "perspective", "perspective": {"yfov": 1.0, "znear": 0.1}}
                ]
            }
        """
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            main = root / "app.xr"
            main.write_text(source, encoding="utf-8")
            (root / "rig.gltf").write_text(camera, encoding="utf-8")
            ui = xrack.load_file(main).create("App")
            before: xrack.SceneModel3D = ui.get("scene")
            self.assertEqual(before.node_at("rig", 0).origin, xrack.Point3D(1, 2, 3))
            ui.invoke("install")
            after: xrack.SceneModel3D = ui.get("scene")
            node = after.node("rig", "/Anchor")
            self.assertTrue(node.valid and node.has_camera and not node.has_mesh)
            self.assertEqual(node.origin, xrack.Point3D(5, -1, 4))
            self.assertEqual(ui.get("mount"), node)
            self.assertEqual(
                after.map_position("attachment", "", xrack.Point3D(0, 0, 0)),
                xrack.Point3D(7, -1, 4),
            )
            self.assertEqual(before.node_at("rig", 0).origin, xrack.Point3D(1, 2, 3))

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
                out property <int> first <- Model.at(root.values, 0, -1);
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

    def test_prepared_scene_model_round_trips(self) -> None:
        source = """
            struct Actor {
                id: string, mesh: string, x: float, y: float, z: float,
                scale-x: float, scale-y: float, scale-z: float,
                yaw: float, pitch: float, roll: float, color: color,
            }
            export component App inherits Window {
                in-out property <scenemodel3d> scene: Scene3D.prepare([
                    Actor { id: "cube", mesh: "primitive:box", x: 2, y: 1, z: -3,
                        scale-x: 2, scale-y: 3, scale-z: 4, yaw: 0, pitch: 0, roll: 90,
                        color: #ffffff },
                ]);
                out property <instance3d> selected <- Scene3D.instance(root.scene, 0, -1, -1);
                SceneView3D { model <- root.scene; }
            }
        """
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "prepared.xr"
            path.write_text(source, encoding="utf-8")
            ui = xrack.load_file(path).create("App")
        scene = ui.get("scene")
        self.assertIsInstance(scene, xrack.SceneModel3D)
        assert isinstance(scene, xrack.SceneModel3D)
        self.assertEqual(len(scene), 1)
        origin = xrack.Point3D(0.0, 0.0, 0.0)
        self.assertEqual(scene.map_position("cube", "", origin), xrack.Point3D(2.0, 1.0, -3.0))
        placement = scene.instance(0, -1, -1)
        self.assertIsInstance(placement, xrack.Instance3D)
        self.assertEqual(placement, ui.get("selected"))
        self.assertTrue(placement.valid)
        self.assertEqual(placement.object_id, "cube")
        self.assertEqual(placement.origin, xrack.Point3D(2.0, 1.0, -3.0))
        self.assertAlmostEqual(placement.axis_x.y, 2.0, places=5)
        self.assertAlmostEqual(placement.axis_y.x, -3.0, places=5)
        self.assertEqual(placement.axis_z.z, 4.0)
        viewport = xrack.Point2D(xrack.Length(1.0), xrack.Length(1.0))
        self.assertEqual(scene.instance_orbit(0, 0, 10, viewport, 0, -1, -1), placement)
        self.assertFalse(scene.instance_camera("missing", viewport, 0, -1, -1).valid)
        self.assertFalse(scene.instance(0, -1, 0).valid)
        ui.set("scene", xrack.SceneModel3D())
        self.assertEqual(ui.get("scene"), xrack.SceneModel3D())
        self.assertEqual(ui.get("selected"), xrack.Instance3D())
        ui.set("scene", scene)
        self.assertEqual(ui.get("scene"), scene)

    def test_procedural_texture_is_a_public_host_value(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "values.xr"
            path.write_text(HOST_VALUES_UI, encoding="utf-8")
            ui = xrack.load_file(path).create("HostValues")
            texture: xrack.TextureData2D = xrack.TextureData2D.rgba8(2, 1, bytes([16, 32, 48, 255] * 2))
            self.assertTrue(texture.valid)
            ui.set("pixel-data", texture)
            self.assertEqual(ui.get("pixel-data"), texture)
            self.assertEqual((texture.width, texture.height), (2, 1))
            ui.set("pixel-data", xrack.TextureData2D.rgba8(1, 1, b""))
            self.assertEqual(ui.get("pixel-data"), xrack.TextureData2D())

    def test_file_instance_table_reads_source_rows_without_a_scene(self) -> None:
        source = """
            export component App inherits Window {
                in-out property <bool> binary: false;
                out property <instancetable3d> table <- root.binary
                    ? @instance-table-url("sample.bin") : @instance-table-url("sample.xml");
                out property <int> rows <- root.table.count;
                out property <instance3d> item <- Scene3D.table-instance(root.table, 0);
            }
        """
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "sample.xml").write_text(
                '<InstanceTable><Instance position="1 0 0" color="red"/></InstanceTable>',
                encoding="utf-8",
            )
            words = (
                -1.0,
                0.125,
                0.0,
                2.0,
                0.0,
                1.0,
                0.0,
                3.0,
                0.0,
                0.0,
                1.0,
                4.0,
                0.0,
                1.0,
                0.0,
                0.25,
                0.5,
                -0.25,
                12.0,
                -1.0,
            )
            (root / "sample.bin").write_bytes(
                struct.pack("<4sHHIII20f", b"QtIR", 1, 0, 80, 20, 1, *words)
            )
            main = root / "app.xr"
            main.write_text(source, encoding="utf-8")
            ui = xrack.load_file(main).create("App")
            table = ui.get("table")
            self.assertIsInstance(table, xrack.InstanceTable3D)
            assert isinstance(table, xrack.InstanceTable3D)
            self.assertTrue(table.loaded)
            self.assertEqual(len(table), 1)
            self.assertEqual(table.count, ui.get("rows"))
            self.assertFalse(xrack.InstanceTable3D().loaded)
            xml = table.instance(0)
            self.assertEqual(xml, ui.get("item"))
            self.assertEqual(xml.origin, xrack.Point3D(1, 0, 0))
            self.assertEqual(xml.color.rgba, (255, 0, 0, 255))
            for index in (-1, 1, 2**63 - 1):
                self.assertFalse(table.instance(index).valid)
            self.assertFalse(xrack.InstanceTable3D().instance(0).valid)

            ui.set("binary", True)
            binary = ui.get("table")
            assert isinstance(binary, xrack.InstanceTable3D)
            raw = binary.instance(0)
            self.assertEqual(raw, ui.get("item"))
            self.assertEqual(raw.origin, xrack.Point3D(2, 3, 4))
            self.assertEqual(raw.axis_x, xrack.Vector3D(-1, 0, 0))
            self.assertEqual(raw.axis_y, xrack.Vector3D(0.125, 1, 0))
            self.assertEqual(raw.axis_z, xrack.Vector3D(0, 0, 1))
            self.assertEqual(raw.color.rgba, (0, 255, 0, 64))
            self.assertEqual((raw.data_x, raw.data_y, raw.data_z, raw.data_w), (0.5, -0.25, 12, -1))
            self.assertEqual((raw.instance_index, raw.row_index, raw.mesh_node_index), (0, -1, -1))
            self.assertEqual(table.instance(0), xml)

    def test_quaternion_value_round_trips_and_rotates_vectors(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "rotation.xr"
            path.write_text(
                """
                export component App inherits Window {
                    in-out property <quaternion3d> rotation;
                    in-out property <rotationtrack3d> track;
                    out property <vector3d> direction <- Quaternion3D.rotate(root.rotation, Vector3D.new(1, 0, 0));
                    out property <quaternion3d> sampled <- Quaternion3D.sample(root.track, 0.5);
                }
                """,
                encoding="utf-8",
            )
            ui = xrack.load_file(path).create("App")
        self.assertEqual(ui.get("rotation"), xrack.Quaternion3D())
        q = xrack.Quaternion3D.from_axis_angle(xrack.Vector3D(0, 0, 1), 90)
        ui.set("rotation", q)
        self.assertEqual(ui.get("rotation"), q)
        direction = ui.get("direction")
        assert isinstance(direction, xrack.Vector3D)
        self.assertAlmostEqual(direction.x, 0.0)
        self.assertAlmostEqual(direction.y, 1.0)
        self.assertEqual(direction, q.rotate(xrack.Vector3D(1, 0, 0)))
        self.assertEqual(xrack.Quaternion3D(0, 0, -1, -1), xrack.Quaternion3D(0, 0, 1, 1))
        self.assertEqual(ui.get("track"), xrack.RotationTrack3D())
        identity = xrack.Quaternion3D()
        track = xrack.RotationTrack3D.from_keys([(0.0, identity), (1.0, q)])
        self.assertTrue(track.valid)
        self.assertEqual((track.count, track.duration), (2, 1.0))
        ui.set("track", track)
        self.assertEqual(ui.get("track"), track)
        self.assertEqual(ui.get("sampled"), track.sample(0.5))
        self.assertFalse(xrack.RotationTrack3D.from_keys([(0.0, identity), (0.0, q)]).valid)

    def test_quaternion_aiming_projects_the_supplied_up_direction(self) -> None:
        eye = xrack.Point3D(0.0, 8.0, 0.0)
        target = xrack.Point3D(0.0, 0.0, 0.0)
        up = xrack.Vector3D(0.0, 0.0, -1.0)
        orientation = xrack.Quaternion3D.look_at(eye, target, up)
        self.assertEqual(
            orientation, xrack.Quaternion3D.look_rotation(xrack.Vector3D(0, -8, 0), up)
        )
        forward = orientation.rotate(xrack.Vector3D(0, 0, -1))
        self.assertAlmostEqual(forward.x, 0.0)
        self.assertAlmostEqual(forward.y, -1.0)
        self.assertAlmostEqual(forward.z, 0.0)
        self.assertEqual(xrack.Quaternion3D.look_at(eye, eye), xrack.Quaternion3D())

    def test_seeded_instance_table_reads_without_a_scene(self) -> None:
        source = """
            struct Spread {
                count: int, enabled: bool, seed: int,
                position-min: vector3d, position-max: vector3d,
            }
            export component App inherits Window {
                in-out property <int> count: 4096;
                in-out property <bool> enabled: true;
                out property <instancetable3d> table <- Scene3D.random-instance-table(Spread {
                    count: root.count, enabled: root.enabled, seed: 27,
                    position-min: Vector3D.new(0, 0, 0), position-max: Vector3D.new(10, 5, 3),
                });
                out property <instance3d> first <- Scene3D.table-instance(root.table, 0);
            }
        """
        with tempfile.TemporaryDirectory() as directory:
            entry = Path(directory) / "seeded.xr"
            entry.write_text(source, encoding="utf-8")
            ui = xrack.load_file(entry).create("App")
        table = ui.get("table")
        assert isinstance(table, xrack.InstanceTable3D)
        self.assertEqual(table.count, 4096)
        self.assertTrue(table.instance(4095).valid)
        self.assertFalse(table.instance(4096).valid)
        first = table.instance(0)
        self.assertNotEqual(table.instance(4095).origin, first.origin)
        self.assertEqual(first, ui.get("first"))

        ui.set("count", 1)
        self.assertEqual(ui.get("first"), first)
        ui.set("count", 0)
        empty = ui.get("table")
        assert isinstance(empty, xrack.InstanceTable3D)
        self.assertTrue(empty.loaded)
        self.assertEqual(empty.count, 0)
        self.assertFalse(empty.instance(0).valid)
        ui.set("enabled", False)
        self.assertEqual(ui.get("table"), xrack.InstanceTable3D())
        self.assertEqual(table.instance(0), first)
        self.assertTrue(table.instance(4095).valid)

    def test_async_handler_requires_void_callback(self) -> None:
        async def label(value: str) -> str:
            return value

        with self.assertRaisesRegex(TypeError, "void"):
            self.ui.on("result-label", label)
        with self.assertRaisesRegex(TypeError, "void"):
            self.ui.on_request("result-label", label, lambda _: None)

    def test_physics_world_round_trip_and_reactive_position(self) -> None:
        source = """
            export component App inherits Window {
                in-out property <physicsworld3d> world;
                in-out property <int> body;
                out property <point3d> position <- Physics3D.position(root.world, root.body);
                callback advance();
                advance() => { Physics3D.step(root.world); }
            }
        """
        world = xrack.PhysicsWorld3D(xrack.Vector3D(0.0, -9.81, 0.0), 0.016)
        body = world.add_sphere(xrack.Point3D(0.0, 2.0, 0.0), 0.5, 1.0, 0.1, 0.0)
        self.assertGreater(body, 0)
        angles = xrack.Vector3D(0.0, 0.0, 0.0)
        platform_start = xrack.Point3D(3.0, 0.5, 0.0)
        platform_target = xrack.Point3D(3.0, 0.52, 0.0)
        platform = world.add_kinematic_box(
            platform_start, xrack.Vector3D(1.0, 0.25, 1.0), angles, 0.5, 0.0
        )
        self.assertGreater(platform, 0)
        trigger = world.add_box_trigger(
            xrack.Point3D(0.0, 2.0, 0.0), xrack.Vector3D(2.0, 2.0, 2.0), angles
        )
        self.assertGreater(trigger, 0)
        self.assertTrue(world.set_collision_filter(body, 31, 0))
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "physics-world.xr"
            path.write_text(source, encoding="utf-8")
            ui = xrack.load_file(path).create("App")
            ui.set("world", world)
            ui.set("body", body)
            self.assertEqual(ui.get("world"), world)
            self.assertEqual(ui.get("position"), xrack.Point3D(0.0, 2.0, 0.0))
            self.assertTrue(world.set_kinematic_target(platform, platform_target, angles))
            self.assertEqual(world.position(platform), platform_start)
            ui.invoke("advance")
            self.assertLess(world.position(body).y, 2.0)
            self.assertEqual(ui.get("position"), world.position(body))
            self.assertAlmostEqual(world.position(platform).y, platform_target.y)
            self.assertTrue(world.trigger_overlaps(trigger, body))
            self.assertEqual(world.trigger_count(trigger), 1)
            self.assertEqual(world.trigger_entered(trigger), [body])
            self.assertTrue(world.set_collision_filter(trigger, 2, 1 << 31))
            self.assertTrue(world.trigger_overlaps(trigger, body))
            ui.invoke("advance")
            self.assertEqual(world.trigger_exited(trigger), [body])
            self.assertEqual(world.trigger_count(trigger), 0)
            self.assertTrue(world.set_collision_filter(trigger, 2, 0))
            ui.invoke("advance")
            self.assertEqual(world.trigger_entered(trigger), [body])
            self.assertTrue(world.remove(body))
            ui.invoke("advance")
            self.assertEqual(world.trigger_exited(trigger), [body])
            self.assertEqual(world.trigger_count(trigger), 0)

    def test_physics_joint_motor_and_removal(self) -> None:
        world = xrack.PhysicsWorld3D(xrack.Vector3D(0.0, 0.0, 0.0), 0.016)
        anchor = xrack.Point3D(0.0, 0.0, 0.0)
        support = world.add_sphere(anchor, 0.125, 0.0, 0.0, 0.0)
        slider = world.add_sphere(anchor, 0.25, 1.0, 0.0, 0.0)
        joint = world.add_prismatic_joint(
            support, slider, anchor, xrack.Vector3D(1.0, 0.0, 0.0), -0.2, 0.7
        )
        self.assertGreater(joint, slider)
        self.assertTrue(world.set_joint_motor(joint, 2.0, 30.0))
        for _ in range(80):
            self.assertTrue(world.step())
        self.assertAlmostEqual(world.joint_position(joint), 0.7, places=2)
        self.assertTrue(world.remove_joint(joint))
        self.assertFalse(world.set_joint_limits(joint, 0.0, 1.0))

    def test_compound_trigger_reports_one_entry(self) -> None:
        world = xrack.PhysicsWorld3D(xrack.Vector3D(0.0, 0.0, 0.0), 0.016)
        origin = xrack.Point3D(0.0, 0.0, 0.0)
        parts = xrack.RecordList(
            "ColliderPart",
            [
                xrack.Record(
                    "ColliderPart",
                    {
                        "shape": "sphere",
                        "position": xrack.Point3D(x, 0.0, 0.0),
                        "size": xrack.Vector3D(1.0, 1.0, 1.0),
                    },
                )
                for x in (-0.2, 0.2)
            ],
        )
        trigger = world.add_trigger(origin, xrack.Vector3D(0.0, 0.0, 0.0), parts)
        body = world.add_sphere(origin, 0.15, 1.0, 0.0, 0.0)
        self.assertGreater(trigger, 0)
        self.assertTrue(world.step())
        self.assertEqual(world.trigger_count(trigger), 1)
        self.assertEqual(world.trigger_entered(trigger), [body])

    def test_convex_collider_from_builtin_mesh_record(self) -> None:
        zero = xrack.Vector3D(0.0, 0.0, 0.0)
        world = xrack.PhysicsWorld3D(zero, 0.016)
        parts = xrack.RecordList(
            "ColliderPart",
            [xrack.Record("ColliderPart", {"shape": "convex-mesh", "mesh": "primitive:box"})],
        )
        body = world.add_body(xrack.Point3D(0.0, 0.0, 0.0), zero, parts, 2.0, 0.0, 0.0)
        self.assertGreater(body, 0)
        self.assertTrue(world.apply_impulse(body, xrack.Vector3D(4.0, 0.0, 0.0)))
        self.assertTrue(world.step())
        self.assertAlmostEqual(world.velocity(body).x, 2.0)

    def test_physics_contact_reports_expose_typed_points(self) -> None:
        world = xrack.PhysicsWorld3D(xrack.Vector3D(0.0, -9.81, 0.0), 0.016)
        floor = world.add_box(
            xrack.Point3D(0.0, -0.1, 0.0), xrack.Vector3D(6.0, 0.2, 6.0), 0.0, 0.0, 0.0
        )
        body = world.add_sphere(xrack.Point3D(0.0, 1.0, 0.0), 0.4, 1.0, 0.0, 0.0)
        self.assertTrue(world.set_contact_reports(floor, True, False))
        self.assertTrue(world.set_contact_reports(body, False, True))
        for _ in range(30):
            self.assertTrue(world.step())
        point = world.contacts(body)[0]
        self.assertIsInstance(point, xrack.PhysicsContact3D)
        self.assertEqual(point.body, floor)
        self.assertAlmostEqual(point.position.y, 0.0, places=2)
        self.assertGreater(point.normal_impulse, 0.0)
        self.assertAlmostEqual(point.impulse.y, point.normal_impulse)

    def test_body_loads_and_reset_from_python(self) -> None:
        zero = xrack.Vector3D(0.0, 0.0, 0.0)
        origin = xrack.Point3D(0.0, 0.0, 0.0)
        world = xrack.PhysicsWorld3D(zero, 0.05)
        body = world.add_sphere(origin, 0.5, 1.0, 0.0, 0.0)
        self.assertTrue(
            world.apply_force_at(body, xrack.Vector3D(8.0, 0.0, 0.0), xrack.Point3D(0.0, 0.3, 0.0))
        )
        self.assertEqual(world.velocity(body), zero)
        self.assertTrue(world.step())
        self.assertAlmostEqual(world.velocity(body).x, 0.4)
        self.assertLess(world.angular_velocity(body).z, 0.0)
        self.assertTrue(world.reset_body(body, origin, zero))
        self.assertTrue(world.step())
        self.assertEqual(world.position(body), origin)
        self.assertEqual(world.angular_velocity(body), zero)

    def test_contact_transitions_survive_a_rebound_in_one_batch(self) -> None:
        zero = xrack.Vector3D(0.0, 0.0, 0.0)
        world = xrack.PhysicsWorld3D(zero, 0.016)
        wall = world.add_box(
            xrack.Point3D(0.0, 0.0, 0.0),
            xrack.Vector3D(0.3, 2.0, 2.0),
            0.0,
            0.0,
            1.0,
        )
        ball = world.add_sphere(xrack.Point3D(-1.0, 0.0, 0.0), 0.25, 1.0, 0.0, 1.0)
        self.assertTrue(world.set_contact_events(ball, True))
        self.assertTrue(world.set_linear_velocity(ball, xrack.Vector3D(6.0, 0.0, 0.0)))
        self.assertEqual(world.advance(0.64, 40), 40)
        self.assertEqual(world.contact_entered(ball), [wall])
        self.assertEqual(world.contact_exited(ball), [wall])
        self.assertEqual(world.contact_bodies(ball), [])
        self.assertEqual(world.contacts(ball), [])
        self.assertTrue(world.set_contact_events(ball, False))
        self.assertEqual(world.contact_entered(ball), [])

    def test_physics_character_moves_and_jumps_from_python(self) -> None:
        world = xrack.PhysicsWorld3D(xrack.Vector3D(0.0, -9.81, 0.0), 0.016)
        ground = world.add_box(
            xrack.Point3D(0.0, -0.1, 0.0), xrack.Vector3D(5.0, 0.2, 5.0), 0.0, 0.0, 0.0
        )
        body = world.add_character(xrack.Point3D(0.0, 0.706, 0.0), 0.28, 0.84)
        self.assertGreater(body, 0)
        self.assertTrue(world.step())
        self.assertEqual(world.character_support(body), ground)
        self.assertTrue(world.character_grounded(body))
        self.assertTrue(world.set_character_movement(body, xrack.Vector3D(1.0, 0.0, 0.0)))
        self.assertTrue(world.jump_character(body, 3.0))
        self.assertTrue(world.step())
        self.assertFalse(world.character_grounded(body))
        self.assertGreater(world.position(body).x, 0.01)
        self.assertGreater(world.position(body).y, 0.73)

    def test_character_hit_reports_expose_world_coordinates(self) -> None:
        zero = xrack.Vector3D(0.0, 0.0, 0.0)
        world = xrack.PhysicsWorld3D(zero, 0.05)
        wall = world.add_box(
            xrack.Point3D(0.25, 0.0, 0.0), xrack.Vector3D(0.2, 3.0, 3.0), 0.0, 0.0, 0.0
        )
        body = world.add_character(xrack.Point3D(-0.3, 0.0, 0.0), 0.25, 0.6)
        self.assertTrue(world.set_character_movement(body, xrack.Vector3D(5.0, 0.0, 0.0)))
        self.assertTrue(world.step())
        self.assertEqual(world.character_hits(body), [])
        self.assertTrue(world.set_character_hit_reports(body, True))
        self.assertTrue(world.step())
        hit = next(hit for hit in world.character_hits(body) if hit.body == wall)
        self.assertIsInstance(hit, xrack.PhysicsContact3D)
        self.assertAlmostEqual(hit.position.x, 0.15, places=3)
        self.assertLess(hit.normal.x, -0.99)
        self.assertEqual(hit.normal_impulse, 0.0)
        self.assertTrue(world.set_character_hit_reports(body, False))
        self.assertEqual(world.character_hits(body), [])

    def test_physics_ray_query_returns_typed_sorted_hits(self) -> None:
        zero = xrack.Vector3D(0.0, 0.0, 0.0)
        world = xrack.PhysicsWorld3D(zero, 0.1)
        origin = xrack.Point3D(0.0, 0.0, 0.0)
        direction = xrack.Vector3D(2.0, 0.0, 0.0)
        size = xrack.Vector3D(1.0, 1.0, 1.0)
        sensor = world.add_box_trigger(xrack.Point3D(2.0, 0.0, 0.0), size, zero)
        body = world.add_box(xrack.Point3D(4.0, 0.0, 0.0), size, 1.0, 0.0, 0.0)
        nearest = world.ray_cast(origin, direction, 8.0)
        self.assertIsInstance(nearest, xrack.PhysicsHit3D)
        self.assertEqual(nearest.body, body)
        self.assertAlmostEqual(nearest.distance, 3.5)
        self.assertEqual(nearest.position, xrack.Point3D(3.5, 0.0, 0.0))
        self.assertEqual(nearest.normal, xrack.Vector3D(-1.0, 0.0, 0.0))
        hits = world.ray_cast_all(origin, direction, 8.0, include_sensors=True)
        self.assertEqual([hit.body for hit in hits], [sensor, body])
        self.assertTrue(hits[0].sensor)
        self.assertEqual(nearest, hits[1])
        self.assertEqual(world.ray_cast(origin, direction, 8.0, ignore_body=body).body, 0)

    def test_physics_shape_queries_share_collision_records(self) -> None:
        origin = xrack.Point3D(0.0, 0.0, 0.0)
        zero = xrack.Vector3D(0.0, 0.0, 0.0)
        direction = xrack.Vector3D(2.0, 0.0, 0.0)
        world = xrack.PhysicsWorld3D(zero, 0.1)
        center = xrack.Point3D(2.0, 0.0, 0.0)
        shapes = xrack.RecordList("Collider", [xrack.Record("Collider", {"shape": "sphere"})])
        body = world.add_sphere(center, 0.5, 0.0, 0.0, 0.0)
        sensor = world.add_trigger(center, zero, shapes)
        nearest = world.shape_cast(origin, zero, shapes, direction, 5.0)
        self.assertIsInstance(nearest, xrack.PhysicsHit3D)
        self.assertEqual(nearest.body, body)
        self.assertAlmostEqual(nearest.distance, 1.0, places=4)
        self.assertEqual(nearest.position, xrack.Point3D(1.5, 0.0, 0.0))
        all_hits = world.shape_cast_all(origin, zero, shapes, direction, 5.0, include_sensors=True)
        self.assertEqual([hit.body for hit in all_hits], [body, sensor])
        self.assertEqual(world.shape_overlap(center, zero, shapes), [body])
        self.assertEqual(
            world.shape_overlap(center, zero, shapes, ignore_body=body, include_sensors=True),
            [sensor],
        )

    def test_physics_body_declarations_reconcile_by_id(self) -> None:
        zero = xrack.Vector3D(0.0, 0.0, 0.0)
        world = xrack.PhysicsWorld3D(zero, 0.1)
        shapes = xrack.RecordList("Collider", [xrack.Record("Collider", {"shape": "sphere"})])
        ball = xrack.Record(
            "Body",
            {
                "id": "ball",
                "body-kind": "dynamic",
                "position": xrack.Point3D(0.0, 0.0, 0.0),
                "shapes": shapes,
            },
        )
        self.assertTrue(world.sync_bodies(xrack.RecordList("Body", [ball])))
        body = world.body_by_id("ball")
        self.assertGreater(body, 0)
        self.assertTrue(world.set_linear_velocity(body, xrack.Vector3D(3.0, 0.0, 0.0)))
        self.assertTrue(world.step())
        position = world.position(body)
        self.assertGreater(position.x, 0.0)
        self.assertTrue(world.sync_bodies(xrack.RecordList("Body", [ball])))
        self.assertFalse(world.sync_bodies(xrack.RecordList("Body", [ball, ball])))
        self.assertEqual(world.body_by_id("ball"), body)
        self.assertEqual(world.position(body), position)
        self.assertTrue(world.sync_bodies(xrack.RecordList("Body", [])))
        self.assertEqual(world.body_by_id("ball"), 0)

    def test_physics_scene_reconciles_named_joint_replacement(self) -> None:
        zero = xrack.Vector3D(0.0, 0.0, 0.0)
        origin = xrack.Point3D(0.0, 0.0, 0.0)
        resting = xrack.Point3D(1.0, 0.0, 0.0)
        world = xrack.PhysicsWorld3D(zero, 0.01)
        shapes = xrack.RecordList("Shape", [xrack.Record("Shape", {"shape": "sphere"})])

        def mount(point: xrack.Point3D) -> xrack.Record:
            return xrack.Record(
                "Body", {"id": "mount", "body-kind": "fixed", "position": point, "shapes": shapes}
            )

        door = xrack.Record(
            "Body", {"id": "door", "body-kind": "dynamic", "position": resting, "shapes": shapes}
        )
        hinge = xrack.Record(
            "Joint",
            {
                "id": "hinge",
                "joint-kind": "revolute",
                "first": "mount",
                "second": "door",
                "first-anchor": origin,
                "second-anchor": xrack.Point3D(-1.0, 0.0, 0.0),
            },
        )
        joints = xrack.RecordList("Joint", [hinge])
        self.assertTrue(world.sync_scene(xrack.RecordList("Body", [mount(origin), door]), joints))
        handle = world.joint_by_id("hinge")
        door_id = world.body_by_id("door")
        self.assertGreater(handle, 0)
        self.assertTrue(world.sync_scene(xrack.RecordList("Body", [door, mount(origin)]), joints))
        self.assertEqual(world.joint_by_id("hinge"), handle)
        self.assertFalse(world.sync_scene(xrack.RecordList("Body", [door]), joints))
        self.assertEqual(world.joint_by_id("hinge"), handle)
        moved = xrack.Point3D(-0.25, 0.0, 0.0)
        self.assertTrue(world.sync_scene(xrack.RecordList("Body", [mount(moved), door]), joints))
        self.assertNotEqual(world.joint_by_id("hinge"), handle)
        self.assertEqual(world.body_by_id("door"), door_id)
        self.assertFalse(world.remove_joint(handle))
        self.assertTrue(world.remove(door_id))
        self.assertEqual(world.joint_by_id("hinge"), 0)

    def test_physics_material_edits_are_validated_and_observable(self) -> None:
        zero = xrack.Vector3D(0.0, 0.0, 0.0)
        center = xrack.Point3D(0.0, 0.0, 0.0)
        world = xrack.PhysicsWorld3D(zero, 0.01)
        body = world.add_sphere(center, 0.5, 1.0, 0.25, 0.0)
        self.assertEqual(world.static_friction(body), 0.25)
        self.assertEqual(world.dynamic_friction(body), 0.25)
        self.assertTrue(world.set_material(body, 0.75, 0.25, 0.5))
        self.assertFalse(world.set_material(body, 0.0, 0.0, 1.1))
        self.assertEqual((world.static_friction(body), world.dynamic_friction(body)), (0.75, 0.25))
        self.assertEqual(world.restitution(body), 0.5)
        self.assertTrue(world.remove(body))
        self.assertFalse(world.set_material(body, 0.75, 0.25, 0.5))
        self.assertEqual(world.static_friction(body), 0.0)

    def test_physics_mass_modes_update_before_impulses(self) -> None:
        zero = xrack.Vector3D(0.0, 0.0, 0.0)
        origin = xrack.Point3D(0.0, 0.0, 0.0)
        world = xrack.PhysicsWorld3D(zero, 0.01)
        body = world.add_box(origin, xrack.Vector3D(2.0, 1.0, 1.0), 1.0, 0.0, 0.0)
        self.assertTrue(world.use_default_density(body))
        self.assertEqual(world.mass(body), 2.0)
        self.assertTrue(world.set_default_density(2.0))
        self.assertEqual(world.mass(body), 4.0)
        center = xrack.Point3D(0.0, 0.5, 0.0)
        x = xrack.Vector3D(2.0, 1.0, 0.0)
        y = xrack.Vector3D(1.0, 2.0, 0.0)
        z = xrack.Vector3D(0.0, 0.0, 4.0)
        self.assertTrue(world.set_inertia_matrix(body, 2.0, center, x, y, z))
        self.assertEqual(world.center_of_mass(body), center)
        self.assertAlmostEqual(world.inertia_column(body, 0).y, 1.0, places=5)
        self.assertFalse(world.set_inertia_matrix(body, 3.0, center, y, y, z))
        self.assertTrue(world.set_default_density(3.0))
        self.assertEqual(world.mass(body), 2.0)
        self.assertTrue(world.apply_torque_impulse(body, xrack.Vector3D(1.0, 0.0, 0.0)))
        self.assertAlmostEqual(world.angular_velocity(body).y, -1.0 / 3.0, places=5)

    def test_kinematic_handoff_keeps_handle_and_committed_motion(self) -> None:
        origin = xrack.Point3D(0.0, 0.0, 0.0)
        zero = xrack.Vector3D(0.0, 0.0, 0.0)
        world = xrack.PhysicsWorld3D(zero, 0.1)
        body = world.add_box(origin, xrack.Vector3D(1.0, 1.0, 1.0), 1.0, 0.0, 0.0)
        self.assertTrue(world.set_kinematic(body, True))
        self.assertTrue(world.is_kinematic(body))
        self.assertTrue(world.set_mass(body, 2.0))
        self.assertTrue(
            world.set_kinematic_target_with_pivot(
                body, origin, xrack.Vector3D(90.0, 0.0, 0.0), xrack.Vector3D(1.0, 0.0, 0.0)
            )
        )
        self.assertEqual(world.position(body), origin)
        self.assertTrue(world.step())
        linear = world.velocity(body)
        angular = world.angular_velocity(body)
        self.assertAlmostEqual(linear.x, 10.0, places=5)
        self.assertAlmostEqual(angular.y, 15.707963, places=4)
        self.assertTrue(world.set_kinematic(body, False))
        self.assertEqual(world.mass(body), 2.0)
        self.assertEqual(world.velocity(body), linear)
        self.assertEqual(world.angular_velocity(body), angular)
        self.assertTrue(world.set_kinematic(body, True))
        self.assertTrue(world.step())
        self.assertTrue(world.set_kinematic(body, False))
        self.assertEqual(world.velocity(body), zero)
        self.assertEqual(world.angular_velocity(body), zero)

    def test_batched_physics_clock_exposes_pose_and_dropped_time(self) -> None:
        zero = xrack.Vector3D(0.0, 0.0, 0.0)
        origin = xrack.Point3D(0.0, 0.0, 0.0)
        world = xrack.PhysicsWorld3D(zero, 0.1)
        body = world.add_sphere(origin, 0.2, 1.0, 0.0, 0.0)
        self.assertTrue(world.set_linear_velocity(body, xrack.Vector3D(1.0, 0.0, 0.0)))
        self.assertTrue(world.set_pose_interpolation(body, True))
        self.assertEqual(world.advance(0.35, 2), 2)
        self.assertAlmostEqual(world.simulation_time(), 0.2)
        self.assertAlmostEqual(world.dropped_time(), 0.1)
        self.assertAlmostEqual(world.interpolation_alpha(), 0.5)
        self.assertAlmostEqual(world.position(body).x, 0.2)
        self.assertAlmostEqual(world.interpolated_position(body).x, 0.15)
        self.assertTrue(world.set_running(False))
        self.assertEqual(world.advance(1.0, 2), 0)
        self.assertTrue(world.step())
        self.assertEqual(world.interpolated_position(body), world.position(body))

    def test_physics_world_settings_and_character_gravity_read_back(self) -> None:
        origin = xrack.Point3D(0.0, 0.0, 0.0)
        zero = xrack.Vector3D(0.0, 0.0, 0.0)
        gravity = xrack.Vector3D(0.0, -9.0, 0.0)
        world = xrack.PhysicsWorld3D(zero, 0.01)
        body = world.add_sphere(origin, 0.2, 1.0, 0.0, 0.0)
        character = world.add_character(xrack.Point3D(2.0, 0.0, 0.0), 0.25, 0.5)
        self.assertTrue(world.set_ccd_substeps(4))
        self.assertEqual(world.ccd_substeps(), 4)
        self.assertTrue(world.set_body_ccd(body, False))
        self.assertFalse(world.body_ccd(body))
        self.assertTrue(world.set_soft_ccd(body, 0.5))
        self.assertEqual(world.soft_ccd(body), 0.5)
        self.assertTrue(world.set_length_unit(100.0))
        self.assertEqual(world.length_unit(), 100.0)
        self.assertTrue(world.set_character_gravity(character, zero))
        self.assertTrue(world.set_gravity(gravity))
        self.assertEqual(world.gravity(), gravity)
        self.assertEqual(world.character_gravity(character), zero)
        self.assertTrue(world.use_character_world_gravity(character))
        self.assertEqual(world.character_gravity(character), gravity)

    def test_kinematic_contact_mode_reports_points_with_zero_impulse(self) -> None:
        zero = xrack.Vector3D(0.0, 0.0, 0.0)
        origin = xrack.Point3D(0.0, 0.0, 0.0)
        size = xrack.Vector3D(1.0, 1.0, 1.0)
        world = xrack.PhysicsWorld3D(zero, 0.1)
        wall = world.add_box(origin, size, 0.0, 0.0, 0.0)
        mover = world.add_kinematic_box(xrack.Point3D(-2.0, 0.0, 0.0), size, zero, 0.0, 0.0)
        self.assertTrue(world.set_contact_reports(wall, True, False))
        self.assertTrue(world.set_contact_reports(mover, False, True))
        self.assertTrue(world.set_kinematic_contact_reports(True, False))
        self.assertTrue(world.report_static_kinematic_collisions())
        self.assertFalse(world.report_kinematic_kinematic_collisions())
        target = xrack.Point3D(-0.625, 0.0, 0.0)
        self.assertTrue(world.set_kinematic_target(mover, target, zero))
        self.assertTrue(world.step())
        self.assertEqual(world.position(mover), target)
        reports = world.contacts(mover)
        self.assertGreater(len(reports), 0)
        self.assertTrue(all(point.body == wall for point in reports))
        self.assertTrue(all(point.normal_impulse == 0.0 for point in reports))
        self.assertEqual(world.contacts(wall), [])

    def test_empty_mesh_has_empty_catalogs(self) -> None:
        asset = xrack.MeshAsset3D()
        self.assertEqual(asset.material_slots(), [])
        self.assertEqual(asset.material_variants(), [])
        self.assertEqual(asset.animation_clips(), [])
        self.assertEqual(asset.animation_duration("Slide"), 0.0)

    def test_viewport_statistics_has_typed_empty_value_before_paint(self) -> None:
        source = """
            export component App inherits Window {
                view := SceneView3D { render-stats-revision: 1; }
                out property <renderstats3d> sample: view.render-stats();
            }
        """
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "render_stats.xr"
            path.write_text(source, encoding="utf-8")
            sample = xrack.load_file(path).create("App").get("sample")
            self.assertEqual(sample, xrack.RenderStats3D())
            assert isinstance(sample, xrack.RenderStats3D)
            self.assertFalse(sample.valid)
            self.assertFalse(sample.gpu_valid)
            self.assertEqual(sample.mesh_draws, 0)

    def test_native_value_round_trip(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "values.xr"
            path.write_text(HOST_VALUES_UI, encoding="utf-8")
            ui = xrack.load_file(path).create("HostValues")
            ink = xrack.Color.from_hex("#123456")
            self.assertEqual(ui.get("ink"), ink)
            self.assertEqual(ui.get("presentation"), xrack.ButtonDisplay.TextBesideIcon)
            self.assertEqual(ui.get("input-mode"), xrack.InputType.Email)
            self.assertEqual(ui.get("input-hint"), xrack.InputPurpose.Username)
            self.assertEqual(ui.get("keyboard-case"), xrack.KeyboardCapitalization.Words)
            self.assertEqual(ui.get("enter-action"), xrack.ReturnKey.Next)
            self.assertEqual(ui.get("status-priority"), xrack.AccessibleLive.Polite)
            self.assertEqual(ui.get("tone"), xrack.SceneToneMap.Reinhard)
            ui.set("tone", xrack.SceneToneMap.None_)
            self.assertEqual(ui.get("tone"), xrack.SceneToneMap.None_)
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
                ("presentation", xrack.ButtonDisplay.TextUnderIcon),
                ("input-mode", xrack.InputType.Url),
                ("input-mode", xrack.InputType.Tel),
                ("input-hint", xrack.InputPurpose.OneTimeCode),
                ("keyboard-case", xrack.KeyboardCapitalization.None_),
                ("enter-action", xrack.ReturnKey.Done),
                ("status-priority", xrack.AccessibleLive.Assertive),
            )
            for name, value in properties:
                ui.set(name, value)
                self.assertEqual(ui.get(name), value)
            world = xrack.Point3D(0.0, 0.0, 0.0)
            ui.set("world", world)
            self.assertEqual(ui.get("world"), world)
            projected = ui.get("projection")
            self.assertEqual(
                projected,
                xrack.ProjectedPoint3D(
                    xrack.Point2D(xrack.Length(100), xrack.Length(100)), 8.0, True, True
                ),
            )
            hit = ui.get("hit")
            assert isinstance(hit, xrack.Hit3D)
            self.assertTrue(hit.valid)
            self.assertFalse(hit.entering)
            self.assertEqual(hit.object_id, "cube")
            self.assertEqual(hit.distance, 2.5)
            self.assertEqual(hit.position, xrack.Point3D(0.0, 0.0, 0.5))
            self.assertEqual(hit.normal, xrack.Vector3D(0.0, 0.0, 1.0))
            self.assertEqual(hit.row_index, 0)
            self.assertEqual(hit.mesh_node_index, -1)
            self.assertEqual(hit.instance_index, -1)
            self.assertEqual(ui.get("hits"), [hit])
            crossings: xrack.Value = ui.get("crossings")
            assert isinstance(crossings, list)
            self.assertEqual(len(crossings), 2)
            entry, departure = crossings
            assert isinstance(entry, xrack.Hit3D)
            assert isinstance(departure, xrack.Hit3D)
            self.assertTrue(entry.entering)
            self.assertFalse(departure.entering)
            self.assertEqual(entry.distance, 1.25)
            self.assertEqual(departure.distance, 2.75)
            movement: xrack.Value = ui.get("movement")
            assert isinstance(movement, xrack.WalkResult3D)
            self.assertEqual(movement.position, xrack.Point3D(2.0, 0.0, 0.0))
            self.assertEqual(movement.crossings, crossings)
            self.assertFalse(movement.grounded)
            self.assertEqual(movement.contacts, [])
            collision: xrack.Value = ui.get("collision")
            assert isinstance(collision, xrack.WalkResult3D)
            self.assertTrue(collision.hit_wall)
            self.assertEqual(len(collision.contacts), 1)
            self.assertEqual(collision.contacts[0].object_id, "cube")
            self.assertAlmostEqual(collision.contacts[0].position.x, -0.5)
            self.assertAlmostEqual(collision.contacts[0].normal.x, -1.0)
            standing: xrack.Value = ui.get("standing")
            assert isinstance(standing, xrack.WalkResult3D)
            self.assertTrue(standing.grounded)
            self.assertFalse(standing.hit_wall)
            self.assertFalse(standing.hit_ceiling)
            self.assertEqual(standing.contacts, [])
            self.assertTrue(standing.support.valid)
            self.assertEqual(standing.support.object_id, "cube")
            self.assertEqual(standing.support.position, xrack.Point3D(0.0, 0.5, 0.0))
            self.assertEqual(standing.support.normal, xrack.Vector3D(0.0, 1.0, 0.0))
            default_movement: xrack.Value = ui.get("saved-movement")
            assert isinstance(default_movement, xrack.WalkResult3D)
            self.assertEqual(default_movement.position, xrack.Point3D(0.0, 0.0, 0.0))
            self.assertEqual(default_movement.crossings, [])
            self.assertFalse(default_movement.grounded)
            replacement: xrack.WalkResult3D = xrack.WalkResult3D(
                world,
                movement.crossings,
                grounded=True,
                hit_wall=True,
                hit_ceiling=True,
                contacts=collision.contacts,
                support=standing.support,
            )
            ui.set("saved-movement", replacement)
            restored: xrack.Value = ui.get("saved-movement")
            self.assertEqual(restored, replacement)
            assert isinstance(restored, xrack.WalkResult3D)
            self.assertTrue(restored.grounded and restored.hit_wall and restored.hit_ceiling)
            self.assertEqual(restored.contacts, collision.contacts)
            self.assertEqual(restored.support, standing.support)
            ui.on("reflect-hits", lambda hits: hits)
            self.assertEqual(ui.invoke("reflect-hits", [hit]), [hit])
            self.assertEqual(ui.invoke("reflect-hits", []), [])
            ui.set("saved-hits", [hit])
            self.assertEqual(ui.get("saved-hits"), [hit])
            ui.set("saved-hits", [])
            self.assertEqual(ui.get("saved-hits"), [])
            miss: xrack.Hit3D = xrack.Hit3D.miss()
            self.assertFalse(miss.valid)
            self.assertEqual(miss.item_source, "")
            self.assertEqual(miss.item_position.x, xrack.Length(0))
            self.assertEqual(miss.item_position.y, xrack.Length(0))
            with self.assertRaises(TypeError):
                ui.set("offset", 4.0)
            with self.assertRaisesRegex(RuntimeError, "initialized only once"):
                ui.reset("offset")
            self.assertEqual(ui.get("offset"), xrack.Length(48))

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
