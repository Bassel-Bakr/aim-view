"""The review app's server: a thin HTTP layer over the review service (service/, in Rust), through its Python module
(aimview, built from python-bindings/: see AGENTS.md).

The service does the work: it lists the recordings in the VODs folder, pairs each with its stats file, reviews them
(ffmpeg, the core and the detector, as the desktop app does) and keeps each recording's reviews and marks in
test_out/vod_app/, where the Python server before it kept them (python/retired/server.py). This file forwards every
/api/... and /video request to it (aimview.Library.handle), serves the Angular app's server-mode build
(ui/dist/server/browser, from `bun run build`) at /, and the old page (python/app/) at /old/. It listens on 127.0.0.1
only.
Usage: python python/server.py [--vods E:/OBS/KovOBS] [--stats <KovaaK's stats folder>] [--port 8770]
Then open http://127.0.0.1:8770/

Scripts take the library from here as before: Library (list, resolve, cache_dir, stats_for, stats_of), NAME,
STATS_DEFAULT and AREA_EXAMPLES (areas.py, model/build_kills.py, tests/find_popups.py, tests/fixtures.py).
"""
import argparse
import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import quote, unquote, urlparse

import aimview

HERE = Path(__file__).resolve().parent
DATA = HERE.parent / "test_out"                          # the service's data folder, in this server's layout
CACHE = DATA / "vod_app"                                 # each recording's folder, and the library's own files
EXPORTS = HERE / "model" / "exports"                     # the models (their _u8in exports; models.json is in model/)
AREA_EXAMPLES = CACHE / "area_examples.jsonl"            # the saved areas the area finder learns from (areas.py)
UI = HERE.parent / "ui" / "dist" / "server" / "browser"  # the Angular app's server-mode build, at /
APP = HERE / "app"                                       # the old page, at /old/
VODS_DEFAULT = r"E:\OBS\KovOBS"
STATS_DEFAULT = aimview.STATS_DEFAULT                    # KovaaK's stats folder where Steam puts it
# the UI's files by extension (Windows' registry can map .js to text/plain, which a module script refuses)
TYPES = {".html": "text/html; charset=utf-8", ".js": "text/javascript; charset=utf-8",
         ".mjs": "text/javascript; charset=utf-8", ".css": "text/css; charset=utf-8", ".json": "application/json",
         ".map": "application/json", ".wasm": "application/wasm", ".svg": "image/svg+xml", ".ico": "image/x-icon",
         ".png": "image/png", ".jpg": "image/jpeg", ".webp": "image/webp", ".woff2": "font/woff2",
         ".txt": "text/plain; charset=utf-8"}


class Names:
    """KovOBS's recording names ("<scenario> - <score> - <yyyy.mm.dd-hh.mm.ss>.mp4"), read by the service:
    match(name) gives the parts by name (m["scenario"], m["score"], m["stamp"]), or None for another name."""

    @staticmethod
    def match(name):
        parts = aimview.parse_name(name)
        return None if parts is None else dict(zip(("scenario", "score", "stamp"), parts))


NAME = Names()


class Library:
    """The review service's library (aimview.Library) over this server's data, with what the scripts use from it."""

    def __init__(self, vods=VODS_DEFAULT, stats=STATS_DEFAULT, **config):
        """config: the service's other settings (scenarios, device, ffmpeg: see python-bindings/src/lib.rs)."""
        self.service = aimview.Library(data=str(DATA), layout="python", vods=str(vods), stats=str(stats),
                                       models=str(EXPORTS), **config)

    def handle(self, method, path_and_query, range=None, body=b""):
        """An API request (/api/... or /video): (status, headers, body)."""
        return self.service.handle(method, path_and_query, range, body)

    def get(self, path_and_query):
        """A GET request's answer as Python objects; an error answer raises."""
        status, _, body = self.handle("GET", path_and_query)
        out = json.loads(body)
        if status != 200:
            raise (FileNotFoundError if status == 404 else ValueError)(out.get("error"))
        return out

    def list(self):
        """The recordings, newest first: what /api/vods gives."""
        return self.service.recordings()

    def resolve(self, vid):
        """A recording's video (its id: its path in the VODs folder, or uploads/<name>); FileNotFoundError if none."""
        return self.service.resolve(vid)

    @staticmethod
    def cache_dir(vid):
        """A recording's folder: its reviews, areas and marks."""
        return CACHE / aimview.slug(vid)

    def stats_for(self, scenario, stamp):
        """KovaaK's stats file for a run of the scenario that ended at the time stamp (within 5 s), or None."""
        return self.service.stats_for(scenario, stamp)

    def stats_of(self, vid, video):
        """The stats file for a recording: the user's choice, else one uploaded beside it, else by name and time."""
        return self.service.stats_of(vid, str(video))

    def load_stats_index(self):
        """Nothing to do: the service lists the stats folder when it is first asked, and again once a minute."""

    def review_video(self, video, model=None, out=None, **options):
        """A video reviewed as the app reviews a recording, without touching the library's reviews (the evaluation
        scripts): see Library.review_video in python-bindings/src/lib.rs."""
        return self.service.review_video(str(video), model, None if out is None else str(out), **options)


class Handler(BaseHTTPRequestHandler):
    lib: Library = None

    def log_message(self, fmt, *args):            # quiet: only errors reach the console
        pass

    def do_GET(self):
        self.route()

    def do_POST(self):
        self.route()

    def do_OPTIONS(self):
        self.route()

    def route(self):
        path = unquote(urlparse(self.path).path)
        try:
            if path in ("/api", "/video") or path.startswith("/api/"):
                return self.forward()
            if self.command != "GET":
                return self.send(404, "application/json", json.dumps(dict(error="not found")).encode())
            if path == "/old":                        # the page's own files are named relative to /old/
                self.send_response(301)
                self.send_header("Location", "/old/")
                self.send_header("Content-Length", "0")
                return self.end_headers()
            if path.startswith("/old/"):
                return self.send_file(APP, path[len("/old/"):] or "index.html", page=False)
            return self.send_file(UI, path.lstrip("/"), page=True)
        except (ConnectionError, BrokenPipeError):
            pass

    def forward(self):
        """The request, answered by the service. The service's work runs without Python's lock, so requests run at
        once (a review takes seconds to minutes, in a thread of the service's own)."""
        size = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(size) if size else b""
        if len(body) < size:
            return self.send(400, "application/json", json.dumps(dict(error="the upload stopped early")).encode())
        status, headers, out = self.lib.handle(self.command, self.path, self.headers.get("Range"), body)
        self.send_response(status)
        for k, v in headers:
            if k.lower() != "content-length":
                self.send_header(k, v)
        self.send_header("Content-Length", str(len(out)))
        self.end_headers()
        self.wfile.write(out)

    def send(self, status, kind, body, extra=()):
        self.send_response(status)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        for k, v in extra:
            self.send_header(k, v)
        self.end_headers()
        self.wfile.write(body)

    def send_file(self, root, rel, page):
        """A file of root. page: the single-page app, whose own pages (any path that is not a file) get index.html,
        sent with the headers its development server sends (cross-origin isolation)."""
        p = (root / (rel or "index.html")).resolve()
        if root.resolve() not in p.parents or not p.is_file():
            if not page:
                return self.send(404, "application/json", json.dumps(dict(error="not found")).encode())
            p = root / "index.html"
            if not p.is_file():
                return self.send(404, "text/plain; charset=utf-8",
                                 f"No server-mode build in {root}: run `bun run build`.".encode())
        isolation = [("Cross-Origin-Opener-Policy", "same-origin"),
                     ("Cross-Origin-Embedder-Policy", "require-corp")] if page else []
        return self.send(200, TYPES.get(p.suffix.lower(), "application/octet-stream"), p.read_bytes(),
                         [("Cache-Control", "no-cache")] + isolation)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--vods", default=VODS_DEFAULT)
    ap.add_argument("--stats", default=STATS_DEFAULT)
    ap.add_argument("--scenarios", nargs="+", help="the folders of scenario files (default: KovaaK's own and the "
                                                    "workshop's, beside the stats folder's KovaaK)")
    ap.add_argument("--port", type=int, default=8770)
    ap.add_argument("--detector", default="auto",
                    help="auto (the model picked in the app, else infer.BEST), or a model name: reviews use it from "
                         "now on (it becomes the app's pick)")
    ap.add_argument("--device", default="auto", choices=("auto", "directml", "cuda", "cpu"),
                    help="where the detector runs: auto (the GPU when there is one, else the CPU)")
    a = ap.parse_args()
    lib = Library(a.vods, a.stats, device=a.device, **(dict(scenarios=a.scenarios) if a.scenarios else {}))
    if a.detector != "auto":
        status, _, body = lib.handle("POST", f"/api/model?name={quote(a.detector, safe='')}")
        if status != 200:
            sys.exit(f"model {a.detector}: {json.loads(body).get('error')}")
    info = lib.get("/api/info")
    Handler.lib = lib
    print(f"Aim View: http://127.0.0.1:{a.port}/  (VODs in {a.vods}; model: {info['detector']} on {info['device']}; "
          f"the old page: http://127.0.0.1:{a.port}/old/)")
    sys.stdout.flush()
    ThreadingHTTPServer(("127.0.0.1", a.port), Handler).serve_forever()


if __name__ == "__main__":
    main()
