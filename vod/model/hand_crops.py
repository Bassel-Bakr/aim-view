"""Crops for labelling by hand (label_check.py --data), from chosen VODs: frames sampled through each run, and one
256 x 256 crop per frame round what the current model finds there, its least sure find first (where a theme's tiles or
seams pass for targets). Each crop is saved in the training format (rgb, fixed, tmask, boxes, hidden), with boxes = the
model's finds that the review would keep (the scenario's target count, the crosshair's neighbours always), so the
label page starts from what the review sees and shows every other find for reference.
Usage: python vod/model/hand_crops.py <mp4> [<mp4> ...] --out test_out/vod_model/hand/<name> [--per-vod 50]

Then, once checked: python vod/model/hand_crops.py --labels <checked.jsonl> --out <dataset> [--test <text>] writes the
checked crops as a training set, with the hand boxes as labels ("skip" meant no target there, the user's way,
2026-10-02). Crop files are named hand_<...> so train.py --repeat can weight them (prefix "hand_crop_"). Crops whose
name contains --test (or any of its comma-separated parts) go to test/, the rest to train/.
"""
import argparse
import collections
import hashlib
import json
import math
import random
import subprocess
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
import infer  # noqa: E402
import review  # noqa: E402

CROP = 256


def cutoff_crops(video, frames, fps, start, end, exclude, offset, out, per=20, near=2.0):
    """Labels from a faint-target cut-off the user submitted in the review app: the tracks the cut leaves out are not
    targets, the ones it keeps are. Written as label_check.py writes its checks (crops in out/train, rows in
    out/checked.jsonl), so to_dataset turns them into a training set; they are not hand-checked one by one, so train.py
    weights them less (prefix "cut_crop_"). Guards: only frames inside the run (start to end, frame numbers), only
    crops that miss every exclude area (shares of the frame), and none holding a track too short to have a score
    (away from the crosshair). Up to `per` crops round a left-out track and `per` round a kept one, from frames spread
    over the run. Returns the number of crops."""
    q, _, level = review.faint_scores(frames, near)        # near=0 for tracking runs (review.without_faint)
    if level is None:
        return 0
    cut = level - offset
    W, H = review.W, review.H
    ex = [(b[0] * W, b[1] * H, b[2] * W, b[3] * H) for b in exclude]
    stem = f"{Path(video).stem[:40]}_{hashlib.md5(Path(video).name.encode()).hexdigest()[:6]}".replace(" ", "_")
    Path(out, "train").mkdir(parents=True, exist_ok=True)
    rnd = random.Random(stem)
    fixed = None
    rows = []

    def box_px(x, y, w, h):
        cx, cy = review.to_px(x, y)
        x0, y0 = review.to_px(x - w / 2, y + h / 2)
        x1, y1 = review.to_px(x + w / 2, y - h / 2)
        return cx, cy, abs(x1 - x0), abs(y1 - y0)

    for kind in ("left out", "kept"):
        cand = [i for i in range(max(0, start), min(len(frames), end + 1)) if "wh" in frames[i] and any(
            tid in q and (q[tid] < cut) == (kind == "left out") and math.hypot(x, y) >= near
            for tid, x, y in frames[i]["t"])]
        for i in [cand[int(k * len(cand) / per)] for k in range(min(per, len(cand)))]:
            f = frames[i]
            pts = [(tid, *box_px(x, y, *wh)) for (tid, x, y), wh in zip(f["t"], f["wh"])]
            focus = [p for p, (tid, x, y) in zip(pts, f["t"])
                     if tid in q and (q[tid] < cut) == (kind == "left out") and math.hypot(x, y) >= near]
            c = rnd.choice(focus)
            x0 = int(np.clip(c[1] - CROP // 2 + rnd.randint(-48, 48), 0, W - CROP))
            y0 = int(np.clip(c[2] - CROP // 2 + rnd.randint(-48, 48), 0, H - CROP))
            if any(a < x0 + CROP and x0 < b and c_ < y0 + CROP and y0 < d for a, c_, b, d in ex):
                continue                                # touches an exclude area
            inside = [(p, t) for p, t in zip(pts, f["t"]) if x0 <= p[1] < x0 + CROP and y0 <= p[2] < y0 + CROP]
            if any(t[0] not in q and math.hypot(t[1], t[2]) >= near for _, t in inside):
                continue                                # a track too short to judge
            if fixed is None:
                fixed = review.fixed_map(list(review._frames(video, keyframes=True))).astype(np.uint8)
            raw = subprocess.run(["ffmpeg", "-v", "error", "-ss", f"{i / fps:.3f}", "-i", video, "-frames:v", "1", "-vf",
                                  f"scale={W}:{H}:flags=area,format=rgb24", "-f", "rawvideo", "-"],
                                 capture_output=True).stdout
            if len(raw) != W * H * 3:
                continue
            rgb = np.frombuffer(raw, np.uint8).reshape(H, W, 3)
            keep = [[p[1] - x0, p[2] - y0, p[3], p[4]] for p, t in inside if t[0] not in q or q[t[0]] >= cut]
            every = [[p[1] - x0, p[2] - y0, p[3], p[4]] for p, _ in inside]
            name = f"train/{stem}_{i:06d}.npz"
            bb = np.array(keep, np.float32).reshape(-1, 4)
            np.savez_compressed(Path(out) / name, rgb=rgb[y0:y0 + CROP, x0:x0 + CROP].copy(),
                                fixed=fixed[y0:y0 + CROP, x0:x0 + CROP], tmask=np.zeros((CROP, CROP), np.uint8), boxes=bb,
                                hidden=np.uint8(0))
            rows.append(dict(file=name, boxes=[[round(v, 2) for v in b] for b in keep], verdict="correct",
                             model=[[round(v, 2) for v in b] for b in every], source="cutoff", video=str(video),
                             offset=offset, cut=round(cut, 3)))
    with open(Path(out) / "checked.jsonl", "a", encoding="utf-8") as fh:  # a later submit's rows win (to_dataset)
        for r in rows:
            fh.write(json.dumps(r) + "\n")
    return len(rows)


def to_dataset(labels, out, test=None, prefix="hand_crop_"):
    """The checked crops as a dataset: rgb and fixed from the crop, the hand boxes as labels. A box clicked without a
    drag gets the size of the model's find under it (within 3 px), or at least 4 px. prefix names the crops for
    train.py --repeat: "hand_crop_" for hand checks (counted 20 times), "cut_crop_" for cut-off labels (5 times)."""
    last = {}
    for line in Path(labels).read_text(encoding="utf-8").splitlines():
        if line.strip():
            r = json.loads(line)
            last[r["file"]] = r
    src = Path(labels).parent
    n = collections.Counter()
    for r in last.values():
        if r["verdict"] == "unsure":                    # "Can't tell": left out
            continue
        z = np.load(src / r["file"])
        finds = r.get("model", []) + r.get("auto", [])
        boxes = []
        for b in r["boxes"] if r["verdict"] == "correct" else []:
            near = [m for m in finds if math.hypot(m[0] - b[0], m[1] - b[1]) <= 3]
            w = near[0][2] if near and b[2] <= 3 else max(4.0, b[2])
            h = near[0][3] if near and b[3] <= 3 else max(4.0, b[3])
            boxes.append([b[0], b[1], w, h])
        bb = np.array(boxes, np.float32).reshape(-1, 4)
        yy, xx = np.mgrid[0:CROP, 0:CROP]
        tmask = np.zeros((CROP, CROP), np.uint8)
        for b in bb:
            tmask[(xx - b[0]) ** 2 + (yy - b[1]) ** 2 <= max(1.5, 0.5 * b[2]) ** 2] = 1
        split = "test" if test and any(t in r["file"] for t in test.split(",")) else "train"
        (Path(out) / split).mkdir(parents=True, exist_ok=True)
        name = prefix + Path(r["file"]).stem
        np.savez_compressed(Path(out) / split / f"{name}.npz", rgb=z["rgb"], fixed=z["fixed"], tmask=tmask, boxes=bb,
                            hidden=np.uint8(0))
        n[split] += 1
    print(dict(n), "crops in", out)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("vods", nargs="*")
    ap.add_argument("--out", required=True)
    ap.add_argument("--per-vod", type=int, default=50)
    ap.add_argument("--centre", choices=("find", "crosshair", "mixed"), default="find",
                    help="the crop round the least sure find, round the crosshair (targets under it), or every other one")
    ap.add_argument("--labels")
    ap.add_argument("--test")
    ap.add_argument("--prefix", default="hand_crop_", help="crop name prefix: cut_crop_ for the cut-off labels")
    a = ap.parse_args()
    if a.labels:
        return to_dataset(a.labels, a.out, a.test, a.prefix)
    out = Path(a.out) / "train"
    out.mkdir(parents=True, exist_ok=True)
    det = infer.TorchDetector(str(HERE / "exports" / f"detector_{infer.BEST}.pt"))
    rnd = random.Random(1)
    n = 0
    for v in a.vods:
        fps, dur = review.probe(v)
        count = review.target_counts().get(Path(v).stem.rsplit(" - ", 2)[0].lower())
        fixed = review.fixed_map(list(review._frames(v, keyframes=True))).astype(np.uint8)
        step = (dur - 4.0) / a.per_vod
        raw = subprocess.run(["ffmpeg", "-v", "error", "-ss", "2", "-i", v, "-t", f"{dur - 4.0:.2f}", "-vf",
                              f"fps=1/{step:.4f},scale={review.W}:{review.H}:flags=area,format=rgb24", "-f", "rawvideo", "-"],
                             capture_output=True).stdout
        frames = np.frombuffer(raw, np.uint8).reshape(-1, review.H, review.W, 3)
        for i, rgb in enumerate(frames):
            d = det(rgb, fixed)
            at_cross = a.centre == "crosshair" or (a.centre == "mixed" and i % 2 == 0)
            if not len(d) and not at_cross:
                continue
            # what the review keeps (review.track_model's cap): near the crosshair, then the most confident others
            dist = [math.hypot(*review.to_deg(float(b[0]), float(b[1]))) for b in d]
            near = [b for b, r in zip(d, dist) if r < 2.0]
            rest = sorted((b for b, r in zip(d, dist) if r >= 2.0), key=lambda b: -b[4])
            keep = near + rest[:max(0, count - len(near))] + [b for b in rest[max(0, count - len(near)):][:1] if b[4] >= 0.5] \
                if count else list(d)
            c = (review.CX, review.CY) if at_cross else min(d, key=lambda b: b[4])   # or the least sure find
            x0 = int(np.clip(c[0] - CROP // 2 + rnd.randint(-48, 48), 0, review.W - CROP))
            y0 = int(np.clip(c[1] - CROP // 2 + rnd.randint(-48, 48), 0, review.H - CROP))
            bb = np.array([[b[0] - x0, b[1] - y0, b[2], b[3]] for b in keep
                           if x0 <= b[0] < x0 + CROP and y0 <= b[1] < y0 + CROP], np.float32).reshape(-1, 4)
            yy, xx = np.mgrid[0:CROP, 0:CROP]
            tmask = np.zeros((CROP, CROP), np.uint8)
            for b in bb:
                tmask[(xx - b[0]) ** 2 + (yy - b[1]) ** 2 <= max(1.5, 0.5 * b[2]) ** 2] = 1
            name = f"{Path(v).stem[:40]}_{hashlib.md5(Path(v).name.encode()).hexdigest()[:6]}_{i:03d}".replace(" ", "_")
            np.savez_compressed(out / f"{name}.npz", rgb=rgb[y0:y0 + CROP, x0:x0 + CROP].copy(),
                                fixed=fixed[y0:y0 + CROP, x0:x0 + CROP], tmask=tmask, boxes=bb, hidden=np.uint8(0))
            n += 1
        print(f"{Path(v).name}: {len(frames)} frames", flush=True)
    print(f"{n} crops in {out}")


if __name__ == "__main__":
    main()
