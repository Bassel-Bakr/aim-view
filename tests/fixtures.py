"""Expected results for the Rust core's parity tests, made by the Python reference (python/review.py) on a real
recording, stage by stage: the fixed map, the detector's raw boxes per frame, which excluded areas are pop-ups and
when they show, and link()'s input and output. Written to test_out/parity/<name>/ (not in git: it is data).
The detector is the ONNX export the browser runs (detector_<model>_u8in.onnx, ONNX Runtime on the CPU).
Usage: python tests/fixtures.py <video> [--name NAME] [--model full_v3] [--areas exclude.json]
       python tests/fixtures.py --hypot   (math.hypot cases for the Rust core's hypot)"""
import argparse
import json
import sys
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "python"))
sys.path.insert(0, str(ROOT / "python" / "model"))

import infer  # noqa: E402
import review  # noqa: E402


def hypot_cases():
    """[x, y, math.hypot(x, y)] for 20,000 random pairs in the frame's range and a few edge cases (the Rust core
    computes CPython's hypot, not the C library's), and the KovOBS overlay's shares (OVERLAY_SHARES)."""
    import math
    import random
    random.seed(3)
    pts = [(random.uniform(-700, 700), random.uniform(-500, 500)) for _ in range(20000)]
    pts += [(509.0789866674102, x) for x in (0.3, -639.97, 1e-300, 3.0, 4.0)] + [(0.0, 0.0), (1e-310, 3e-310)]
    out = ROOT / "test_out" / "parity"
    out.mkdir(parents=True, exist_ok=True)
    json.dump([[a, b, math.hypot(a, b)] for a, b in pts], open(out / "hypot.json", "w"))
    json.dump([s[:4] for s in review.OVERLAY_SHARES], open(out / "overlay.json", "w"))


def main():
    if sys.argv[1:] == ["--hypot"]:
        return hypot_cases()
    ap = argparse.ArgumentParser()
    ap.add_argument("video")
    ap.add_argument("--name", default=None)
    ap.add_argument("--model", default="full_v3")
    ap.add_argument("--areas", default=None, help="a saved exclude.json; default: the KovOBS overlay")
    a = ap.parse_args()
    name = a.name or Path(a.video).stem
    out = ROOT / "test_out" / "parity" / name
    out.mkdir(parents=True, exist_ok=True)
    seen = {}

    fixed_map = review.fixed_map
    review.fixed_map = lambda frames: seen.setdefault("fixed", fixed_map(frames))
    link = review.link
    review.link = lambda dets: seen.setdefault("frames", link(seen.setdefault("dets", dets)))
    showing = review.AreaWatch.showing
    review.AreaWatch.showing = lambda self: seen.setdefault("showing", showing(self))

    det = infer.OnnxDetector(str(ROOT / "python" / "model" / "exports" / f"detector_{a.model}_u8in.onnx"))
    raw = seen["raw"] = []
    call = det.__call__
    det_call = lambda rgb, fixed: raw.append(call(rgb, fixed)) or raw[-1]  # noqa: E731
    detector = type("Detector", (), {"__call__": lambda self, rgb, fixed: det_call(rgb, fixed)})()

    scenario = Path(a.video).stem.rsplit(" - ", 2)[0].lower()
    cap = review.target_counts().get(scenario)
    areas = json.load(open(a.areas)) if a.areas else review.OVERLAY_SHARES
    tracks = review.track_model(a.video, detector, cap=cap, areas=areas)

    fixed = seen["fixed"].astype(np.uint8)
    np.save(out / "fixed.npy", fixed)
    with open(out / "keys.yuv", "wb") as f:          # the key frames fixed_map read: YUV 4:2:0 at 1280 x 720
        keys = sum(f.write(k) > 0 for k in review._frames(a.video, keyframes=True))
    json.dump(dict(video=str(a.video), model=a.model, cap=cap, areas=areas, fps=tracks["fps"],
                   key_frames=keys, fixed_share=float(fixed.mean()),
                   showing=[None if s is None else s.astype(int).tolist() for s in seen.get("showing", [])]),
              open(out / "meta.json", "w"))
    json.dump([d.tolist() for d in raw], open(out / "raw.json", "w"))
    json.dump(seen["dets"], open(out / "dets.json", "w"))
    json.dump(seen["frames"], open(out / "frames.json", "w"))
    print(f"{name}: {len(raw)} frames, cap {cap}, fixed share {fixed.mean():.4f}, written to {out}")


if __name__ == "__main__":
    main()
