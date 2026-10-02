"""A minimal local inference API for the detector (deployment path B). Python standard library + NumPy + ONNX Runtime;
no PyTorch. Listens on 127.0.0.1 only.

  GET  /health                       -> {"model": ..., "threshold": ...}
  POST /detect?w=1280&h=720[&fixed=1][&thr=0.3]
       body: the frame as raw RGB bytes (w * h * 3), followed, when fixed=1, by the fixed map (w * h bytes, 0 or 1)
       -> {"detections": [[cx, cy, w, h, score], ...], "ms": inference time}
w and h must be multiples of 16. Coordinates are pixels of the frame sent.
Usage: python vod/model/serve.py [--model vod/model/exports/detector_small_v2_fp32.onnx] [--port 8771] [--threads 4]
       python vod/model/serve.py --client [--n 100]   (sends a real frame N times and prints the round-trip time)
"""
import argparse
import json
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
import infer  # noqa: E402


class Handler(BaseHTTPRequestHandler):
    det = None
    model = None

    def log_message(self, fmt, *args):
        pass

    def reply(self, obj, code=200):
        b = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(b)))
        self.end_headers()
        self.wfile.write(b)

    def do_GET(self):
        if urlparse(self.path).path == "/health":
            return self.reply(dict(model=self.model, threshold=infer.THRESHOLD))
        self.reply(dict(error="not found"), 404)

    def do_POST(self):
        u = urlparse(self.path)
        if u.path != "/detect":
            return self.reply(dict(error="not found"), 404)
        try:
            q = {k: v[0] for k, v in parse_qs(u.query).items()}
            w, h = int(q.get("w", 1280)), int(q.get("h", 720))
            if w % 16 or h % 16:
                raise ValueError("w and h must be multiples of 16")
            has_fixed = q.get("fixed") == "1"
            body = self.rfile.read(int(self.headers["Content-Length"]))
            need = w * h * 3 + (w * h if has_fixed else 0)
            if len(body) != need:
                raise ValueError(f"expected {need} bytes, got {len(body)}")
            rgb = np.frombuffer(body, np.uint8, w * h * 3).reshape(h, w, 3)
            fixed = (np.frombuffer(body, np.uint8, w * h, w * h * 3).reshape(h, w) if has_fixed
                     else np.zeros((h, w), np.uint8))
            t = time.perf_counter()
            d = self.det(rgb, fixed, float(q.get("thr", infer.THRESHOLD)))
            self.reply(dict(detections=[[round(float(v), 2) for v in r] for r in d],
                            ms=round(1000 * (time.perf_counter() - t), 2)))
        except (ValueError, KeyError, TypeError) as e:
            self.reply(dict(error=str(e)), 400)


def client(port, n):
    import http.client
    import bench
    rgb, fixed = bench.sample()
    body = rgb.tobytes() + fixed.tobytes()
    times, server = [], []
    for _ in range(n):
        c = http.client.HTTPConnection("127.0.0.1", port)
        t = time.perf_counter()
        c.request("POST", "/detect?w=1280&h=720&fixed=1", body, {"Content-Type": "application/octet-stream"})
        r = json.loads(c.getresponse().read())
        times.append(time.perf_counter() - t)
        server.append(r["ms"])
    print(f"{len(r['detections'])} detections: {r['detections'][:4]}")
    print(f"round trip median {1000 * np.median(times):.1f} ms (inference {np.median(server):.1f} ms), "
          f"p90 {1000 * np.percentile(times, 90):.1f} ms over {n} requests")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default=str(Path(__file__).resolve().parent / "exports" / f"detector_{infer.BEST}_fp32.onnx"))
    ap.add_argument("--port", type=int, default=8771)
    ap.add_argument("--threads", type=int, default=4)
    ap.add_argument("--client", action="store_true")
    ap.add_argument("--n", type=int, default=100)
    a = ap.parse_args()
    if a.client:
        return client(a.port, a.n)
    Handler.det = infer.OnnxDetector(a.model, a.threads)
    Handler.model = Path(a.model).name
    print(f"detector API on http://127.0.0.1:{a.port}/ ({Handler.model}, {a.threads} threads)", flush=True)
    ThreadingHTTPServer(("127.0.0.1", a.port), Handler).serve_forever()


if __name__ == "__main__":
    main()
