"""Training crops from the moments just before kills, where the model is weakest: a target held under the crosshair.

The key-frame dataset (build_data.py) almost never catches a kill. Here, for each static VOD with a stats file:
1. the video's clock is lined up with the stats file's: the first seconds are decoded, tracked with the model, and the
   first kills matched (review.match_times votes the offset, to the frame);
2. about 10 kills spread over the run are picked, and only the third of a second before each is decoded (seeking to
   the key frame before it);
3. those frames are labelled with the model's detections. The killed target keeps its label even where the model lost
   it under the crosshair: it does not move, so its place follows the camera's turn (the tracks' frame-to-frame shift)
   from where it was last seen. Those are the labels the model needs to learn.
Each frame gives one 256 x 256 crop round the crosshair (shifted a little at random), saved like build_data.py's
(rgb, fixed, tmask, boxes) plus "hidden" (1 when the killed target's label came from following the camera).
Splits follow build_data.split_of (scenario folders), so the end-to-end test VODs never reach training.
Incremental: a VOD already in the manifest (same file and size) is skipped.
Usage: python python/model/build_kills.py [--out test_out/vod_model/data_kills] [--per-folder 1] [--kills 10]
"""
import argparse
import hashlib
import json
import math
import random
import statistics as st
import subprocess
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
import build_data  # noqa: E402
import infer  # noqa: E402
import review  # noqa: E402
import server  # noqa: E402

CROP = 256
BACK = (1, 3, 6, 10, 15, 22, 30)          # frames before the kill used, at 120 fps (scaled for other rates)
LABELLER = "small_v2"                      # never marks KovaaK's crosshair; the models trained on these crops did
GPU = threading.Lock()


def decode(video, t0, t1):
    """Frames from t0 to t1 seconds, RGB 1280 x 720 (ffmpeg's own conversion, as in training and in the review)."""
    p = subprocess.run(["ffmpeg", "-v", "error", "-ss", f"{max(0.0, t0):.4f}", "-i", str(video), "-t", f"{t1 - max(0.0, t0):.4f}",
                        "-vf", f"scale={review.W}:{review.H}:flags=area,format=rgb24", "-f", "rawvideo", "-"],
                       capture_output=True)
    n = len(p.stdout) // (review.W * review.H * 3)
    return np.frombuffer(p.stdout[:n * review.W * review.H * 3], np.uint8).reshape(n, review.H, review.W, 3)


def detect_stream(det, video, t1, fixed):
    """The model's boxes for every frame from the start to t1 seconds, decoded and detected 16 frames at a time (the
    frames are not kept: a captured pipe of 10 s of 720p RGB is 3.3 GB and took 32 s)."""
    size = review.W * review.H * 3
    p = subprocess.Popen(["ffmpeg", "-v", "error", "-i", str(video), "-t", f"{t1:.4f}", "-vf",
                          f"scale={review.W}:{review.H}:flags=area,format=rgb24", "-f", "rawvideo", "-"],
                         stdout=subprocess.PIPE, bufsize=0)
    buf, out = np.empty((16, review.H, review.W, 3), np.uint8), []
    try:
        while True:
            n = 0
            while n < 16:
                mv, got = memoryview(buf[n].reshape(-1)), 0
                while got < size and (k := p.stdout.readinto(mv[got:])):
                    got += k
                if got < size:
                    break
                n += 1
            if n:
                out += detect(det, buf[:n], fixed)
            if n < 16:
                return out
    finally:
        p.stdout.close()
        p.wait()


def detect(det, frames, fixed):
    """The model's boxes per frame (cx, cy, w, h, score in pixels), the overlay masked out as in the review."""
    out = []
    for i in range(0, len(frames), 16):
        with GPU:
            ds = det.batch(frames[i:i + 16], fixed)
        for d in ds:
            keep = [b for b in d if review.MASK[min(review.H - 1, max(0, int(b[1]))), min(review.W - 1, max(0, int(b[0])))]]
            out.append(np.array(keep, np.float32).reshape(-1, 5))
    return out


def tracks_of(boxes, start):
    """Link per-frame boxes into tracks (review.link), frames numbered from `start`."""
    rows = [[(*review.to_deg(float(b[0]), float(b[1])), int(round(math.pi / 4 * b[2] * b[3]))) for b in d] for d in boxes]
    frames = review.link(rows)
    for k, f in enumerate(frames):
        f["i"] = start + k
    return frames


def to_px(xd, yd):
    x = review.CX + review.K * math.tan(math.radians(xd))
    return x, review.CY - math.tan(math.radians(yd)) * math.hypot(review.K, x - review.CX)


def one(job, det):
    folder, video, stats, split, out, seed = job
    count = COUNTS.get(folder.lower())
    row = dict(folder=folder, file=Path(video).name, size=Path(video).stat().st_size, split=split, kept=False, crops=0)
    fps, dur = review.probe(video)
    meta, srows = review.load_stats(stats)
    t0 = review.datetime.strptime(meta["Challenge Start"], "%H:%M:%S.%f")
    kt = [(review.datetime.strptime(r[1], "%H:%M:%S.%f") - t0).total_seconds() for r in srows]
    if len(kt) < 8:
        return dict(row, reason="too few kills")
    fixed = review.fixed_map(list(review._frames(video, keyframes=True))).astype(np.uint8)
    # 1. the offset, from the first kills (the offsets seen so far are 0.4 to 2.2 s)
    first = [t for t in kt if t < 10][:6]
    if len(first) < 3:
        first = kt[:4]
    end = min(dur, first[-1] + 3.0)
    tr = dict(fps=fps, frames=tracks_of(detect_stream(det, video, end, fixed), 0))
    fl, info = review.match_times(tr, first, [1] * len(first))
    if info.get("offset") is None or info.get("confirmed", 0) < max(2, (len(first) + 1) // 2):
        return dict(row, reason=f"no clock offset ({info.get('confirmed')} of {len(first)} first kills lined up)")
    off = info["offset"]
    row["offset"] = round(off, 4)
    # 2. the kills used: spread over the run, inside the video
    usable = [k for k, t in enumerate(kt) if 1.0 < t + off < dur - 0.5]
    rnd = random.Random(seed)
    picks = sorted(rnd.sample(usable, min(KILLS, len(usable))))
    back = [max(1, round(b * fps / 120)) for b in BACK]
    L, W = max(back) + int(0.1 * fps), int(0.25 * fps)
    rx, ry = review.CX, review.CY
    stem = hashlib.md5(video.encode()).hexdigest()[:10]
    d = Path(out) / split
    n, hidden = 0, 0
    for k in picks:
        kf = int(round((kt[k] + off) * fps))
        s0 = kf - L
        fr = decode(video, s0 / fps, (kf + 3) / fps)
        if len(fr) < L:
            continue
        boxes = detect(det, fr, fixed)
        frames = tracks_of(boxes, s0)
        pos = {}
        for f in frames:
            for tid, x, y in f["t"]:
                pos.setdefault(tid, {})[f["i"]] = (x, y)
        # 3. the killed target: the track nearest the crosshair that ends in the last quarter second
        best = None
        for tid, p in pos.items():
            last = max(i for i in p if i <= kf + 2)
            if last < kf - W:
                continue
            dist = math.hypot(*p[last])
            if dist < 1.5 and (best is None or dist + 4.0 * (kf - min(last, kf)) / fps < best[0]):
                best = (dist + 4.0 * (kf - min(last, kf)) / fps, tid, last)
        if best is None:
            continue
        _, tid, last = best
        shift = {f["i"]: f.get("shift") or [0.0, 0.0] for f in frames}
        place, p = {}, pos[tid]
        x, y = p[min(p)]
        for i in range(min(p), kf):                         # where the model lost it (or skipped a frame): follow the
            if i in p:                                      # camera's turn from where it was last seen
                x, y = p[i]
            else:
                x, y = x + shift.get(i, [0, 0])[0], y + shift.get(i, [0, 0])[1]
            place[i] = (x, y)
        if kf - 1 not in place:
            continue
        if math.hypot(*place[kf - 1]) > 1.5:
            continue
        # its size, from the frames where the model saw it
        sizes = []
        for i, (px, py) in pos[tid].items():
            b = boxes[i - s0]
            if len(b):
                cx, cy = to_px(px, py)
                j = int(np.argmin(np.hypot(b[:, 0] - cx, b[:, 1] - cy)))
                if math.hypot(b[j, 0] - cx, b[j, 1] - cy) < 2:
                    sizes.append(b[j, 2:4])
        if not sizes:
            continue
        w, h = np.median(np.array(sizes), 0)
        for bk in back:
            i = kf - bk
            if i - s0 < 0 or i not in place:
                continue
            seen = i in pos[tid]
            cx, cy = to_px(*place[i])
            if not seen:    # where the labeller lost it: label it only where some of it shows beside the crosshair,
                # unlike the wall round it. A label on the crosshair alone (the target gone, or wholly covered) taught
                # small_v4 to v6 the crosshair as a target
                r = max(1.5, 0.5 * w)
                R = int(3 * r) + 2
                ya, yb = max(0, int(cy) - R), min(review.H, int(cy) + R + 1)
                xa, xb = max(0, int(cx) - R), min(review.W, int(cx) + R + 1)
                yy, xx = np.mgrid[ya:yb, xa:xb]
                d2 = (xx - cx) ** 2 + (yy - cy) ** 2
                free = fixed[ya:yb, xa:xb] == 0
                vis, ring = (d2 <= r ** 2) & free, (d2 >= (1.6 * r) ** 2) & (d2 <= (2.6 * r) ** 2) & free
                if vis.sum() < 3 or ring.sum() < 6:
                    continue
                px = fr[i - s0][ya:yb, xa:xb].astype(np.float32)
                if np.linalg.norm(np.median(px[vis], 0) - np.median(px[ring], 0)) < 40:
                    continue
            # the other targets: at most the scenario's count minus the killed one, the model's most confident.
            # Everything else is background: labelling every detection taught wall seams as targets (small_v4)
            rest = sorted((b for b in boxes[i - s0] if math.hypot(b[0] - cx, b[1] - cy) > max(2.0, 0.5 * w)),
                          key=lambda b: -b[4])
            others = [b[:4] for b in (rest[:max(0, count - 1)] if count else [b for b in rest if b[4] >= 0.6])]
            labels = [np.array([cx, cy, w, h], np.float32)] + others
            x0 = int(np.clip(rx - CROP // 2 + rnd.randint(-48, 48), 0, review.W - CROP))
            y0 = int(np.clip(ry - CROP // 2 + rnd.randint(-48, 48), 0, review.H - CROP))
            bb = np.array([[b[0] - x0, b[1] - y0, b[2], b[3]] for b in labels
                           if x0 <= b[0] < x0 + CROP and y0 <= b[1] < y0 + CROP], np.float32).reshape(-1, 4)
            yy, xx = np.mgrid[0:CROP, 0:CROP]
            tmask = np.zeros((CROP, CROP), np.uint8)
            for b in bb:
                tmask[(xx - b[0]) ** 2 + (yy - b[1]) ** 2 <= max(1.5, 0.5 * b[2]) ** 2] = 1
            np.savez_compressed(d / f"{stem}_k{k:03d}_{bk:02d}.npz", rgb=fr[i - s0][y0:y0 + CROP, x0:x0 + CROP],
                                fixed=fixed[y0:y0 + CROP, x0:x0 + CROP], tmask=tmask, boxes=bb, hidden=np.uint8(not seen))
            n += 1
            hidden += not seen
    return dict(row, kept=n > 0, crops=n, hidden=hidden, kills=len(picks))


KILLS = 10                                  # kills used per VOD (--kills)
COUNTS = {}                                 # targets alive at once per scenario (build_data.target_counts)


def main():
    global KILLS, COUNTS
    ap = argparse.ArgumentParser()
    ap.add_argument("--vods", default=r"E:\OBS\KovOBS")
    ap.add_argument("--out", default="test_out/vod_model/data_kills")
    ap.add_argument("--per-folder", type=int, default=1)
    ap.add_argument("--also", help="a file of VOD hashes to include whatever --per-folder says (new runs)")
    ap.add_argument("--kills", type=int, default=10)
    ap.add_argument("--threads", type=int, default=4)
    a = ap.parse_args()
    KILLS = a.kills
    for s in ("train", "val", "test"):
        (Path(a.out) / s).mkdir(parents=True, exist_ok=True)
    lib = server.Library(a.vods, server.STATS_DEFAULT)
    lib.load_stats_index()
    static = build_data.static_scenarios()
    COUNTS = build_data.target_counts()
    also = set(Path(a.also).read_text().split()) if a.also else set()
    jobs = []
    for folder in sorted(p for p in Path(a.vods).iterdir() if p.is_dir()):
        if folder.name.lower() not in static:
            continue
        with_stats = []
        for v in sorted(folder.glob("*.mp4"), key=lambda p: -p.stat().st_mtime):
            m = server.NAME.match(v.name)
            s = lib.stats_for(m["scenario"], m["stamp"]) if m else None
            if s:
                with_stats.append((v, s))
        chosen = with_stats[:a.per_folder] + [x for x in with_stats[a.per_folder:]
                                              if hashlib.md5(str(x[0]).encode()).hexdigest()[:10] in also]
        jobs += [(folder.name, str(v), str(s), build_data.split_of(folder.name), a.out, i) for i, (v, s) in enumerate(chosen)]
    man = Path(a.out) / "manifest.jsonl"
    old = {}
    if man.exists():
        for line in man.read_text(encoding="utf-8").splitlines():
            if line.strip():
                r = json.loads(line)
                old[(r["folder"], r["file"])] = r
    rows, todo = [], []
    for j in jobs:
        r = old.get((j[0], Path(j[1]).name))
        (rows.append(r) if r and r.get("size") == Path(j[1]).stat().st_size else todo.append(j))
    print(f"{len(jobs)} VODs: {len(rows)} done before, {len(todo)} to do", flush=True)
    det = infer.TorchDetector(str(HERE / "exports" / f"detector_{LABELLER}.pt"))
    t0 = time.time()
    with ThreadPoolExecutor(a.threads) as pool:
        for k, row in enumerate(pool.map(lambda j: _safe(j, det), todo)):
            rows.append(row)
            with open(man, "w", encoding="utf-8") as f:
                f.writelines(json.dumps(r) + "\n" for r in sorted(rows, key=lambda r: (r["split"], r["folder"], r["file"])))
            print(f"[{k + 1}/{len(todo)}] {row['split']:5s} {'keep' if row['kept'] else 'drop'} {row['folder'][:40]:40s} "
                  f"{row.get('reason', '')} {row['crops']} crops, {row.get('hidden', 0)} hidden ({time.time() - t0:.0f} s)",
                  flush=True)
    for s in ("train", "val", "test"):
        rs = [r for r in rows if r["split"] == s]
        print(f"{s}: {sum(r['kept'] for r in rs)} of {len(rs)} VODs, {sum(r['crops'] for r in rs)} crops, "
              f"{sum(r.get('hidden', 0) for r in rs)} with the killed target lost by the model")


def _safe(job, det):
    try:
        return one(job, det)
    except Exception as e:                                  # one bad VOD does not stop the build
        return dict(folder=job[0], file=Path(job[1]).name, size=Path(job[1]).stat().st_size, split=job[3], kept=False,
                    crops=0, reason=f"{type(e).__name__}: {e}"[:120])


if __name__ == "__main__":
    main()
