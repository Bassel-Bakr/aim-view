"""Expected results for the Rust core's parity tests, made by the Python reference (python/review.py) on a real
recording, stage by stage: the fixed map, the detector's raw boxes per frame, which excluded areas are pop-ups and
when they show, and link()'s input and output. Written to test_out/parity/<name>/ (not in git: it is data).
The detector is the ONNX export the browser runs (detector_<model>_u8in.onnx, ONNX Runtime on the CPU).
Usage: python tests/fixtures.py <video> [--name NAME] [--model full_v3] [--areas exclude.json]
       python tests/fixtures.py --hypot   (math.hypot cases for the Rust core's hypot)
       python tests/fixtures.py --scenarios   (every scenario file's facts, for the core's scenario reader)
       python tests/fixtures.py --review <name>   (Python's review of a fixture's tracks, with its stats file)
       python tests/fixtures.py --from-cache <review cache folder> <name>   (the same from the review app's cached
           tracks and camera readings, for runs no fixture covers)"""
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


def scenario_cases():
    """Every scenario file in the order review.scenario_facts reads them (later files win), and Python's facts by
    lower-case name: kind, time limit, and target count (target_counts). The Rust core reads the same files."""
    import glob
    import os
    paths = glob.glob(os.path.join(review.SCENARIOS[0], "*.sce")) + \
        glob.glob(os.path.join(review.SCENARIOS[1], "*", "*.sce"))
    facts = review.scenario_facts()
    counts = review.target_counts()
    out = ROOT / "test_out" / "parity"
    out.mkdir(parents=True, exist_ok=True)
    json.dump(dict(paths=paths, facts={n: dict(kind=k, limit=lim, targets=counts.get(n)) for n, (k, lim) in facts.items()}),
              open(out / "scenarios.json", "w", encoding="utf-8"))
    print(f"{len(paths)} scenario files, {len(facts)} scenarios")


def review_case(name):
    """Python's review of a fixture's tracks (frames.json, ONNX Runtime on the CPU) with the recording's stats file:
    flicks.json, measures.json and report.json in test_out/parity/<name>/review/, as review.review writes them."""
    import server
    d = ROOT / "test_out" / "parity" / name
    meta = json.load(open(d / "meta.json"))
    out = d / "review"
    out.mkdir(exist_ok=True)
    for f in ("flicks.json", "measures.json", "report.json"):
        (out / f).unlink(missing_ok=True)
    tracks = dict(fps=meta["fps"], frames=json.load(open(d / "frames.json")), fixed=meta["fixed_share"],
                  detector="OnnxDetector")
    json.dump(tracks, open(out / "tracks.json", "w"))
    lib = server.Library(r"E:\OBS\KovOBS", server.STATS_DEFAULT)
    stats = lib.stats_of(Path(meta["video"]).name, Path(meta["video"]))
    meta["stats"] = str(stats) if stats else None
    json.dump(meta, open(d / "meta.json", "w"))
    report = review.review(meta["video"], str(stats) if stats else None, out, exclude=meta["areas"])
    if report["mode"] == "track":
        track_inputs(meta["video"], out, len(tracks["frames"]))
        print(f"{name}: tracking run, on target {report['summary']['on_target']}, stats {stats.name if stats else None}")
    else:
        print(f"{name}: {report['summary']['measured']} flicks measured, stats {stats.name if stats else None}")


def cache_case(cache, name):
    """A parity case from the review app's cache of a run (tracks.json, camera.json and report.json in `cache`):
    Python's review again of the same tracks and camera readings, with the run's stats file and no user marks."""
    import shutil
    cache = Path(cache)
    old = json.load(open(cache / "report.json"))
    out = ROOT / "test_out" / "parity" / name / "review"
    out.mkdir(parents=True, exist_ok=True)
    (out / "report.json").unlink(missing_ok=True)
    shutil.copyfile(cache / "tracks.json", out / "tracks.json")
    shutil.copyfile(cache / "camera.json", out / "camera.json")      # newer than the tracks: review() reuses it
    report = review.review(old["video"], old["stats"], out)
    track_inputs(old["video"], out, len(json.load(open(out / "tracks.json"))["frames"]))
    m = report["summary"].get("motion") or {}
    print(f"{name}: on target {report['summary']['on_target']}, reversals {m.get('reversals')}")


def track_inputs(video, out, n, pairs=40):
    """What a tracking run's review reads from the video besides the tracks: each frame's count of countdown-teal
    pixels (countdown_end's test, teal.json), and for camera_motion `pairs` sample frames with the frame before them,
    gray at 1280 x 720 as camera_motion reads them (gray.raw, frames in gray.json)."""
    import subprocess
    W, H = review.W, review.H
    p = subprocess.Popen(["ffmpeg", "-v", "error", "-i", video, "-vf",
                          f"scale={W}:{H}:flags=area,crop=300:60:490:200,format=rgb24", "-f", "rawvideo", "-"],
                         stdout=subprocess.PIPE, bufsize=0)
    teal = []
    for buf in review._read(p, 300 * 60 * 3):
        a = np.frombuffer(buf, np.uint8).reshape(60, 300, 3).astype(np.int16)
        teal.append(int(((a[..., 0] < 60) & (a[..., 1] > 200) & (np.abs(a[..., 2] - 184) < 45)).sum()))
    json.dump(teal, open(out / "teal.json", "w"))
    picks = sorted({int(i) for i in np.linspace(1, n - 1, pairs)})
    want = {i - 1 for i in picks} | set(picks)
    p = subprocess.Popen(["ffmpeg", "-v", "error", "-i", video, "-vf", f"scale={W}:{H}:flags=area,format=gray",
                          "-f", "rawvideo", "-"], stdout=subprocess.PIPE, bufsize=0)
    with open(out / "gray.raw", "wb") as f:
        for i, buf in enumerate(review._read(p, W * H)):
            if i in want:
                f.write(buf)
    json.dump(dict(frames=sorted(want), picks=picks), open(out / "gray.json", "w"))


def main():
    if sys.argv[1:2] == ["--review"]:
        return review_case(sys.argv[2])
    if sys.argv[1:2] == ["--from-cache"]:
        return cache_case(sys.argv[2], sys.argv[3])
    if sys.argv[1:] == ["--hypot"]:
        return hypot_cases()
    if sys.argv[1:] == ["--scenarios"]:
        return scenario_cases()
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
