"""A small local HTTP API for the detector, so a program in another language can call it. It needs only Python's
standard library, NumPy and ONNX Runtime (no PyTorch), and listens on 127.0.0.1 only.

  GET  /health                       answers {"model": ..., "threshold": ...}
  POST /detect?w=1280&h=720[&fixed=1][&thr=0.3]
       body: the frame as raw RGB bytes (w * h * 3), followed, when fixed=1, by the fixed map (w * h bytes, 0 or 1)
       answers {"detections": [[cx, cy, w, h, score], ...], "ms": inference time}
w and h must be multiples of 16. Coordinates are pixels of the frame sent. A bad request gets 400 and {"error": ...}.
Usage: python python/model/serve.py [--model python/model/exports/detector_<infer.BEST>_fp32.onnx]
       [--port <the settings' tool_ports.detector_api, 8771>]
       [--threads 4]
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
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import infer  # noqa: E402
import local_config  # noqa: E402

WIDTH_PX, HEIGHT_PX = 1280, 720     # a frame's size when the request does not give it
SIZE_MULTIPLE = 16                  # the network needs a frame's sides in multiples of this
CHANNELS = 3                        # bytes a pixel in the RGB frame
SHOWN = 4                           # --client: detections printed


class Handler(BaseHTTPRequestHandler):
    """Answers /health and /detect. main sets the detector and the model's file name before the server starts."""

    # infer.OnnxDetector: called with the frame, the fixed map and the threshold
    detector = None
    # the model file's name, for /health
    model = None

    def log_message(self, message_format, *args):
        """Logs nothing, where the default prints a line per request."""
        pass

    def reply(self, answer, code=200):
        """Sends `answer` as JSON with the HTTP status `code`."""
        body = json.dumps(answer).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        """/health: the model's file name and the default threshold; any other path: 404."""
        if urlparse(self.path).path == "/health":
            return self.reply(dict(model=self.model, threshold=infer.THRESHOLD))
        self.reply(dict(error="not found"), 404)

    def do_POST(self):
        """/detect: the frame's detections and the detector's time in ms (400 for a bad size or body); any other path:
        404."""
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
    """Sends bench.py's sample frame and fixed map to the server on `port` `requests` times, one connection each, and
    prints the last answer's first detections and the median and 90th percentile round trip."""
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
    """Runs the client with --client, else loads the model and serves until stopped."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", default=str(Path(__file__).resolve().parent / "exports" /
                                               f"detector_{infer.BEST}_fp32.onnx"))
    parser.add_argument("--port", type=int, default=local_config.tool_port("detector_api"))
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
