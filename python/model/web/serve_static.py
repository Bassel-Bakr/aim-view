"""Serve the repository root for the browser benchmark, with the cross-origin isolation headers that multi-threaded
WASM needs (SharedArrayBuffer). Open http://127.0.0.1:8772/python/model/web/ .
Usage: python python/model/web/serve_static.py [--port 8772]"""
import argparse
import functools
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]


class Handler(SimpleHTTPRequestHandler):
    extensions_map = {**SimpleHTTPRequestHandler.extensions_map, ".mjs": "text/javascript", ".wasm": "application/wasm"}

    def end_headers(self):
        self.send_header("Cross-Origin-Opener-Policy", "same-origin")
        self.send_header("Cross-Origin-Embedder-Policy", "require-corp")
        # readable from the UI's dev server (another port) too, which is cross-origin isolated as well
        self.send_header("Cross-Origin-Resource-Policy", "cross-origin")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Cache-Control", "no-cache")
        super().end_headers()

    def log_message(self, fmt, *args):
        pass


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=8772)
    a = ap.parse_args()
    print(f"http://127.0.0.1:{a.port}/python/model/web/", flush=True)
    ThreadingHTTPServer(("127.0.0.1", a.port), functools.partial(Handler, directory=str(ROOT))).serve_forever()
