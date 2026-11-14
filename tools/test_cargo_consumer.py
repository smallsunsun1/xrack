#!/usr/bin/env python3
"""Run the standalone Cargo example using a staged host compiler package."""

import argparse
from contextlib import contextmanager, nullcontext
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
from threading import Thread
import tomllib

import compiler_bundles as bundles
import mlir_bundles
import media_bundles


def check_interpreter(app, compiler, env):
    (app / "src").mkdir(parents=True)
    (app / "Cargo.toml").write_text(f"""[package]
name = "xrack-mlir-consumer"
version = "0.0.0"
edition = "2024"

[dependencies]
xrack = {{ version = "={bundles.workspace_version()}", features = ["interpreter"] }}
""")
    (app / "src/main.rs").write_text("""fn main() {
    use xrack::interpreter::{HostValue, load_mlir_file};
    let program = load_mlir_file("counter.mlir").unwrap();
    let ui = program.create("Counter").unwrap();
    ui.set("count", HostValue::Int(21)).unwrap();
    assert_eq!(ui.get("doubled").unwrap(), HostValue::Int(42));
    assert_eq!(ui.try_render().unwrap().kind, "Window");
    println!("interpreted count=21, doubled=42");
}
""")
    source = app / "counter.xr"
    source.write_text("""export component Counter inherits Window {
    in-out property <int> count: 0;
    out property <int> doubled: count * 2;
    Text { text: "" + root.doubled; }
}
""")
    subprocess.run(
        (compiler, source, "--emit", "mlir", "-o", app / "counter.mlir"),
        cwd=app, env=env, check=True,
    )


@contextmanager
def media_server(directory):
    class Handler(SimpleHTTPRequestHandler):
        def log_message(self, *_args):
            pass

    handler = partial(Handler, directory=directory)
    with ThreadingHTTPServer(("127.0.0.1", 0), handler) as server:
        thread = Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            yield f"http://127.0.0.1:{server.server_port}/live.m3u8"
        finally:
            server.shutdown()
            thread.join()


def prepare_media(app):
    (app / "src").mkdir(parents=True)
    (app / "Cargo.toml").write_text(f"""[package]
name = "xrack-media-consumer"
version = "0.0.0"
edition = "2024"

[dependencies]
xrack = {{ version = "={bundles.workspace_version()}", features = ["media"] }}

[build-dependencies]
xrack-build = "={bundles.workspace_version()}"
""")
    (app / "build.rs").write_text('fn main() { xrack_build::compile("video.xr").unwrap(); }\n')
    shutil.copyfile(bundles.ROOT / "examples/video/video.xr", app / "video.xr")
    shutil.copyfile(bundles.ROOT / "crates/xrack-media/tests/sync.mp4", app / "sync.mp4")
    media = bundles.ROOT / "crates/xrack-media/tests"
    shutil.copytree(media / "hls", app / "hls")
    playlist = (app / "hls/vod.m3u8").read_text().replace(
        "#EXT-X-PLAYLIST-TYPE:VOD\n", "#EXT-X-PLAYLIST-TYPE:EVENT\n"
    ).replace("#EXT-X-ENDLIST\n", "")
    (app / "hls/live.m3u8").write_text(playlist)
    (app / "src/main.rs").write_text("""use std::{thread, time::{Duration, Instant}};
use xrack::{Component, MediaQueue};

xrack::include_modules!();

fn main() {
    xrack::install_media();
    let mut ui = VideoDemo::new();
    ui.set_video_source(std::env::current_dir().unwrap().join("sync.mp4").to_str().unwrap().into());
    ui.set_start_playing(true);
    let mut queue = MediaQueue::default();
    let wait = |queue: &mut MediaQueue, ui: &VideoDemo, done: fn(&VideoDemo) -> bool| {
        let until = Instant::now() + Duration::from_secs(12);
        loop {
            let scene = ui.render_shared();
            queue.sync(scene.find("player").unwrap().media.as_ref());
            queue.advance(Instant::now());
            assert!(ui.get_error().is_empty(), "{}", ui.get_error());
            if done(ui) { break; }
            assert!(Instant::now() < until, "media clock did not advance");
            thread::sleep(Duration::from_millis(10));
        }
    };
    wait(&mut queue, &ui, |ui| ui.get_position() > 0.1);
    ui.invoke_pause();
    ui.invoke_seek(2.1);
    wait(&mut queue, &ui, |ui| ui.get_ready() && ui.get_position() == 2.1);
    assert!(!ui.get_playing());
    let live_url = std::env::var("XRACK_TEST_HLS").unwrap();
    ui.set_video_source(live_url.into());
    wait(&mut queue, &ui, |ui| {
        ui.get_ready() && ui.get_is_live() && ui.get_position() > 0.1
    });
    ui.invoke_pause();
    let rewind = ui.get_seek_start() + 0.2;
    ui.invoke_seek(rewind);
    wait(&mut queue, &ui, |ui| ui.get_ready() && ui.get_position() < 0.5);
    ui.invoke_go_live();
    wait(&mut queue, &ui, |ui| ui.get_playing() && ui.get_at_live_edge());
    println!("bundled media played MP4 and live HLS with seek and go-live");
}
""")


def stage_media(workspace, archives, version, host):
    commit = subprocess.check_output(("git", "rev-parse", "HEAD"), cwd=bundles.ROOT, text=True).strip()
    bundles.install_bundle(workspace, archives, version, host, commit)
    archive, _ = bundles.verified_archive(archives, version, host)
    media_bundles.stage_bundle(workspace, archive, host)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archives", type=Path, nargs="?")
    parser.add_argument("host", choices=bundles.hosts())
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--interpreter", action="store_true")
    mode.add_argument("--media", action="store_true")
    parser.add_argument("--published", action="store_true", help="test the published media crates from crates.io")
    args = parser.parse_args()
    if args.published:
        if not args.media or args.archives is not None:
            parser.error("--published requires --media and no archive directory")
    elif args.archives is None:
        parser.error("an archive directory is required")
    archives = args.archives.resolve() if args.archives else None
    root = bundles.ROOT
    version = bundles.workspace_version()
    with (root / "rust-toolchain.toml").open("rb") as toolchain:
        channel = tomllib.load(toolchain)["toolchain"]["channel"]
    with tempfile.TemporaryDirectory(prefix="xrack-cargo-consumer-") as directory:
        stage = Path(directory)
        workspace = stage / "sdk"
        if not args.published:
            bundles.stage_workspace(workspace)
        app = stage / "app"
        env = {
            **os.environ,
            "RUSTUP_TOOLCHAIN": channel,
            "CARGO_TARGET_DIR": os.environ.get("CARGO_TARGET_DIR", str(stage / "build")),
        }
        for key in ("XRACKC", "XRACK_ANTLR_DIR", "XRACK_MLIR_BINDINGS", "XRACK_MEDIA_LIBRARY", "OUT_DIR"):
            env.pop(key, None)
        if args.interpreter:
            archive, _ = bundles.verified_archive(archives, version, args.host)
            if "windows" in args.host:
                commit = subprocess.check_output(("git", "rev-parse", "HEAD"), cwd=root, text=True).strip()
                bundles.install_bundle(workspace, archives, version, args.host, commit)
            mlir_bundles.stage_bundle(workspace, archive, args.host)
            compiler = bundles.extract_compiler(archive, stage)
            check_interpreter(app, compiler, env)
            expected = "interpreted count=21, doubled=42"
        elif args.media:
            prepare_media(app)
            env["XRACK_MEDIA_TEST_AUDIO"] = "1"
            expected = "bundled media played MP4 and live HLS with seek and go-live"
            if not args.published:
                stage_media(workspace, archives, version, args.host)
        else:
            commit = subprocess.check_output(("git", "rev-parse", "HEAD"), cwd=root, text=True).strip()
            bundles.install_bundle(workspace, archives, version, args.host, commit)
            shutil.copytree(root / "examples/cargo-consumer", app)
            expected = "count=1, doubled=2"
        command = ["cargo", "run", "--release"]
        if not args.published:
            # Patch the standalone registry graph to the exact staged release sources.
            patches = {path.parent.name: path.parent for path in (workspace / "crates").glob("*/Cargo.toml")
                       if path.parent.name not in ("xrack-compiler", "xrack-codegen", "xrack-lsp", "xrack-preview")}
            if args.interpreter:
                patches.pop("xrack-build")
            for host in bundles.hosts():
                if not args.interpreter or "windows" in host:
                    patches[bundles.crate_name(host)] = workspace / "bundles/compiler" / host
                patches[f"xrack-mlir-bin-{host}"] = workspace / "bundles/mlir" / host
                patches[f"xrack-media-bin-{host}"] = workspace / "bundles/media" / host
            for name, directory in patches.items():
                path = directory.as_posix()
                command.extend(("--config", f"patch.crates-io.{name}.path={json.dumps(path)}"))
            if args.interpreter or args.media:
                names = [f"xrack-media-bin-{args.host}" if args.media else f"xrack-mlir-bin-{args.host}"]
                if args.interpreter and "windows" in args.host:
                    names.append(bundles.crate_name(args.host))
                packages = ["cargo", "package", "--locked", "--no-verify", "--allow-dirty"]
                for name in names:
                    packages.extend(("-p", name))
                subprocess.run(packages, cwd=workspace, env=env, check=True)
                for name in names:
                    artifact = Path(env["CARGO_TARGET_DIR"]) / "package" / f"{name}-{version}.crate"
                    if artifact.stat().st_size > 10_000_000:
                        raise ValueError(f"{artifact.name} exceeds the crates.io package size limit")
        with media_server(app / "hls") if args.media else nullcontext() as url:
            if url:
                env["XRACK_TEST_HLS"] = url
            result = subprocess.run(command, cwd=app, env=env, check=True, text=True, stdout=subprocess.PIPE)
        print(result.stdout, end="")
        if result.stdout.strip() != expected:
            raise SystemExit("unexpected Cargo consumer output")


if __name__ == "__main__":
    main()
