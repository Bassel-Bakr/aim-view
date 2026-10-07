"""Crops where two detectors disagree, for a check by eye on the Crops page (crop_check/README.md).

Each recording is reviewed as the app reviews it (aimview-tool review) with the model checked and with a reference,
kept in <out>/reviews/<model>/<stem>/ so a rerun skips it. A frame gives a crop when the reference boxes a target that
the model misses ("missed": no box of the model within MATCH_PX of it), or the model boxes something the reference
does not ("extra"), inside the run only (from the end of KovaaK's countdown to the stats file's length, half a
second in from each end, as build_mined.py's window; a recording without both is skipped). The picks are spread over
each recording (at least MIN_GAP_S apart), missed ones first. Each crop is
256 x 256 round that box (shifted up to 48 px at random), saved like build_mined.py's: rgb, fixed, tmask, boxes (the
frame's boxes of the reference for a missed target, of the model for an extra one), scores, frame and why, in the
split folder build_data.split_of gives its scenario folder, with a manifest.jsonl (stem, folder, kind) for
make_page.py. Leave the gate's own recordings out: crops trained on would make its checks no test of them.

Usage: python python/model/build_disagreements.py <out> <model> <reference> <video or folder> ... [--per-recording 25]
       [--seed 0]
"""
import argparse
import hashlib
import json
import math
import random
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
import aimview_tools  # noqa: E402
import build_data  # noqa: E402
import build_mined  # noqa: E402
import old_review  # noqa: E402

CROP, JITTER_PX = build_mined.CROP, build_mined.JITTER_PX
WIDTH, HEIGHT = build_mined.WIDTH, build_mined.HEIGHT
MATCH_PX = 12                   # a box of the other model this near (px) is the same target
MIN_GAP_S = 0.25                # picks of one recording at least this far apart, so they are not near copies
MISSED_SHARE = 0.8              # the share of a recording's picks that are missed targets, when it has enough


def boxes_px(frame):
    """A tracks.json frame's boxes as (cx, cy, w, h, score) in 1280 x 720 pixels."""
    out = []
    for (_, x, y), (width_deg, height_deg), score in zip(frame["t"], frame.get("wh", []), frame.get("s", [])):
        cx, cy = old_review.to_px(x, y)
        out.append((cx, cy, *build_mined.px_size(x, y, width_deg, height_deg), score))
    return out


def reviewed(lib, video, model, out):
    """A recording's frames as `model` tracks them (reviewed once, kept under out/reviews)."""
    folder = out / "reviews" / model / Path(video).stem
    if not (folder / "tracks.json").is_file():
        lib.review_video(str(video), model, str(folder), quiet=True)
    return json.loads((folder / "tracks.json").read_text())["frames"]


def unmatched(boxes, others):
    """The boxes with no box of `others` within MATCH_PX of their center."""
    return [box for box in boxes if all(math.hypot(box[0] - other[0], box[1] - other[1]) > MATCH_PX
                                        for other in others)]


def run_window(folder, stats, fps, frame_count):
    """The run's frames (first, last) from a review's countdown and the stats file's length, or None."""
    readings = json.loads((folder / "readings.json").read_text())
    counting = [i for i, on in enumerate(readings.get("countdown") or []) if on]
    length = old_review.stats_length(str(stats)) if stats else None
    if not counting or not length:
        return None
    start = (counting[-1] + 1) / fps
    return (round((start + build_mined.EDGE_S) * fps),
            min(frame_count - 1, round((start + length - build_mined.EDGE_S) * fps)))


def disagreements(frames, reference, window):
    """{frame: (rule, the box it is about, the boxes that label the crop)} for each frame of the run window where the
    two differ. A frame with a missed target is "missed" even when it also has an extra box; the box it is about is
    the highest scored of its kind."""
    found = {}
    for frame, ref_frame in zip(frames, reference):
        if not window[0] <= frame["i"] <= window[1]:
            continue
        mine, theirs = boxes_px(frame), boxes_px(ref_frame)
        if missed := unmatched(theirs, mine):
            found[frame["i"]] = ("missed", max(missed, key=lambda box: box[4]), theirs)
        elif extra := unmatched(mine, theirs):
            found[frame["i"]] = ("extra", max(extra, key=lambda box: box[4]), mine)
    return found


def spread(candidates, fps, count, rnd):
    """Up to `count` frames from candidates, at least MIN_GAP_S apart, in random order."""
    order = list(candidates)
    rnd.shuffle(order)
    picked = []
    for i in order:
        if len(picked) == count:
            break
        if all(abs(i - j) >= MIN_GAP_S * fps for j in picked):
            picked.append(i)
    return sorted(picked)


def picks_of(found, fps, count, rnd):
    """A recording's picks: missed targets first (MISSED_SHARE of them), extra boxes for the rest."""
    missed = spread([i for i, (rule, *_) in found.items() if rule == "missed"], fps, round(count * MISSED_SHARE), rnd)
    extra = spread([i for i, (rule, *_) in found.items() if rule == "extra"], fps, count - len(missed), rnd)
    return sorted(missed + extra)


def save_crop(path, frame_rgb, fixed, about, labels, i, why, rnd):
    """One crop round the box `about`, labelled with the boxes inside it."""
    x0 = int(np.clip(about[0] - CROP // 2 + rnd.randint(-JITTER_PX, JITTER_PX), 0, WIDTH - CROP))
    y0 = int(np.clip(about[1] - CROP // 2 + rnd.randint(-JITTER_PX, JITTER_PX), 0, HEIGHT - CROP))
    inside = [box for box in labels if x0 <= box[0] < x0 + CROP and y0 <= box[1] < y0 + CROP]
    crop_boxes = np.array([(box[0] - x0, box[1] - y0, box[2], box[3]) for box in inside], np.float32).reshape(-1, 4)
    target_mask = np.zeros((CROP, CROP), np.uint8)
    yy, xx = np.ogrid[0:CROP, 0:CROP]
    for box_x, box_y, box_w, box_h in crop_boxes:
        target_mask[((xx - box_x) / max(1.0, box_w / 2)) ** 2 + ((yy - box_y) / max(1.0, box_h / 2)) ** 2 <= 1] = 1
    np.savez_compressed(path, rgb=frame_rgb[y0:y0 + CROP, x0:x0 + CROP], fixed=fixed[y0:y0 + CROP, x0:x0 + CROP],
                        tmask=target_mask, boxes=crop_boxes, scores=np.array([box[4] for box in inside], np.float32),
                        frame=np.int32(i), why=np.str_(why))


def crops_of(lib, video, args, rnd):
    """Writes one recording's crops into its split folder and gives its manifest row (with no stem and no crops when
    it has no run window)."""
    video = Path(video)
    frames = reviewed(lib, video, args.model, args.out)
    reference = reviewed(lib, video, args.reference, args.out)
    folder = args.out / "reviews" / args.model / video.stem
    fps = json.loads((folder / "tracks.json").read_text())["fps"]
    window = run_window(folder, lib.stats_of(None, video), fps, len(frames))
    if window is None:
        print(f"{video.name}: no countdown or stats file, so no run window; skipped", flush=True)
        return dict(stem=None, folder=video.parent.name, video=str(video), crops=0)
    found = disagreements(frames, reference, window)
    picks = picks_of(found, fps, args.per_recording, rnd)
    stem = hashlib.md5(video.name.encode()).hexdigest()[:10]
    folder = args.out / build_data.split_of(video.parent.name)
    folder.mkdir(parents=True, exist_ok=True)
    decoded = build_mined.decode(video, picks)
    fixed = old_review.fixed_map(build_data.keyframes(video, "yuv420p")).astype(np.uint8)
    for n, i in enumerate(sorted(decoded)):
        rule, about, labels = found[i]
        why = (f"{args.model} has no box here; {args.reference} scores it {about[4]:.2f}" if rule == "missed"
               else f"{args.model} boxes this ({about[4]:.2f}); {args.reference} has no box here")
        save_crop(folder / f"{stem}_{i:05d}_{rule[0]}{n:02d}.npz", decoded[i], fixed, about, labels, i, why, rnd)
    counts = {rule: sum(found[i][0] == rule for i in decoded) for rule in ("missed", "extra")}
    print(f"{video.name}: {len(found)} frames differ, {len(decoded)} crops {counts}", flush=True)
    return dict(stem=stem, folder=video.parent.name, kind="tracking", video=str(video), crops=len(decoded), **counts)


def videos_of(paths):
    """Each path that is a file, and the .mp4 files of each folder, in name order."""
    for path in map(Path, paths):
        yield from sorted(path.glob("*.mp4")) if path.is_dir() else [path]


def main():
    """Writes every recording's crops and <out>/manifest.jsonl, and prints the crop count."""
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("out", type=Path)
    parser.add_argument("model")
    parser.add_argument("reference")
    parser.add_argument("videos", nargs="+", help="recordings, or folders of them")
    parser.add_argument("--per-recording", type=int, default=25)
    parser.add_argument("--seed", type=int, default=0)
    args = parser.parse_args()
    lib = aimview_tools.Library()
    rnd = random.Random(args.seed)
    rows = [crops_of(lib, video, args, rnd) for video in videos_of(args.videos)]
    (args.out / "manifest.jsonl").write_text("".join(json.dumps(row) + "\n" for row in rows))
    print(f"{sum(row['crops'] for row in rows)} crops from {len(rows)} recordings in {args.out}")


if __name__ == "__main__":
    main()
