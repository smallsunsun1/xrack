#!/usr/bin/env python3
"""Serve a rolling two-rendition HLS stream at http://127.0.0.1:8785."""

import shutil
import signal
import subprocess
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from tempfile import TemporaryDirectory
from threading import Event
from time import sleep
from urllib.parse import urlsplit


HOST = "127.0.0.1"
PORT = 8785
WINDOW = 12
HIGH_DELAY = 0.8
MASTER = """#EXTM3U
#EXT-X-VERSION:3
#EXT-X-INDEPENDENT-SEGMENTS
#EXT-X-STREAM-INF:BANDWIDTH=120000,RESOLUTION=160x90
low/index.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=480000,RESOLUTION=480x270
high/index.m3u8
"""


class Handler(SimpleHTTPRequestHandler):
    extensions_map = {
        **SimpleHTTPRequestHandler.extensions_map,
        ".m3u8": "application/vnd.apple.mpegurl",
        ".ts": "video/mp2t",
    }

    def do_GET(self):
        path = urlsplit(self.path).path
        if path == "/":
            slow = self.server.slow.is_set()
            action = "normal" if slow else "slow"
            label = "Restore normal delivery" if slow else "Slow the high rendition"
            page = f"""<!doctype html>
<meta charset="utf-8"><title>XRack HLS demo</title>
<h1>XRack HLS demo</h1>
<p>Stream URL <code>http://{HOST}:{PORT}/live.m3u8</code></p>
<p>High rendition: {"delayed" if slow else "normal"}.</p>
<form method="post" action="/{action}"><button>{label}</button></form>
""".encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(page)))
            self.end_headers()
            self.wfile.write(page)
            return
        if path.startswith("/high/") and path.endswith(".ts") and self.server.slow.is_set():
            sleep(HIGH_DELAY)
        super().do_GET()

    def do_POST(self):
        if self.path == "/slow":
            self.server.slow.set()
        elif self.path == "/normal":
            self.server.slow.clear()
        else:
            self.send_error(404)
            return
        self.send_response(303)
        self.send_header("Location", "/")
        self.end_headers()

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def log_message(self, *_args):
        pass


def encode(ffmpeg, directory):
    return subprocess.Popen([
        ffmpeg, "-nostdin", "-hide_banner", "-loglevel", "error",
        "-readrate", "1", "-readrate_initial_burst", str(WINDOW),
        "-f", "lavfi", "-i", "testsrc2=size=480x270:rate=20",
        "-readrate", "1", "-readrate_initial_burst", str(WINDOW),
        "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000",
        "-filter_complex", "[0:v]split=2[high][base];[base]scale=160:90[low]",
        "-map", "[low]", "-map", "1:a", "-map", "[high]", "-map", "1:a",
        "-c:v", "libx264", "-preset", "ultrafast", "-tune", "zerolatency",
        "-g", "20", "-keyint_min", "20", "-sc_threshold", "0", "-pix_fmt", "yuv420p",
        "-threads:v", "2",
        "-b:v:0", "80k", "-maxrate:v:0", "80k", "-bufsize:v:0", "160k",
        "-b:v:1", "400k", "-maxrate:v:1", "400k", "-bufsize:v:1", "800k",
        "-c:a", "aac", "-b:a", "32k",
        "-f", "hls", "-hls_time", "1", "-hls_list_size", str(WINDOW),
        "-hls_delete_threshold", str(WINDOW),
        "-hls_flags", "delete_segments+omit_endlist+independent_segments+temp_file",
        "-var_stream_map", "v:0,a:0,name:low v:1,a:1,name:high",
        "-hls_segment_filename", "%v/seg-%06d.ts", "%v/index.m3u8",
    ], cwd=directory)


def main():
    ffmpeg = shutil.which("ffmpeg")
    if ffmpeg is None:
        raise SystemExit("FFmpeg with the libx264 encoder is required to generate the local stream.")
    with TemporaryDirectory(prefix="xrack-hls-") as temporary:
        directory = Path(temporary)
        (directory / "live.m3u8").write_text(MASTER, encoding="utf-8")
        for name in ("low", "high"):
            (directory / name).mkdir()

        handler = partial(Handler, directory=directory)
        with ThreadingHTTPServer((HOST, PORT), handler) as server:
            server.slow = Event()
            signal.signal(signal.SIGTERM, signal.default_int_handler)
            process = encode(ffmpeg, directory)
            try:
                for _ in range(100):
                    if process.poll() is not None:
                        raise SystemExit(f"FFmpeg exited with status {process.returncode}.")
                    if all((directory / name / "index.m3u8").exists() for name in ("low", "high")):
                        break
                    sleep(0.1)
                else:
                    raise SystemExit("Timed out waiting for the first live segments.")
                print(f"Live HLS  http://{HOST}:{PORT}/live.m3u8", flush=True)
                print(f"Controls  http://{HOST}:{PORT}/", flush=True)
                server.timeout = 0.5
                while process.poll() is None:
                    server.handle_request()
                raise SystemExit(f"FFmpeg exited with status {process.returncode}.")
            except KeyboardInterrupt:
                pass
            finally:
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()


if __name__ == "__main__":
    main()
