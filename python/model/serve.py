"""A minimal local inference API for the detector (deployment path B). Python standard library + NumPy + ONNX Runtime;
no PyTorch. Listens on 127.0.0.1 only.

  GET  /health                       -> {"model": ..., "threshold": ...}
  POST /detect?w=1280&h=720[&fixed=1][&thr=0.3]
       body: the frame as raw RGB bytes (w * h * 3), followed, when fixed=1, by the fixed map (w * h bytes, 0 or 1)
       -> {"detections": [[cx, cy, w, h, score], ...], "ms": inference time}
w and h must be multiples of 16. Coordinates are pixels of the frame sent.
Usage: python python/model/serve.py [--model python/model/exports/detector_small_v2_fp32.onnx] [--port 8771] [--threads 4]
       python python/model/serve.py --client [--n 100]   (sends a real frame N times and prints the round-trip time)
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

WIDTH_PX, HEIGHT_PX = 1280, 720     # a frame's size when the request does not give it
SIZE_MULTIPLE = 16                  # the network needs a frame's sides in multiples of this
CHANNELS = 3                        # bytes a pixel in the RGB frame
SHOWN = 4                           # --client: detections printed


class Handler(BaseHTTPRequestHandler):
    detector = None
    model = None

    def log_message(self, message_format, *args):
        pass

    def reply(self, answer, code=200):
        body = json.dumps(answer).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if urlparse(self.path).path == "/health":
            return self.reply(dict(model=self.model, threshold=infer.THRESHOLD))
        self.reply(dict(error="not found"), 404)

    def do_POST(self):
        url = urlparse(self.path)
        if url.path != "/detect":
            return self.reply(dict(error="not found"), 404)
        try:
            query = {key: values[0] for key, values in parse_qs(url.query).items()}
            width, height = int(query.get("w", WIDTH_PX)), int(query.get("h", HEIGHT_PX))
            if width % SIZE_MULTIPLE or height % SIZE_MULTIPLE:
                raise ValueError("w and h must be multiples of 16")
            has_fixed = query.get("fixed") == "1"
            body = self.rfile.read(int(self.headers["Content-Length"]))
            rgb_bytes = width * height * CHANNELS
            need = rgb_bytes + (width * height if has_fixed else 0)
            if len(body) != need:
                raise ValueError(f"expected {need} bytes, got {len(body)}")
            rgb = np.frombuffer(body, np.uint8, rgb_bytes).reshape(height, width, CHANNELS)
            fixed = (np.frombuffer(body, np.uint8, width * height, rgb_bytes).reshape(height, width) if has_fixed
                     else np.zeros((height, width), np.uint8))
            started = time.perf_counter()
            detections = self.detector(rgb, fixed, float(query.get("thr", infer.THRESHOLD)))
            self.reply(dict(detections=[[round(float(value), 2) for value in box] for box in detections],
                            ms=round(1000 * (time.perf_counter() - started), 2)))
        except (ValueError, KeyError, TypeError) as error:
            self.reply(dict(error=str(error)), 400)


def client(port, requests):
    import http.client
    import bench
    rgb, fixed = bench.sample()
    body = rgb.tobytes() + fixed.tobytes()
    round_trips, inference_ms = [], []
    for _ in range(requests):
        connection = http.client.HTTPConnection("127.0.0.1", port)
        started = time.perf_counter()
        connection.request("POST", "/detect?w=1280&h=720&fixed=1", body, {"Content-Type": "application/octet-stream"})
        answer = json.loads(connection.getresponse().read())
        round_trips.append(time.perf_counter() - started)
        inference_ms.append(answer["ms"])
    print(f"{len(answer['detections'])} detections: {answer['detections'][:SHOWN]}")
    print(f"round trip median {1000 * np.median(round_trips):.1f} ms (inference {np.median(inference_ms):.1f} ms), "
          f"p90 {1000 * np.percentile(round_trips, 90):.1f} ms over {requests} requests")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", default=str(Path(__file__).resolve().parent / "exports" /
                                               f"detector_{infer.BEST}_fp32.onnx"))
    parser.add_argument("--port", type=int, default=8771)
    parser.add_argument("--threads", type=int, default=4)
    parser.add_argument("--client", action="store_true")
    parser.add_argument("--n", type=int, default=100)
    args = parser.parse_args()
    if args.client:
        return client(args.port, args.n)
    Handler.detector = infer.OnnxDetector(args.model, args.threads)
    Handler.model = Path(args.model).name
    print(f"detector API on http://127.0.0.1:{args.port}/ ({Handler.model}, {args.threads} threads)", flush=True)
    ThreadingHTTPServer(("127.0.0.1", args.port), Handler).serve_forever()


if __name__ == "__main__":
    main()
