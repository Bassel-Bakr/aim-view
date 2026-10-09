"""Serve the repository root for the browser benchmark, with the cross-origin isolation headers that multi-threaded
WASM needs (SharedArrayBuffer). Open http://127.0.0.1:<port>/python/model/web/ (the port: the settings'
tool_ports.web_demo, 8772).
Usage: python python/model/web/serve_static.py [--port <port>]"""
import argparse
import functools
import sys
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "python"))
import local_config  # noqa: E402


class Handler(SimpleHTTPRequestHandler):
    """Serves the files under the repo's root, with the headers the benchmark page needs."""

    # browsers load a module script or a WebAssembly file only with its own type
    extensions_map = {**SimpleHTTPRequestHandler.extensions_map, ".mjs": "text/javascript", ".wasm": "application/wasm"}

    def end_headers(self):
        """Adds cross-origin isolation (so the page gets SharedArrayBuffer) and no caching to every answer."""
        self.send_header("Cross-Origin-Opener-Policy", "same-origin")
        self.send_header("Cross-Origin-Embedder-Policy", "require-corp")
        # readable from the UI's dev server (another port) too, which is cross-origin isolated as well
        self.send_header("Cross-Origin-Resource-Policy", "cross-origin")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Cache-Control", "no-cache")
        super().end_headers()

    def log_message(self, fmt, *args):
        """Logs nothing, where the default prints a line per request."""
        pass


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=local_config.tool_port("web_demo"))
    a = ap.parse_args()
    print(f"http://127.0.0.1:{a.port}/python/model/web/", flush=True)
    ThreadingHTTPServer(("127.0.0.1", a.port), functools.partial(Handler, directory=str(ROOT))).serve_forever()
