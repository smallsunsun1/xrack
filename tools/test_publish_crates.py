"""Regression checks for staging and retrying the Cargo renderer release."""

import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import femtovg_bundle
import publish_crates as release


VERSION = "0.9.0"
COMMIT = "0123456789abcdef0123456789abcdef01234567"
IDENTITY = {
    "version": VERSION,
    "source_commit": COMMIT,
    "upstream_version": "0.28.0",
    "upstream_sha256": "upstream",
    "patches": {"font.patch": "digest"},
}


class RendererReleaseTests(unittest.TestCase):
    def test_retry_checks_patch_digest_and_yank_state(self) -> None:
        name = femtovg_bundle.PACKAGE
        with (
            patch.object(release, "published_version", return_value=None) as index,
            patch.object(
                release, "published_metadata", return_value=IDENTITY
            ) as metadata,
        ):
            self.assertTrue(release.pending_staged_package(name, VERSION, IDENTITY))
            index.return_value = {"cksum": "archive", "yanked": False}
            self.assertFalse(release.pending_staged_package(name, VERSION, IDENTITY))
            metadata.assert_called_with(name, VERSION, "archive", "release.json")
            metadata.return_value = {**IDENTITY, "patches": {"font.patch": "other"}}
            with self.assertRaisesRegex(RuntimeError, "different release state"):
                release.pending_staged_package(name, VERSION, IDENTITY)
            metadata.return_value = IDENTITY
            index.return_value = {"cksum": "archive", "yanked": True}
            with self.assertRaisesRegex(RuntimeError, "different release state"):
                release.pending_staged_package(name, VERSION, IDENTITY)

    def publish_plan(
        self, *, optimized: bool, existing: set[str]
    ) -> tuple[list[list[str]], bool]:
        def output(*args: str) -> str:
            if args[0] == "cargo":
                return json.dumps(
                    {
                        "packages": [
                            {"name": name, "version": VERSION, "publish": None}
                            for name in release.CRATES
                        ]
                    }
                )
            return "" if args[1] == "status" else COMMIT

        def stage_renderer(workspace: Path, *, source_commit: str) -> Path:
            self.assertEqual(source_commit, COMMIT)
            path = workspace / "bundles" / "femtovg"
            path.mkdir(parents=True)
            (path / "release.json").write_text(json.dumps(IDENTITY), encoding="utf-8")
            return path

        def published_version(name: str, version: str) -> dict[str, object] | None:
            self.assertEqual(version, VERSION)
            return {"cksum": "archive", "yanked": False} if name in existing else None

        with tempfile.TemporaryDirectory(prefix="xrack-release-test-") as directory:
            root = Path(directory)
            if optimized:
                (root / "tools").mkdir()
                (root / "tools" / "femtovg_bundle.py").touch()
            with (
                patch.object(release, "ROOT", root),
                patch.object(release, "output", side_effect=output),
                patch.object(
                    release, "published_version", side_effect=published_version
                ),
                patch.object(release, "published_metadata", return_value=IDENTITY),
                patch.object(release.bundles, "stage_workspace"),
                patch.object(
                    femtovg_bundle, "stage_renderer", side_effect=stage_renderer
                ) as prepare,
                patch.object(femtovg_bundle, "update_lock"),
                patch.object(
                    release, "pending_bundles", return_value=["compiler-payload"]
                ),
                patch.object(release, "verify_media_sources"),
                patch.object(release, "cargo") as cargo,
                patch.dict(release.os.environ, {"CARGO_REGISTRY_TOKEN": "test-only"}),
                patch(
                    "sys.argv",
                    [
                        "publish_crates.py",
                        VERSION,
                        "--publish",
                        "--archives",
                        str(root),
                    ],
                ),
            ):
                release.main()
            return [call.args[3] for call in cargo.call_args_list], prepare.called

    def test_companion_publishes_before_renderer_and_source_crates(self) -> None:
        plan, staged = self.publish_plan(optimized=True, existing=set())
        self.assertTrue(staged)
        self.assertNotIn(femtovg_bundle.RENDERER, plan[0])
        self.assertEqual(plan[1], [femtovg_bundle.PACKAGE, femtovg_bundle.RENDERER])
        self.assertEqual(plan[2], ["compiler-payload"])
        self.assertIn("xrack", plan[3])

    def test_retry_skips_the_uploaded_companion(self) -> None:
        plan, _ = self.publish_plan(optimized=True, existing={femtovg_bundle.PACKAGE})
        self.assertEqual(plan[1], [femtovg_bundle.RENDERER])

    def test_old_tags_use_the_original_renderer(self) -> None:
        plan, staged = self.publish_plan(optimized=False, existing=set())
        self.assertFalse(staged)
        self.assertIn(femtovg_bundle.RENDERER, plan[0])
        self.assertEqual(plan[1], [])


if __name__ == "__main__":
    unittest.main()
