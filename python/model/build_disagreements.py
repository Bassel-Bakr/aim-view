"""Crops where two detectors disagree, for a check by eye on the Crops page (crop_check/README.md).

Each recording is reviewed as the app reviews it (aimview-tool review) with the model checked and with a reference,
kept in <out>/reviews/<model>/<stem>/ so a rerun skips it. A frame gives a crop when the reference boxes a target that
the model misses ("missed": no box of the model within MATCH_PX of it), or the model boxes something the reference
does not ("extra"), inside the run only (from the end of KovaaK's countdown to the stats file's length, half a
second in from each end, crop_batch.run_window; a recording without both is skipped). The picks are spread over
each recording (at least MIN_GAP_S apart), missed ones first. Each crop is
256 x 256 round that box (shifted up to 48 px at random), saved like build_mined.py's: rgb, fixed, tmask, boxes (the
frame's boxes of the reference for a missed target, of the model for an extra one), scores, frame and why, in the
split folder build_data.split_of gives its scenario folder, with a manifest.jsonl (stem, folder, kind) for
make_page.py. Leave the gate's own recordings out: crops trained on would make its checks no test of them.

Usage: python python/model/build_disagreements.py <out> <model> <reference> <video or folder> ... [--per-recording 25]
       [--seed 0]
"""
import json
import math
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
import build_data  # noqa: E402
import build_mined  # noqa: E402
import crop_batch  # noqa: E402
from crop_batch import boxes_px  # noqa: E402

CROP, JITTER_PX = build_mined.CROP, build_mined.JITTER_PX
WIDTH, HEIGHT = build_mined.WIDTH, build_mined.HEIGHT
MATCH_PX = 12                   # a box of the other model this near (px) is the same target
MIN_GAP_S = 0.25                # picks of one recording at least this far apart, so they are not near copies
MISSED_SHARE = 0.8              # the share of a recording's picks that are missed targets, when it has enough


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
    crop_batch.save_crop_npz(path, frame_rgb[y0:y0 + CROP, x0:x0 + CROP], fixed[y0:y0 + CROP, x0:x0 + CROP],
                             target_mask, crop_boxes, [box[4] for box in inside], i, why)


def crops_of(lib, video, args, rnd):
    """Writes one recording's crops into its split folder and gives its manifest row (with no stem and no crops when
    it has no run window)."""
    video = Path(video)
    frames = reviewed(lib, video, args.model, args.out)
    reference = reviewed(lib, video, args.reference, args.out)
    folder = args.out / "reviews" / args.model / video.stem
    fps = json.loads((folder / "tracks.json").read_text())["fps"]
    window = crop_batch.run_window(folder, lib.stats_of(None, video), fps, len(frames))
    if window is None:
        print(f"{video.name}: no countdown or stats file, so no run window; skipped", flush=True)
        return dict(stem=None, folder=video.parent.name, video=str(video), crops=0)
    found = disagreements(frames, reference, window)
    picks = picks_of(found, fps, args.per_recording, rnd)
    stem = crop_batch.recording_stem(video)
    folder = args.out / build_data.split_of(video.parent.name)
    folder.mkdir(parents=True, exist_ok=True)
    decoded = build_mined.decode(video, picks)
    fixed = crop_batch.fixed_mask(video, np.uint8)
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


def add_arguments(parser):
    """The arguments besides crop_batch.run_builder's: the reference model, the recordings and --per-recording."""
    parser.add_argument("reference")
    parser.add_argument("videos", nargs="+", help="recordings, or folders of them")
    parser.add_argument("--per-recording", type=int, default=25)


def main():
    """Writes every recording's crops and <out>/manifest.jsonl, and prints the crop count."""
    crop_batch.run_builder(__doc__.split("\n\n")[0], add_arguments,
                           lambda lib, args, rnd: [(video,) for video in videos_of(args.videos)], crops_of,
                           program=False)


if __name__ == "__main__":
    main()
