"""Training crops of bots' health bars, which are never targets (REPRODUCE.md step 1). The model boxes a bar's colored
fill at the edge of its threshold (full_v7 on Pasu Switch Wide: 169 boxes, which cost 11 of 46 kills); each crop
shows a bar the model boxes, that box crossed out, and the other targets boxed.

The recordings: build_mined.py's reviews (full_v3's tracks, <mined>/reviews/), the stats-file checks' runs left out
(build_data.check_runs). Their tracks give the frames worth a look: a box above a bigger box (as the bar sits over its
bot). Those frames are decoded once per recording (ffmpeg, from the start: frame numbers equal the review's) and run
through --model (the PyTorch export) at BOX_THRESHOLD. A box there is a bar when it sits above a bigger box (at most
half its area, its center within the bot's width, 0.4 to 2 bot heights higher), its pixels are not the bot's (at most
BOT_LIKE_SHARE within BOT_COLOR_DIFF of the bot's middle color, and DISTINCT_SHARE of them neither the bot's color nor
the wall's: a bar's fill, of any color, as KovaaK's lets the player pick it), and it is part of a strip running
sideways (runs_sideways). A bot's head or a target beside another is the bot's own color, so it is never taken (both
are on Pasu Switch Wide and mccoyfrozentrack), nor is a small colored target beside a bot (the wall is on both its
sides).

Each bar gives one 256 x 256 crop round it (shifted up to 48 px at random), saved like build_mined.py's: rgb, fixed,
tmask, boxes (the model's other boxes there, each scoring TARGET_SCORE or more), scores, mined ("false_bar"), fix (the
bar's box, in the crop), frame and why. A crop is left out when another box in it scores under TARGET_SCORE (not sure
it is a target), when it holds a second bar, or when a box crosses its edge. At most --per-recording crops a recording,
SPACING_S apart. Splits follow build_data.split_of; a manifest.jsonl names each recording's stem, folder and kind.
Each recording's random numbers are seeded by its review's name, so --part K --parts N (the reviews whose place in
name order is K modulo N: run the N parts at once) gives the same crops as one run. A recording that already has
crops in --out is skipped.
Usage: python python/model/build_bars.py [--out test_out/vod_model/data_bars] [--model full_v7] [--per-recording 6]
       [--part K --parts N]
"""
import argparse
import json
import math
import random
import subprocess
import sys
from pathlib import Path

import numpy as np

import build_data
import hand_crops
import infer
import old_review

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import aimview_tools  # noqa: E402
from local_config import folder  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
REVIEWS = folder("data") / "vod_model" / "data_mined" / "reviews"
BOX_THRESHOLD = 0.2                     # the model's boxes looked at: under the review's threshold, to find the bars
                                        # it nearly boxes
TARGET_SCORE = 0.5                      # another box in a crop scoring under this may not be a target: no crop
BAR_AREA_SHARE = 0.5                    # a bar's box has less than this share of its bot's box's area
BAR_ABOVE = (0.4, 2.0)                  # the bar's center above the bot's, in bot heights
BOT_COLOR_DIFF = 40                     # a pixel within this of the bot's middle color (each channel) is the bot's
BOT_LIKE_SHARE = 0.1                    # a bar has at most this share of the bot's color
DISTINCT_SHARE = 0.4                    # a bar has at least this share neither the wall's color nor the bot's
WIDE_BAR = 2.5                          # a box this many times wider than tall covers a bar's whole strip
STRIP_SHARE = 0.5                       # else at least this share beside it is the strip's (the rest of the bar)
MIN_REACH_PX = 3                        # how far beside the box the strip is looked for, at least
MIN_GAP_PX = 4                          # the wall's color is read at least this far above the box
CANDIDATE_SPACING_S = 0.25              # frames looked at, at least this far apart
MAX_CANDIDATES = 40                     # frames decoded a recording at most, picked at random
SPACING_S = 0.5                         # crops of a recording, at least this far apart


def above(bar, bot):
    """Whether box `bar` sits where a health bar sits over box `bot`: boxes (cx, cy, w, h) in pixels, y down."""
    rise = bot[1] - bar[1]
    return (bar[2] * bar[3] < BAR_AREA_SHARE * bot[2] * bot[3] and abs(bar[0] - bot[0]) < bot[2]
            and BAR_ABOVE[0] * bot[3] < rise < BAR_ABOVE[1] * bot[3])


def pixels(rgb, box, inset=0.0):
    """The pixels of a box (cx, cy, w, h), each side moved in by `inset` of the box's size, as (n, 3) floats."""
    cx, cy, width, height = box[:4]
    half_w, half_h = max(1.0, width * (0.5 - inset)), max(1.0, height * (0.5 - inset))
    x0, x1 = max(0, int(round(cx - half_w))), min(rgb.shape[1], int(round(cx + half_w)) + 1)
    y0, y1 = max(0, int(round(cy - half_h))), min(rgb.shape[0], int(round(cy + half_h)) + 1)
    return rgb[y0:y1, x0:x1].reshape(-1, 3).astype(float)


def is_bar(rgb, bar, bot):
    """Whether box `bar` is a health bar over box `bot`: above it, its pixels neither the bot's color nor the wall's,
    on a strip running sideways. The fill may be any color, white and gray too: KovaaK's lets the player pick it."""
    if not above(bar, bot):
        return False
    color = np.median(pixels(rgb, bot, inset=0.25), 0)
    wall = wall_color(rgb, bar)
    patch = pixels(rgb, bar)
    bot_like = np.mean(np.abs(patch - color).max(1) < BOT_COLOR_DIFF)
    distinct = np.mean(unlike(patch, wall, color))
    return bot_like <= BOT_LIKE_SHARE and distinct >= DISTINCT_SHARE and runs_sideways(rgb, bar, color, wall)


def unlike(colors, wall, bot_color):
    """Which of the (n, 3) colors differ from both the wall's and the bot's (by more than BOT_COLOR_DIFF in a
    channel)."""
    return (np.abs(colors - wall).max(1) > BOT_COLOR_DIFF) & (np.abs(colors - bot_color).max(1) > BOT_COLOR_DIFF)


def wall_color(rgb, bar):
    """The wall's color round a box: the median of a band a box height above it, as wide as the box and a box width
    either side."""
    cx, cy, width, height = bar[:4]
    x0, x1 = int(round(cx - width / 2)), int(round(cx + width / 2))
    reach, gap = max(int(round(width)), MIN_REACH_PX), max(int(round(height)), MIN_GAP_PX)
    top = max(0, int(cy - height / 2) - 2 * gap)
    wall_area = rgb[top:max(top + 1, int(cy - height / 2) - gap), max(0, x0 - reach):x1 + reach]
    return np.median(wall_area.reshape(-1, 3).astype(float), 0)


def runs_sideways(rgb, bar, bot_color, wall):
    """Whether a box is part of a horizontal strip, as a bar's fill is: the box is wide, or beside it (one box width
    left or right, in its middle rows) STRIP_SHARE of the pixels are neither the wall's color nor the bot's. A small
    colored target beside a bot has the wall on both sides."""
    cx, cy, width, height = bar[:4]
    if width >= WIDE_BAR * height:
        return True
    x0, x1 = int(round(cx - width / 2)), int(round(cx + width / 2))
    reach = max(int(round(width)), MIN_REACH_PX)
    rows = slice(max(0, int(cy - height / 4)), int(cy + height / 4) + 1)
    for cols in (slice(max(0, x0 - reach), max(0, x0 - 1)), slice(x1 + 2, x1 + 1 + reach)):
        side = rgb[rows, cols].reshape(-1, 3).astype(float)
        if len(side) and unlike(side, wall, bot_color).mean() >= STRIP_SHARE:
            return True
    return False


def candidates(tracks):
    """Frames of the review whose tracks hold a box above a bigger one (degrees, y up), CANDIDATE_SPACING_S apart."""
    found = []
    for frame in tracks["frames"]:
        spots = [(x, -y, w, h) for (_, x, y), (w, h) in zip(frame["t"], frame.get("wh") or [])]
        if any(above(a, b) for a in spots for b in spots if a is not b):
            if not found or frame["i"] - found[-1] >= CANDIDATE_SPACING_S * tracks["fps"]:
                found.append(frame["i"])
    return found


def decode(video, frames):
    """The frames numbered `frames` (RGB, 1280 x 720), decoded from the start in one pass: {frame: rgb}."""
    picks = "+".join(f"eq(n\\,{n})" for n in frames)
    width, height = old_review.W, old_review.H
    raw = subprocess.run(["ffmpeg", "-v", "error", "-i", video, "-vf",
                          f"select='{picks}',scale={width}:{height}:flags=area,format=rgb24", "-fps_mode", "passthrough",
                          "-f", "rawvideo", "-"], capture_output=True).stdout
    size = width * height * 3
    images = [np.frombuffer(raw[i:i + size], np.uint8).reshape(height, width, 3) for i in range(0, len(raw) - size + 1, size)]
    return dict(zip(frames, images)) if len(images) == len(frames) else {}


def crop_of(rgb, boxes, bar_index, rnd):
    """The crop round one bar: (x0, y0, the targets in crop pixels, their scores, the bar in crop pixels), or None
    when a box there is unsure, a second bar, or crosses the crop's edge."""
    bar = boxes[bar_index]
    shift = hand_crops.JITTER_PX
    x0 = int(np.clip(bar[0] - hand_crops.CROP / 2 + rnd.randint(-shift, shift), 0, rgb.shape[1] - hand_crops.CROP))
    y0 = int(np.clip(bar[1] - hand_crops.CROP / 2 + rnd.randint(-shift, shift), 0, rgb.shape[0] - hand_crops.CROP))
    targets, scores = [], []
    for k, (cx, cy, width, height, score) in enumerate(boxes):
        inside = x0 <= cx < x0 + hand_crops.CROP and y0 <= cy < y0 + hand_crops.CROP
        touches = (cx + width / 2 > x0 and cx - width / 2 < x0 + hand_crops.CROP
                   and cy + height / 2 > y0 and cy - height / 2 < y0 + hand_crops.CROP)
        if k == bar_index or not touches:
            continue
        crosses = (cx - width / 2 < x0 or cx + width / 2 > x0 + hand_crops.CROP
                   or cy - height / 2 < y0 or cy + height / 2 > y0 + hand_crops.CROP)
        if not inside or crosses or score < TARGET_SCORE or any(is_bar(rgb, boxes[k], bot) for bot in boxes):
            return None
        targets.append([cx - x0, cy - y0, width, height])
        scores.append(score)
    fix = [bar[0] - x0, bar[1] - y0, bar[2], bar[3]]
    return x0, y0, np.array(targets, np.float32).reshape(-1, 4), np.array(scores, np.float32), np.array(fix, np.float32)


def save(out, name, rgb, fixed, crop, frame, score):
    """Writes the crop crop_of gave as <out>/<name>.npz, the bar's box as `fix` and its `score` after the targets'
    scores."""
    x0, y0, targets, scores, fix = crop
    window = (slice(y0, y0 + hand_crops.CROP), slice(x0, x0 + hand_crops.CROP))
    np.savez_compressed(out / f"{name}.npz", rgb=rgb[window].copy(), fixed=fixed[window], tmask=hand_crops.disc_mask(targets),
                        boxes=targets, hidden=np.uint8(0), scores=np.append(scores, score).astype(np.float32),
                        mined="false_bar", fix=fix, frame=np.int32(frame),
                        why=f"a health bar the model boxes at {score:.2f}")


def recording_crops(job, tracks, detector, out, args):
    """Writes one recording's crops into its split folder of `out` and gives how many. Only a frame with exactly one
    bar gives a crop."""
    rnd = random.Random(job["stem"])
    frames = candidates(tracks)
    if not frames:
        return 0
    frames = sorted(rnd.sample(frames, min(MAX_CANDIDATES, len(frames))))
    images = decode(job["video"], frames)
    if not images:
        return 0
    fixed = old_review.fixed_map(list(old_review._frames(job["video"], keyframes=True))).astype(np.uint8)
    split = build_data.split_of(job["folder"])
    (out / split).mkdir(parents=True, exist_ok=True)
    written, last = 0, -math.inf
    for frame, rgb in images.items():
        if written >= args.per_recording or frame - last < SPACING_S * tracks["fps"]:
            continue
        boxes = [tuple(map(float, box)) for box in detector(rgb, fixed, threshold=BOX_THRESHOLD)]
        bars = [k for k, box in enumerate(boxes) if any(is_bar(rgb, box, bot) for bot in boxes if bot is not box)]
        crop = crop_of(rgb, boxes, bars[0], rnd) if len(bars) == 1 else None
        if crop is None:
            continue
        name = f"{hand_crops.crop_name(job['video'])}_f{frame:05d}".replace(" ", "_")
        save(out / split, name, rgb, fixed, crop, frame, boxes[bars[0]][4])
        written, last = written + 1, frame
    return written


def main():
    """Writes the crops of this part's recordings, then the manifest of every recording with crops in --out."""
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--out", type=Path, default=folder("data") / "vod_model" / "data_bars")
    parser.add_argument("--model", default="full_v7")
    parser.add_argument("--per-recording", type=int, default=6)
    parser.add_argument("--part", type=int, default=0)
    parser.add_argument("--parts", type=int, default=1)
    args = parser.parse_args()
    held = build_data.check_runs(aimview_tools.Library())
    detector = infer.TorchDetector(str(Path(__file__).resolve().parent / "exports" / f"detector_{args.model}.pt"))
    args.out.mkdir(parents=True, exist_ok=True)
    jobs = [json.loads((review / "job.json").read_text(encoding="utf-8")) for review in sorted(REVIEWS.iterdir())]
    total = 0
    for job in jobs[args.part::args.parts]:
        stem = hand_crops.crop_name(job["video"]).replace(" ", "_")
        review = REVIEWS / job["stem"]
        done = any(args.out.glob(f"*/{glob_escape(stem)}_f*.npz"))
        if (job["folder"], job["file"]) in held or not (review / "tracks.json").exists() or done:
            continue
        count = recording_crops(job, json.loads((review / "tracks.json").read_text(encoding="utf-8")), detector,
                                args.out, args)
        if count:
            print(f"{job['file']}: {count} crops", flush=True)
        total += count
    manifest = [{"stem": stem, "folder": job["folder"], "kind": job["kind"], "video": job["video"]}
                for job in jobs for stem in [hand_crops.crop_name(job["video"]).replace(" ", "_")]
                if any(args.out.glob(f"*/{glob_escape(stem)}_f*.npz"))]
    (args.out / "manifest.jsonl").write_text("".join(json.dumps(row) + "\n" for row in manifest), encoding="utf-8")
    print(f"{total} crops written; {len(manifest)} recordings with crops in {args.out}")


def glob_escape(name):
    """A name with glob's special characters ([, ], *, ?) taken literally."""
    return "".join(f"[{char}]" if char in "[]*?" else char for char in name)


if __name__ == "__main__":
    main()
