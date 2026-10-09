"""Crops for labelling by hand (label_check.py --data), from chosen VODs: frames sampled through each run, and one
256 x 256 crop per frame round what the current model finds there, its least sure find first (where a theme's tiles or
seams pass for targets). Each crop is saved in the training format (rgb, fixed, tmask, boxes, hidden), with boxes = the
model's finds that the review would keep (the scenario's target count, the crosshair's neighbors always), so the
label page starts from what the review sees and shows every other find for reference.
Usage: python python/model/hand_crops.py <mp4> [<mp4> ...] --out test_out/vod_model/hand/<name> [--per-vod 50]
       [--centre find|crosshair|mixed]

Then, once checked: python python/model/hand_crops.py --labels <checked.jsonl> --out <dataset> [--test <text>]
[--prefix hand_crop_] writes the checked crops as a training set, with the hand boxes as labels ("skip" meant no target
there, the user's way, 2026-10-02). Crop files are named <prefix><...> so train.py --repeat can weight them. Crops
whose name contains --test (or any of its comma-separated parts) go to test/, the rest to train/.
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
import old_review  # noqa: E402
import recording_names  # noqa: E402

CROP = 256
JITTER_PX = 48                  # a crop's corner moves up to this far at random
STEM_CHARS = 40                 # a crop name keeps this much of the video's name
HASH_CHARS = 6                  # and this much of its name's md5
CLICK_SIDE_PX = 3               # to_dataset: a box side this small was a click without a drag
CLICK_MATCH_PX = 3              # the model's find under a click, within this
MIN_SIDE_PX = 4.0               # to_dataset: a hand box's smallest side
MIN_RADIUS_PX = 1.5             # the target mask's smallest disc
NEAR_DEG = 2.0                  # the review keeps every box this close to the crosshair
EXTRA_SCORE = 0.5               # one box past the target count needs this score (the review's rule)
EDGE_S = 2.0                    # main: seconds left out at each end of a VOD


def crop_name(video):
    """A video's crop-name stem: the start of its name and the start of its name's md5. Its spaces stay; the callers
    change them to "_"."""
    return f"{Path(video).stem[:STEM_CHARS]}_{hashlib.md5(Path(video).name.encode()).hexdigest()[:HASH_CHARS]}"


def disc_mask(boxes):
    """The target mask: a disc of half each box's width (at least MIN_RADIUS_PX) round its center."""
    yy, xx = np.mgrid[0:CROP, 0:CROP]
    target_mask = np.zeros((CROP, CROP), np.uint8)
    for box in boxes:
        target_mask[(xx - box[0]) ** 2 + (yy - box[1]) ** 2 <= max(MIN_RADIUS_PX, 0.5 * box[2]) ** 2] = 1
    return target_mask


def box_px(x, y, width_deg, height_deg):
    """A track's place and size in degrees as a box in frame pixels (cx, cy, w, h)."""
    cx, cy = old_review.to_px(x, y)
    x0, y0 = old_review.to_px(x - width_deg / 2, y + height_deg / 2)
    x1, y1 = old_review.to_px(x + width_deg / 2, y - height_deg / 2)
    return cx, cy, abs(x1 - x0), abs(y1 - y0)


def decode_frame(video, frame, fps):
    """One frame (RGB, the review's 1280 x 720), or None when ffmpeg gives none."""
    width, height = old_review.W, old_review.H
    raw = subprocess.run(["ffmpeg", "-v", "error", "-ss", f"{frame / fps:.3f}", "-i", video, "-frames:v", "1", "-vf",
                          f"scale={width}:{height}:flags=area,format=rgb24", "-f", "rawvideo", "-"],
                         capture_output=True).stdout
    if len(raw) != width * height * 3:
        return None
    return np.frombuffer(raw, np.uint8).reshape(height, width, 3)


def cutoff_crops(video, frames, fps, start, end, exclude, offset, out, per=20, near=NEAR_DEG):
    """Labels from a faint-target cut-off the user submitted in the review app: the tracks the cut leaves out are not
    targets, the ones it keeps are. Written as label_check.py writes its checks (crops in out/train, rows in
    out/checked.jsonl), so to_dataset turns them into a training set; they are not hand-checked one by one, so train.py
    weights them less (prefix "cut_crop_"). Guards: only frames inside the run (start to end, frame numbers), only
    crops that miss every exclude area (shares of the frame), and none holding a track too short to have a score
    (away from the crosshair). Up to `per` crops round a left-out track and `per` round a kept one, from frames spread
    over the run. Returns the number of crops."""
    scores, _, level = old_review.faint_scores(frames, near)   # near=0 for tracking runs (old_review.without_faint)
    if level is None:
        return 0
    cut = level - offset
    width, height = old_review.W, old_review.H
    excluded = [(box[0] * width, box[1] * height, box[2] * width, box[3] * height) for box in exclude]
    stem = crop_name(video).replace(" ", "_")
    Path(out, "train").mkdir(parents=True, exist_ok=True)
    rnd = random.Random(stem)
    fixed = None
    rows = []

    def in_focus(track, x, y, kind):
        """Whether a track at (x, y) degrees is one this pass crops round: scored, on the `kind` side of the cut,
        and not near the crosshair."""
        return track in scores and (scores[track] < cut) == (kind == "left out") and math.hypot(x, y) >= near

    for kind in ("left out", "kept"):
        candidates = [i for i in range(max(0, start), min(len(frames), end + 1)) if "wh" in frames[i] and any(
            in_focus(track, x, y, kind) for track, x, y in frames[i]["t"])]
        for i in [candidates[int(k * len(candidates) / per)] for k in range(min(per, len(candidates)))]:
            frame = frames[i]
            boxes = [(track, *box_px(x, y, *size)) for (track, x, y), size in zip(frame["t"], frame["wh"])]
            focus = [box for box, (track, x, y) in zip(boxes, frame["t"]) if in_focus(track, x, y, kind)]
            center = rnd.choice(focus)
            x0 = int(np.clip(center[1] - CROP // 2 + rnd.randint(-JITTER_PX, JITTER_PX), 0, width - CROP))
            y0 = int(np.clip(center[2] - CROP // 2 + rnd.randint(-JITTER_PX, JITTER_PX), 0, height - CROP))
            if any(left < x0 + CROP and x0 < right and top < y0 + CROP and y0 < bottom
                   for left, top, right, bottom in excluded):
                continue                                # touches an exclude area
            inside = [(box, point) for box, point in zip(boxes, frame["t"])
                      if x0 <= box[1] < x0 + CROP and y0 <= box[2] < y0 + CROP]
            if any(point[0] not in scores and math.hypot(point[1], point[2]) >= near for _, point in inside):
                continue                                # a track too short to judge
            if fixed is None:
                fixed = old_review.fixed_map(list(old_review._frames(video, keyframes=True))).astype(np.uint8)
            rgb = decode_frame(video, i, fps)
            if rgb is None:
                continue
            rows.append(cutoff_crop(rgb, fixed, inside, scores, cut, (x0, y0), f"train/{stem}_{i:06d}.npz", out))
            rows[-1].update(video=str(video), offset=offset, cut=round(cut, 3))
    with open(Path(out) / "checked.jsonl", "a", encoding="utf-8") as checks:  # a later submit's rows win (to_dataset)
        for row in rows:
            checks.write(json.dumps(row) + "\n")
    return len(rows)


def cutoff_crop(rgb, fixed, inside, scores, cut, corner, name, out):
    """One cut-off crop saved at out/name, with the kept tracks' boxes as its labels; its checked.jsonl row."""
    x0, y0 = corner
    keep = [[box[1] - x0, box[2] - y0, box[3], box[4]] for box, point in inside
            if point[0] not in scores or scores[point[0]] >= cut]
    every = [[box[1] - x0, box[2] - y0, box[3], box[4]] for box, _ in inside]
    labels = np.array(keep, np.float32).reshape(-1, 4)
    np.savez_compressed(Path(out) / name, rgb=rgb[y0:y0 + CROP, x0:x0 + CROP].copy(),
                        fixed=fixed[y0:y0 + CROP, x0:x0 + CROP], tmask=np.zeros((CROP, CROP), np.uint8), boxes=labels,
                        hidden=np.uint8(0))
    return dict(file=name, boxes=[[round(value, 2) for value in box] for box in keep], verdict="correct",
                model=[[round(value, 2) for value in box] for box in every], source="cutoff")


def hand_boxes(row):
    """A checked crop's hand boxes. A box clicked without a drag gets the size of the model's find under it (within
    3 px), or at least 4 px."""
    finds = row.get("model", []) + row.get("auto", [])
    boxes = []
    for box in row["boxes"] if row["verdict"] == "correct" else []:
        near = [find for find in finds if math.hypot(find[0] - box[0], find[1] - box[1]) <= CLICK_MATCH_PX]
        width = near[0][2] if near and box[2] <= CLICK_SIDE_PX else max(MIN_SIDE_PX, box[2])
        height = near[0][3] if near and box[3] <= CLICK_SIDE_PX else max(MIN_SIDE_PX, box[3])
        boxes.append([box[0], box[1], width, height])
    return np.array(boxes, np.float32).reshape(-1, 4)


def to_dataset(labels, out, test=None, prefix="hand_crop_"):
    """The checked crops as a dataset: rgb and fixed from the crop, the hand boxes as labels (hand_boxes). prefix
    names the crops for train.py --repeat: "hand_crop_" for hand checks (counted 20 times), "cut_crop_" for cut-off
    labels (5 times)."""
    last = {}
    for line in Path(labels).read_text(encoding="utf-8").splitlines():
        if line.strip():
            row = json.loads(line)
            last[row["file"]] = row
    source = Path(labels).parent
    counts = collections.Counter()
    for row in last.values():
        if row["verdict"] == "unsure":                  # "Can't tell": left out
            continue
        crop = np.load(source / row["file"])
        boxes = hand_boxes(row)
        split = "test" if test and any(part in row["file"] for part in test.split(",")) else "train"
        (Path(out) / split).mkdir(parents=True, exist_ok=True)
        name = prefix + Path(row["file"]).stem
        np.savez_compressed(Path(out) / split / f"{name}.npz", rgb=crop["rgb"], fixed=crop["fixed"],
                            tmask=disc_mask(boxes), boxes=boxes, hidden=np.uint8(0))
        counts[split] += 1
    print(dict(counts), "crops in", out)


def review_keeps(detections, count):
    """What the review keeps (old_review.track_model's cap): the boxes near the crosshair, then the most confident
    others up to the target count, and one more past it when it scores EXTRA_SCORE; all of them without a count."""
    if not count:
        return list(detections)
    distance = [math.hypot(*old_review.to_deg(float(box[0]), float(box[1]))) for box in detections]
    near = [box for box, deg in zip(detections, distance) if deg < NEAR_DEG]
    rest = sorted((box for box, deg in zip(detections, distance) if deg >= NEAR_DEG), key=lambda box: -box[4])
    room = max(0, count - len(near))
    return near + rest[:room] + [box for box in rest[room:][:1] if box[4] >= EXTRA_SCORE]


def vod_crops(video, detector, args, rnd, out):
    """One VOD's crops for labelling; returns the number of frames sampled and of crops written."""
    _, duration = old_review.probe(video)
    count = old_review.target_counts().get(recording_names.scenario_of(video))
    fixed = old_review.fixed_map(list(old_review._frames(video, keyframes=True))).astype(np.uint8)
    step = (duration - 2 * EDGE_S) / args.per_vod
    raw = subprocess.run(["ffmpeg", "-v", "error", "-ss", f"{EDGE_S:g}", "-i", video, "-t",
                          f"{duration - 2 * EDGE_S:.2f}", "-vf",
                          f"fps=1/{step:.4f},scale={old_review.W}:{old_review.H}:flags=area,format=rgb24", "-f",
                          "rawvideo", "-"], capture_output=True).stdout
    frames = np.frombuffer(raw, np.uint8).reshape(-1, old_review.H, old_review.W, 3)
    written = 0
    for i, rgb in enumerate(frames):
        detections = detector(rgb, fixed)
        at_crosshair = args.centre == "crosshair" or (args.centre == "mixed" and i % 2 == 0)
        if not len(detections) and not at_crosshair:
            continue
        keep = review_keeps(detections, count)
        center = ((old_review.CX, old_review.CY) if at_crosshair
                  else min(detections, key=lambda box: box[4]))   # or the least sure find
        x0 = int(np.clip(center[0] - CROP // 2 + rnd.randint(-JITTER_PX, JITTER_PX), 0, old_review.W - CROP))
        y0 = int(np.clip(center[1] - CROP // 2 + rnd.randint(-JITTER_PX, JITTER_PX), 0, old_review.H - CROP))
        boxes = np.array([[box[0] - x0, box[1] - y0, box[2], box[3]] for box in keep
                          if x0 <= box[0] < x0 + CROP and y0 <= box[1] < y0 + CROP], np.float32).reshape(-1, 4)
        name = f"{crop_name(video)}_{i:03d}".replace(" ", "_")
        np.savez_compressed(out / f"{name}.npz", rgb=rgb[y0:y0 + CROP, x0:x0 + CROP].copy(),
                            fixed=fixed[y0:y0 + CROP, x0:x0 + CROP], tmask=disc_mask(boxes), boxes=boxes,
                            hidden=np.uint8(0))
        written += 1
    return len(frames), written


def main():
    """With --labels, writes the checked crops as a dataset; else writes each VOD's crops for labelling into
    <out>/train and prints the counts."""
    parser = argparse.ArgumentParser()
    parser.add_argument("vods", nargs="*")
    parser.add_argument("--out", required=True)
    parser.add_argument("--per-vod", type=int, default=50)
    parser.add_argument("--centre", choices=("find", "crosshair", "mixed"), default="find",
                        help="the crop round the least sure find, round the crosshair (targets under it), or every "
                        "other one")
    parser.add_argument("--labels")
    parser.add_argument("--test")
    parser.add_argument("--prefix", default="hand_crop_", help="crop name prefix: cut_crop_ for the cut-off labels")
    args = parser.parse_args()
    if args.labels:
        return to_dataset(args.labels, args.out, args.test, args.prefix)
    out = Path(args.out) / "train"
    out.mkdir(parents=True, exist_ok=True)
    detector = infer.TorchDetector(str(HERE / "exports" / f"detector_{infer.BEST}.pt"))
    rnd = random.Random(1)
    written = 0
    for video in args.vods:
        frames, crops = vod_crops(video, detector, args, rnd, out)
        written += crops
        print(f"{Path(video).name}: {frames} frames", flush=True)
    print(f"{written} crops in {out}")


if __name__ == "__main__":
    main()
