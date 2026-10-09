"""Crops of overlapping targets for the user to check on the Crops page (crop_check/README.md), each target with its own
box. large_v15e4 failed the gate on Bounce 180 Sparky Jumbo by drawing one wide box over two overlapping spheres, and
build_auto_labels.py refuses a part that is two overlapping targets, so no overlapping pair had a label. Nothing here
goes into training data; the crops are only for review.

In: the library's sphere scenarios (hitbox spheroid, width over height within SPHERE_RATIO: a round
target) with a stats file, outside the gate's runs and build_auto_labels.LEFT_OUT (build_label_batch.candidates). First
the recordings build_label_batch.py already reviewed (--cached: its reviews, no GPU), then up to --more recordings of
scenarios whose names hold one of PAIR_WORDS (one per scenario before a second, seeded), reviewed the same way
(build_label_batch.reviewed: aimview-tool review) into <out>/reviews/<model>/.

A recording's candidates, in its run window (build_label_batch.run_window), from its tracks: a model box MERGE_ASPECT
times as long as it is wide or more against a single target's shape, both as it is and with the target's smear (its
move in one frame) taken off, or two model boxes whose IoU is PAIR_IOU or more; all inside the frame and clear of the
overlays the review leaves out. A single sphere's shape at a place is an ellipse: the picture's perspective stretches
it along the line from the screen's center (1 / cos of its angle off the view's axis, sphere_extent), and the
recording may be stretched wider (its stretch: the median box against that shape; many players stretch 4:3 to 16:9).
Checked on the sheet (2026-10-09): without these, nearly every wide box was one sphere near the screen's edge or moving
fast, or an overlay's edge. At most PER_RECORDING a recording, MIN_GAP_S or more apart and spread over the run (the
farthest from those picked first): up to MERGED_FIRST merged-looking boxes, then pairs, then merged-looking ones. Every
candidate is cropped and labelled (below); at most --crops are kept, by crop_order: the split ones first, then the pairs
left whole, then merged-looking boxes left whole (one peak), then those with no part of the target's color; within each,
every recording's first candidate before its second, merged-looking before pairs. The sheets showed most merged-looking
boxes left whole are one sphere drawn wide (an animation, it seems) or a false box, so they come after the pairs.

Each candidate gives a 256 x 256 crop centered on it, shifted up to JITTER_PX at random. Its boxes: the targets' parts
by the recording's color (build_label_batch.learned_color; without one, the median color in the candidate boxes' cores
against the crop's median as the wall); the part under the candidate split at its peaks of the distance to its edge (as
build_auto_labels.two_targets finds them, a column counting 1 / stretch of a row). Two peaks or more behind a saddle
(each MIN_SIDE_PX across or more) give one box a peak (centered on it, twice the peak's distance plus 1 px across its
short axis, shaped as a sphere there) in place of the model boxes on that part, why "split"; else the model's boxes
stay, why "unsplit" and the reason. The crop's other model boxes stay as they are.

Out, in <out>:
- crops/: the crops, saved like build_label_batch.py's (rgb, fixed, tmask, boxes, scores, mined, frame, why, sources,
  model_boxes, model_scores, pixel_boxes: the split boxes) with candidate ("merged" or "pair"), manifest.jsonl (stem,
  folder, kind, scenario, video) for make_page.py, and sheet.png: the first SHEET_CROPS crops (boxes green, model
  boxes blue).
- summary.json: the recordings (cached or newly reviewed), their candidates and crops, and the crops by why.
Last, crop_check/make_page.py adds the set merged_pairs to the page folder (--page).

Usage: python python/model/build_merged_pairs.py <out> [--model large_v13e4] [--cached <label batch folder>]
       [--more 30] [--crops 120] [--seed 0] [--page <folder>]
"""
import argparse
import json
import random
import re
import subprocess
import sys
from collections import Counter
from itertools import combinations
from pathlib import Path
from types import SimpleNamespace

import numpy as np
from scipy import ndimage

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
import aimview_tools  # noqa: E402
import build_auto_labels as auto  # noqa: E402  the pixel rules
import build_label_batch as batch  # noqa: E402  the recordings, their reviews and their target's color
import build_mined  # noqa: E402
import crop_batch  # noqa: E402
from crop_batch import boxes_px  # noqa: E402
import eval_video_alone  # noqa: E402
import infer  # noqa: E402
import label_score  # noqa: E402
import old_review  # noqa: E402
import teacher_label  # noqa: E402
from local_config import folder  # noqa: E402

CROP, WIDTH, HEIGHT = build_mined.CROP, build_mined.WIDTH, build_mined.HEIGHT
SPHERE_RATIO = (0.85, 1.18)     # a sphere scenario's hitbox width over height lies in this range
# scenario names likely to put targets over each other
PAIR_WORDS = re.compile(r"jumbo|bounce|big|huge|large|fireworks|cluster|bunch|multi|first")
MERGE_ASPECT = 1.5              # a box this many times as long as it is wide may be two targets in one
PAIR_IOU = 0.15                 # two model boxes overlapping this much (IoU) are a pair
MIN_SIDE_PX = 8                 # a merged-looking box's or a split box's short side is at least this (smaller: a
                                # small box's steps, a health bar's ends; the smallest pair on the sheet was 8.3)
EDGE_PX = 2                     # a candidate's boxes lie this far inside the frame or more (no target cut by the edge)
MIN_SPLIT = 2                   # a part splits into this many targets or more, or stays whole
PER_RECORDING = 5               # candidates a recording gives at most
MERGED_FIRST = 3                # of which merged-looking boxes take at most this many before pairs (the sheets showed
                                # most merged-looking boxes are one target, and pairs real overlaps)
MIN_GAP_S = 1.0                 # a recording's candidates are at least this far apart (s)
JITTER_PX = 32                  # a crop's center moves up to this far from the candidate's at random
LOOK_BACK = auto.LOOK_BACK      # frames before a kill its target's color is looked for in
COLOR_KILLS = batch.COLOR_KILLS  # the stats file's kills (at most) the target's color is learned at
PIXEL_SCORE = batch.PIXEL_SCORE  # the score a split box shows on the page
SHEET_CROPS, SHEET_COLUMNS = 40, 8
SET_NAME, SET_TITLE = "merged_pairs", "Overlapping targets: each one its own box"
KIND_ORDER = ("merged", "pair")  # merged-looking boxes are picked before pairs
UNSPLIT_ONE_PEAK = "unsplit: one peak"


def is_sphere(facts):
    """Whether a scenario's targets are spheres: a spheroid hitbox about as wide as it is tall."""
    hitbox = facts.get("hitbox") or {}
    return hitbox.get("kind") == "spheroid" and SPHERE_RATIO[0] <= (hitbox.get("widthToHeight") or 0) <= SPHERE_RATIO[1]


def pick_recordings(lib, args, rnd):
    """[(video, stats file, scenario, kind, the folder its reviews are kept under)]: every sphere recording with a
    review in --cached, then up to --more with PAIR_WORDS in their scenario's name (the first of each scenario before
    a second, seeded)."""
    facts = lib.scenarios()
    found = {scenario: recordings for scenario, recordings in batch.candidates(lib).items()
             if is_sphere(facts.get(scenario.lower(), {}))}
    cached, fresh = [], []
    for scenario in sorted(found):
        recordings = sorted(found[scenario])
        rnd.shuffle(recordings)
        kind = facts[scenario.lower()]["kind"]
        for turn, (video, stats) in enumerate(recordings):
            if (args.cached / "reviews" / args.model / video.stem / "tracks.json").is_file():
                cached.append((video, stats, scenario, kind, args.cached))
            elif PAIR_WORDS.search(scenario.lower()):
                fresh.append(((turn, rnd.random()), (video, stats, scenario, kind, args.out)))
    fresh = [pick for _, pick in sorted(fresh, key=lambda ranked: ranked[0])[:args.more]]
    print(f"{len(cached)} sphere recordings already reviewed, {len(fresh)} to review", flush=True)
    return cached + fresh


def inside_frame(box):
    """Whether a box (cx, cy, w, h in frame pixels) lies EDGE_PX or more inside the frame and clear of the overlays
    the review leaves out (eval_video_alone.AREAS: a box reaching into one is cut by it, or is the overlay's edge)."""
    x0, y0, x1, y1 = (box[0] - box[2] / 2 - EDGE_PX, box[1] - box[3] / 2 - EDGE_PX,
                      box[0] + box[2] / 2 + EDGE_PX, box[1] + box[3] / 2 + EDGE_PX)
    if x0 < 0 or y0 < 0 or x1 > WIDTH or y1 > HEIGHT:
        return False
    return not any(x0 < ax1 * WIDTH and ax0 * WIDTH < x1 and y0 < ay1 * HEIGHT and ay0 * HEIGHT < y1
                   for ax0, ay0, ax1, ay1, _ in eval_video_alone.AREAS)


def smear_px(frames, frame, track):
    """How far the track `track` moves on the screen in one frame about `frame` (x, y in pixels, unsigned): half its
    move from the frame before to the frame after, else its move to the one it has; (0, 0) when neither has it. A
    moving target is smeared that far in a recording, so its box is that much longer."""
    def place(index):
        """The track's place (frame pixels) in frame `index`, None when it is not there."""
        if not 0 <= index < len(frames):
            return None
        return next((old_review.to_px(x, y) for found, x, y in frames[index]["t"] if found == track), None)
    before, here, after = place(frame - 1), place(frame), place(frame + 1)
    if before is not None and after is not None:
        return abs(after[0] - before[0]) / 2, abs(after[1] - before[1]) / 2
    other = before if before is not None else after
    return (0.0, 0.0) if other is None else (abs(here[0] - other[0]), abs(here[1] - other[1]))


def sphere_extent(x, y):
    """How many times a sphere's radius its box reaches across and down at (x, y) (frame pixels), as (across, down):
    the picture's perspective stretches a sphere off the view's axis into an ellipse, 1 / cos of its angle off the
    axis longer along the line from the screen's center (about 1.6 at the screen's left and right edges)."""
    u, v = (x - old_review.CX) / old_review.K, (y - old_review.CY) / old_review.K
    radial_sq = 1 + u * u + v * v                   # the ellipse's long axis over its short one, squared
    off_sq = u * u + v * v
    cos_sq, sin_sq = (u * u / off_sq, v * v / off_sq) if off_sq > 0 else (1.0, 0.0)
    return np.sqrt(radial_sq * cos_sq + sin_sq), np.sqrt(radial_sq * sin_sq + cos_sq)


def sphere_aspect(x, y):
    """A sphere's box width over its height at (x, y) (frame pixels) in a picture that is not stretched."""
    across, down = sphere_extent(x, y)
    return across / down


def recording_stretch(frames, window):
    """How much wider than tall the recording's picture is stretched: the median over the run window's boxes (a short
    side of MIN_SIDE_PX or more) of their width over height against a sphere's there (sphere_aspect), 1 without any.
    Many players stretch a 4:3 picture to 16:9, so every sphere shows about 1.33 times as wide."""
    ratios = [box[2] / box[3] / sphere_aspect(*box[:2]) for frame in range(window[0], window[1] + 1)
              for box in boxes_px(frames[frame]) if min(box[2:4]) >= MIN_SIDE_PX]
    return float(np.median(ratios)) if ratios else 1.0


def merged_elongation(box, smear, stretch):
    """How many times a box (cx, cy, w, h in frame pixels) is as long as wide against a single sphere's shape there
    (`stretch` times sphere_aspect), both as it is and with its target's smear (x, y in pixels) taken off: the smaller,
    or 1 when the two lie along different sides (a smear may be longer than the box shows it)."""
    if min(box[2:4]) <= 0:
        return 1.0
    expected = stretch * sphere_aspect(*box[:2])
    as_is = np.log(box[2] / box[3] / expected)
    unsmeared = np.log(max(1.0, box[2] - smear[0]) / max(1.0, box[3] - smear[1]) / expected)
    return float(np.exp(min(abs(as_is), abs(unsmeared)))) if as_is * unsmeared > 0 else 1.0


def frame_candidates(frames, frame, stretch):
    """A frame's best candidate of each kind: {kind: (strength, box indexes)}, the strength how many times a merged
    box is as long as wide (merged_elongation, its target's smear from smear_px), or a pair's IoU."""
    boxes, best = boxes_px(frames[frame]), {}
    for i, box in enumerate(boxes):
        elongation = merged_elongation(box, smear_px(frames, frame, frames[frame]["t"][i][0]), stretch)
        if min(box[2:4]) >= MIN_SIDE_PX and elongation >= MERGE_ASPECT and inside_frame(box):
            best["merged"] = max(best.get("merged", (0, ())), (elongation, (i,)))
    for i, j in combinations(range(len(boxes)), 2):
        overlap = label_score.iou(boxes[i][:4], boxes[j][:4])
        if overlap >= PAIR_IOU and inside_frame(boxes[i]) and inside_frame(boxes[j]):
            best["pair"] = max(best.get("pair", (0, ())), (overlap, (i, j)))
    return best


def spread(frames, taken, gap, count):
    """Frames of `frames` (sorted) added to `taken` until it holds `count`: each the farthest from those taken (the
    middle one first), none nearer than `gap` frames to one taken."""
    chosen = list(taken)
    while frames and len(chosen) < count:
        if not chosen:
            chosen.append(frames[len(frames) // 2])
            continue
        far = max(frames, key=lambda frame: min(abs(frame - other) for other in chosen))
        if min(abs(far - other) for other in chosen) < gap:
            break
        chosen.append(far)
    return chosen[len(taken):]


def recording_candidates(frames, window, fps, stretch):
    """A recording's candidates: [(kind, rank, frame, box indexes)], at most PER_RECORDING: up to MERGED_FIRST
    merged-looking ones, then pairs, then merged-looking ones again (`stretch`: the recording's, recording_stretch);
    and how many frames had one of each kind."""
    by_kind = {kind: {} for kind in KIND_ORDER}
    for frame in range(window[0], window[1] + 1):
        for kind, found in frame_candidates(frames, frame, stretch).items():
            by_kind[kind][frame] = found[1]
    picked, out = [], []
    for kind, up_to in (("merged", MERGED_FIRST), ("pair", PER_RECORDING), ("merged", PER_RECORDING)):
        added = spread(sorted(set(by_kind[kind]) - set(picked)), picked, MIN_GAP_S * fps, up_to)
        ranked = sum(candidate[0] == kind for candidate in out)
        out += [(kind, ranked + n, frame, by_kind[kind][frame]) for n, frame in enumerate(added)]
        picked += added
    return out, {kind: len(by_kind[kind]) for kind in KIND_ORDER}


def reviewed_recordings(lib, picks, args):
    """Each pick reviewed (or its review read) with its run window and candidates: a list of dicts."""
    recordings = []
    for video, stats, scenario, kind, root in picks:
        cached = root == args.cached
        review_args = SimpleNamespace(out=root, model=args.model, program=args.program)
        folder_of, frames, fps, offset = batch.reviewed(lib, video, stats, review_args)
        row = dict(stem=crop_batch.recording_stem(video), folder=video.parent.name, kind=kind,
                   scenario=scenario, video=str(video), stats=str(stats), cached=cached, frames=frames, fps=fps,
                   offset=offset, candidates=[], frames_with={})
        window = batch.run_window(folder_of, stats, fps, len(frames), offset)
        if window is None:
            row["reason"] = "no run window"
        else:
            row["window"] = window
            row["stretch"] = round(recording_stretch(frames, window), 3)
            row["candidates"], row["frames_with"] = recording_candidates(frames, window, fps, row["stretch"])
        print(f"{'cached' if cached else 'reviewed'} {video.name}: frames with a candidate {row['frames_with']}, "
              f"picked {len(row['candidates'])}, stretch {row.get('stretch')}", flush=True)
        recordings.append(row)
    return recordings


def crop_corner(union, rnd):
    """A crop's corner (x0, y0 in frame pixels): centered on the union of a candidate's boxes (x0, y0, x1, y1),
    shifted up to JITTER_PX at random unless that cuts the union off; inside the frame."""
    center = ((union[0] + union[2]) / 2, (union[1] + union[3]) / 2)
    for jitter in (JITTER_PX, 0):
        x0 = int(np.clip(center[0] - CROP // 2 + rnd.randint(-jitter, jitter), 0, WIDTH - CROP))
        y0 = int(np.clip(center[1] - CROP // 2 + rnd.randint(-jitter, jitter), 0, HEIGHT - CROP))
        if x0 <= union[0] and y0 <= union[1] and union[2] <= x0 + CROP and union[3] <= y0 + CROP:
            break
    return x0, y0


def core_color(rgb, crop_fixed, boxes):
    """The candidate's color (RGB) when the recording has none: the median of its boxes' core colors
    (label_score.box_color), or None when it does not stand out from the wall (the crop's median)."""
    colors = [color for color in (label_score.box_color(rgb, crop_fixed, box[:4]) for box in boxes)
              if color is not None]
    if not colors:
        return None
    color = np.median(np.array(colors), 0)
    wall = np.median(rgb.reshape(-1, 3).astype(np.float32), 0)
    return None if np.linalg.norm(color - wall) < auto.MIN_CONTRAST else color


def part_under(labelled, boxes):
    """The part's number most common in the boxes' cores (the middle half of each side), 0 when none is there."""
    numbers = []
    for cx, cy, width, height in (box[:4] for box in boxes):
        x0, x1 = int(cx - width / 4), int(np.ceil(cx + width / 4))
        y0, y1 = int(cy - height / 4), int(np.ceil(cy + height / 4))
        core = labelled[max(0, y0):max(0, y1), max(0, x0):max(0, x1)]
        numbers += core[core > 0].tolist()
    return Counter(numbers).most_common(1)[0][0] if numbers else 0


def apart(distance, first, second, stretch):
    """Whether two peaks (row, column) of the distance to a part's edge are two targets, as
    build_auto_labels.two_targets judges: farther apart than the larger distance, and the distance on the line between
    them dips under SADDLE_SHARE of the smaller. Distances are in rows, a column counting 1 / `stretch` of one."""
    gap = np.hypot(second[0] - first[0], (second[1] - first[1]) / stretch)
    high, low = sorted((distance[tuple(first)], distance[tuple(second)]), reverse=True)
    if gap <= high:
        return False
    steps = np.linspace(0, 1, int(np.hypot(*(second - first))) + 2)[:, None]
    line = np.round(first + steps * (second - first)).astype(int)
    return distance[line[:, 0], line[:, 1]].min() < auto.SADDLE_SHARE * low


def split_peaks(mask, stretch):
    """A part's targets: [(row, column, distance in rows)] at its peaks of the distance to its edge (PEAK_SHARE of the
    highest or more, the greatest within PEAK_PX), the highest first, each kept when it is `apart` from every one kept
    before. A column counts 1 / `stretch` of a row (a single target's width over height), so a stretched sphere's
    ellipse measures as a circle. No bot head rule: the targets here are spheres."""
    distance = ndimage.distance_transform_edt(mask, sampling=(1.0, 1.0 / stretch))
    top = distance.max()
    peaks = np.argwhere((distance == ndimage.maximum_filter(distance, size=2 * auto.PEAK_PX + 1))
                        & (distance >= auto.PEAK_SHARE * top))
    kept = []
    for peak in sorted(peaks, key=lambda peak: -distance[tuple(peak)]):
        if all(apart(distance, other, peak, stretch) for other in kept):
            kept.append(peak)
    return [(int(row), int(column), float(distance[row, column])) for row, column in kept]


def split_box(row, column, reach, corner, stretch):
    """The box (cx, cy, w, h in crop pixels) of a sphere whose peak of the distance to its part's edge is at (row,
    column) of the crop at `corner` (frame pixels), `reach` rows from the edge: its short radius, as wide and tall as
    a sphere there (sphere_extent) in a picture stretched `stretch` times as wide."""
    across, down = sphere_extent(column + 0.5 + corner[0], row + 0.5 + corner[1])
    return [column + 0.5, row + 0.5, (2 * reach + 1) * across * stretch, (2 * reach + 1) * down]


def proposed(rgb, crop_fixed, color, model, chosen, place):
    """A crop's boxes: (boxes, scores, sources, split boxes, why). `model` holds the crop's model boxes (cx, cy, w, h,
    score in crop pixels), `chosen` the candidate's indexes in it, `color` the recording's target color or None,
    `place` the crop's corner (frame pixels) and the recording's stretch (split_box)."""
    corner, stretch = place
    keep = ([box[:4] for box in model], [box[4] for box in model], ["model"] * len(model), [])
    color = core_color(rgb, crop_fixed, [model[k] for k in chosen]) if color is None else color
    if color is None:
        return (*keep, "unsplit: no target color")
    labelled, _ = auto.parts(rgb, crop_fixed, None, color)
    number = part_under(labelled, [model[k] for k in chosen])
    if number == 0:
        return (*keep, "unsplit: no part of the target's color under the box")
    mask = labelled == number
    peaks = [peak for peak in split_peaks(mask, stretch) if 2 * peak[2] + 1 >= MIN_SIDE_PX]
    if len(peaks) < MIN_SPLIT:
        return (*keep, UNSPLIT_ONE_PEAK)
    split = [split_box(row, column, reach, corner, stretch) for row, column, reach in peaks]
    on_part = set(chosen) | {k for k, box in enumerate(model)
                             if mask[int(np.clip(box[1], 0, CROP - 1)), int(np.clip(box[0], 0, CROP - 1))]}
    others = [k for k in range(len(model)) if k not in on_part]
    return (split + [model[k][:4] for k in others], [PIXEL_SCORE] * len(split) + [model[k][4] for k in others],
            ["pixels"] * len(split) + ["model"] * len(others), split,
            f"split: {len(split)} peaks in place of {len(on_part)} model boxes")


def color_kills(row, rnd):
    """Up to COLOR_KILLS of the stats file's kills (frames, seeded) the target's color is learned at, as
    build_label_batch.py picks them."""
    if row["offset"] is None:
        return []
    truth = [kill for kill in eval_video_alone.truth_frames(Path(row["stats"]), row["offset"], row["fps"],
                                                            len(row["frames"])) if kill > max(LOOK_BACK)]
    return sorted(rnd.sample(truth, min(COLOR_KILLS, len(truth))))


def crops_of(row, rnd):
    """One recording's candidates cropped and labelled (crop_candidate), in memory."""
    video = Path(row["video"])
    kills = color_kills(row, rnd)
    decoded = build_mined.decode(video, sorted({frame for _, _, frame, _ in row["candidates"]}
                                               | {kill - back for kill in kills for back in LOOK_BACK}))
    fixed = crop_batch.fixed_mask(video, bool)
    color, source, count = batch.learned_color(video, row["frames"], row["window"], kills, decoded, fixed)
    row.update(color_source=source, color_count=count, color=None if color is None else color.round(1).tolist())
    crops = [crop_candidate(decoded[candidate[2]], fixed, row, candidate, color, rnd)
             for candidate in row["candidates"] if candidate[2] in decoded]
    print(f"{video.name}: color from {count} {source}; "
          f"{dict(Counter(crop['why'].split(':')[0] for crop in crops))}", flush=True)
    return crops


def crop_candidate(frame_rgb, fixed, row, candidate, color, rnd):
    """One candidate cropped and labelled: {name, kind, rank, why, arrays} (the arrays as the crop file holds them)."""
    kind, rank, frame, indexes = candidate
    boxes = boxes_px(row["frames"][frame])
    union = (min(boxes[i][0] - boxes[i][2] / 2 for i in indexes), min(boxes[i][1] - boxes[i][3] / 2 for i in indexes),
             max(boxes[i][0] + boxes[i][2] / 2 for i in indexes), max(boxes[i][1] + boxes[i][3] / 2 for i in indexes))
    x0, y0 = crop_corner(union, rnd)
    rgb, crop_fixed = frame_rgb[y0:y0 + CROP, x0:x0 + CROP], fixed[y0:y0 + CROP, x0:x0 + CROP]
    in_crop = [i for i, box in enumerate(boxes) if x0 <= box[0] < x0 + CROP and y0 <= box[1] < y0 + CROP]
    model = [(boxes[i][0] - x0, boxes[i][1] - y0, *boxes[i][2:]) for i in in_crop]
    chosen = [in_crop.index(i) for i in indexes]
    labels, scores, sources, split, why = proposed(rgb, crop_fixed, color, model, chosen, ((x0, y0), row["stretch"]))
    labels = np.array(labels, np.float32).reshape(-1, 4)
    arrays = crop_batch.crop_arrays(rgb, crop_fixed.astype(np.uint8), teacher_label.pill_mask(labels, (CROP, CROP)),
                                    labels, scores, frame, why, mined=SET_NAME,
                                    sources=np.array(sources, dtype=np.str_), candidate=np.str_(kind),
                                    model_boxes=np.array([box[:4] for box in model], np.float32).reshape(-1, 4),
                                    model_scores=np.array([box[4] for box in model], np.float32),
                                    pixel_boxes=np.array(split, np.float32).reshape(-1, 4))
    return dict(name=f"{row['stem']}_{frame:05d}.npz", stem=row["stem"], kind=kind, rank=rank, why=why,
                arrays=arrays)


def crop_order(crop):
    """Where a crop stands in the keeping order: split crops first, then pairs left whole (two model boxes over each
    other: overlapping targets, or one target boxed twice), then merged-looking boxes with one peak (mostly a single
    sphere squashed by its death or spawn), then those without a part of the target's color (mostly no target)."""
    if crop["why"].startswith("split"):
        return 0
    if crop["kind"] == "pair":
        return 1
    return 2 if crop["why"] == UNSPLIT_ONE_PEAK else 3


def keep_crops(crops, count, out, rnd):
    """Saves at most `count` crops in <out>/crops by crop_order, then each recording's first candidate before its
    second, merged-looking before pairs (ties seeded); gives the counts kept and left by kind and why."""
    ranked = sorted(crops, key=lambda crop: (crop_order(crop), crop["rank"], KIND_ORDER.index(crop["kind"]),
                                             rnd.random()))
    (out / "crops").mkdir(parents=True, exist_ok=True)
    totals = Counter()
    for n, crop in enumerate(ranked):
        kept = n < count
        if kept:
            np.savez_compressed(out / "crops" / crop["name"], **crop["arrays"])
        totals[f"{'kept' if kept else 'left'} {crop['kind']} {crop['why']}"] += 1
    return totals


def draw_sheet(files, path):
    """The crops at full size, SHEET_COLUMNS a row: model boxes blue, then the crop's boxes green over them, and its
    why's tag (S split, U unsplit) in the corner."""
    crop_batch.draw_sheet(files, path, [crop_batch.boxes_layer("model_boxes", "blue"),
                                        crop_batch.boxes_layer("boxes", "lime"), why_tag], SHEET_COLUMNS)


def why_tag(pen, crop, file, scale):
    """A sheet layer: the crop's why's tag (S split, U unsplit), its candidate's kind's letter and its file's stem."""
    tag = "S" if str(crop["why"]).startswith("split") else "U"
    pen.text((4, 4), f"{tag} {str(crop['candidate'])[0]} {file.stem}", fill="yellow")


def write_outputs(recordings, totals, args):
    """The crops' manifest.jsonl and sheet.png, summary.json (`totals`: the crops kept and left by kind and why), and
    the page's set; gives the summary without the recordings."""
    crops = args.out / "crops"
    crops.mkdir(parents=True, exist_ok=True)
    crop_batch.write_manifest(crops / "manifest.jsonl", recordings, batch.MANIFEST_KEYS)
    saved = sorted(crops.glob("*.npz"))
    draw_sheet(saved[:SHEET_CROPS], crops / "sheet.png")
    for row in recordings:
        row["kept"] = sum(path.name.startswith(row["stem"]) for path in saved)
    summary = dict(model=args.model, cached=sum(row["cached"] for row in recordings),
                   reviewed=sum(not row["cached"] for row in recordings),
                   with_candidates=sum(bool(row["candidates"]) for row in recordings),
                   candidates=Counter(kind for row in recordings for kind, *_ in row["candidates"]),
                   crops=len(saved), totals=dict(sorted(totals.items())))
    listed = [{key: value for key, value in row.items() if key not in ("frames", "video", "stats")}
              for row in recordings]
    (args.out / "summary.json").write_text(json.dumps(dict(summary, recordings=listed), indent=1, default=str),
                                           encoding="utf-8")
    if any(crops.glob("*.npz")):
        subprocess.run([sys.executable, str(HERE / "crop_check" / "make_page.py"), str(args.page), SET_NAME,
                        str(crops), "--title", SET_TITLE], check=True)
    return summary


def main():
    """Picks and reviews the recordings, finds their candidates, writes the crops, the sheet, summary.json and the
    page's set."""
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("out", type=Path)
    parser.add_argument("--model", default=infer.BEST)
    parser.add_argument("--cached", type=Path, default=folder("data") / "vod_model" / "label_batch_2026-10-08",
                        help="a build_label_batch.py out folder whose reviews are reused")
    parser.add_argument("--more", type=int, default=30, help="recordings with PAIR_WORDS to review besides")
    parser.add_argument("--crops", type=int, default=120, help="crops at most")
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--page", type=Path, default=folder("data") / "vod_model" / "check_merged_pairs")
    args = parser.parse_args()
    sys.stdout.reconfigure(encoding="utf-8")        # scenario names hold characters Windows' console page lacks
    rnd = random.Random(args.seed)
    lib = aimview_tools.Library()
    args.program = eval_video_alone.review_program()
    recordings = reviewed_recordings(lib, pick_recordings(lib, args, rnd), args)
    crops = [crop for row in recordings if row["candidates"] for crop in crops_of(row, rnd)]
    crop_batch.retire_old(args.out, ("crops",))
    totals = keep_crops(crops, args.crops, args.out, rnd)
    summary = write_outputs(recordings, totals, args)
    print(json.dumps(summary, indent=1, default=str))
    print(f"the page: {args.page}")


if __name__ == "__main__":
    main()
