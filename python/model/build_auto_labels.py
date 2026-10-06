"""Training crops labelled with no one drawing: the stats file is the label. At a kill KovaaK's stats file records, the
killed target is under the crosshair, and a target's color stands out from the wall's. So where the detector has no
box near the crosshair before a stats file's kill, the target it missed is found in the pixels.

Each recording (with a stats file, outside the gate's runs) is reviewed as the app reviews it (aimview-tool review, the
excluded areas eval_video_alone.py uses), kept in <out>/reviews/<model>/<stem>/. For each kill of the stats file with no
box within NEAR_DEG of the crosshair in the LOOK_BACK frames before it, the nearest of those frames whose pixels agree
gives a crop of 256 x 256 round the crosshair (shifted up to 48 px at random), labelled from its pixels (`label_crop`):
- the wall's color is the crop's median, the target's the median of the pixels next to the crosshair (the fixed map's
  pixels, the crosshair's and the HUD's, left out); it must differ from the wall's by MIN_CONTRAST;
- a pixel is the target's where it is nearer the target's color than the wall's and differs from the wall's by half
  MIN_CONTRAST; the fixed map's pixels next to such pixels join them (the crosshair drawn over a target), holes are
  filled (a sphere's highlight), and the connected parts taken;
- the part at the crosshair is the killed target: it must be a target's shape (SOLID_SHARE of its box filled, its sides
  within MAX_ASPECT and no wider than BAR_ASPECT times its height (a health bar lies flat), convex: CONVEX_SHARE of its hull filled, where two targets touching are not, and one target: its
  distance transform has no second peak behind a saddle, as two overlapping targets have and a capsule's even ridge
  has not, and no notch as deep as one of unlike size leaves: `notched`), stay off the crop's edge and be MIN_AREA_PX
  or more; else the next frame back is tried;
- every other part of a target's shape, off the crop's edge and no bigger than AREA_RANGE[1] times it (no smaller than
  AREA_RANGE[0] times it beside a bot, whose head may be a part of its own), is a target too; any other part, and a
  detector box over no part whose pixels stand out from the wall, is left to train.py as "ignore" (learnt neither as a
  target nor as wall); a line (LINE_FILL of its box) and a detector box that does not stand out are left as wall
  (`stands_out`).
Each kill's crop is paired with the same crop GONE_FRAMES after the kill (`gone_crop`), saved with the tag GONE_TAG:
the target gone, the crosshair on the wall, labelled with no target at the crosshair (no part of the killed target's
color within CROSSHAIR_REACH_PX of it, else none is saved) and the other targets by the same rules. Without the pairs
the crops taught that something at the crosshair is a target: two seeds of large_v14 boxed the crosshair itself while
the view turned 2.4 and 5.4 times as often as large_v13e4 on 1wall 6targets extra small (2026-10-06).
The boxes are the parts' extents and the target mask their pixels. It suits plain targets on a plain wall (tiles,
spheres): on a sweep of 40 other clicking recordings (2026-10-06) the detector missed almost no kill's target, and the
few crops it made held health bars and humanoid bots, which a color's parts cannot box as the detector must (a bot
whole, never its bar). Flow Fix's recordings are left out: its shots were deleted or delayed, so its stats files are
not the kills' truth. The crops are saved like build_kill_feedback.py's
(rgb, fixed, tmask, boxes, scores, mined, frame, why, ignore), all in train/ with TAG before their names (train.py
--repeat), with a manifest.jsonl and a sheet of every crop (sheet.png, and sheet_gone.png for the pairs: each target's outline,
its mask, in green; the boxes left out in yellow) to look over.

Usage: python python/model/build_auto_labels.py <out> <model> [--match tile] [--kinds static,dynamic,switching]
       [--recordings 40] [--per-recording 25] [--seed 0]
"""
import argparse
import hashlib
import json
import random
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw
from scipy import ndimage
from scipy.spatial import ConvexHull, QhullError

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
import aimview_tools  # noqa: E402
import build_data  # noqa: E402
import build_disagreements  # noqa: E402
import build_kill_feedback  # noqa: E402
import build_mined  # noqa: E402
import eval_video_alone  # noqa: E402
import old_review  # noqa: E402

CROP, JITTER_PX = build_mined.CROP, build_mined.JITTER_PX
WIDTH, HEIGHT = build_mined.WIDTH, build_mined.HEIGHT
TAG = "auto_kill_"              # 10 characters: train.py --repeat's key
GONE_TAG = "auto_gone_"
GONE_FRAMES = 8                 # frames after a kill its target has gone (the death's fade is about 6)
NEAR_DEG = 1.5                  # a box this near the crosshair before a kill: the detector saw the target
LOOK_BACK = (1, 2, 3, 4)        # frames before a kill its target is looked for in, nearest first
MIN_CONTRAST = 60.0             # RGB distance a target's color must have from the wall's
CROSSHAIR_REACH_PX = 3          # the target's color: the pixels this near the crosshair
MIN_COLOR_PX = 3                # of which at least this many off the fixed map
MIN_AREA_PX = 12
SOLID_SHARE = 0.5               # a target fills at least this share of its box (a sphere 0.79, a cube's face 1)
MAX_ASPECT = 4.0                # a target's box is at most this many times as long as it is wide
BAR_ASPECT = 1.8                # a part this many times wider than tall lies flat: a health bar, never a target
CONVEX_SHARE = 0.9              # a target's pixels against its pixel centers' hull (a cube or sphere over 1)
PEAK_SHARE = 0.6                # a second peak of the distance to the part's edge counts from this share of the first
SADDLE_SHARE = 0.8              # two peaks are two targets when the distance dips under this share of the lower between
PEAK_PX = 3                     # a peak is the greatest distance within this many pixels
HEAD_SLANT = 0.5                # a head is above its body within this run over rise
NOTCH_SHARE = 0.06              # two overlapping targets leave a notch this deep against the part's shorter side
MIN_NOTCH_PX = 3.0              # and at least this deep (a small target's pixel steps leave 1 to 2)
LINE_FILL = 0.2                 # a part filling less of its box is a line (a wall's edge, a panel's outline)
UPRIGHT_ASPECT = 2.2            # a part this many times taller than wide is a bot standing: its neck is no notch
AREA_RANGE = (0.2, 5.0)         # another target's area against the killed one's (the least only beside a bot)
STANDS_OUT_SHARE = 0.1          # a detector box may hold a target where this share of its pixels stands out
BRIDGE_PX = 2                   # fixed-map pixels this near the target's color join it
SHEET_COLUMNS, THUMB = 10, 128
LEFT_OUT = ("flow fix",)        # scenarios whose stats files are not the kills' truth


def missed_kills(lib, video, stats, args):
    """The review's frames and the stats file's kills with no box near the crosshair before them, or None when the
    stats file gives no clock offset."""
    folder = args.out / "reviews" / args.model / video.stem
    if not (folder / "tracks.json").is_file():
        lib.review_video(str(video), args.model, str(folder), stats=str(stats), areas=eval_video_alone.AREAS,
                         quiet=True)
    with_stats = eval_video_alone.core_review(args.program, folder, video, stats)
    offset = with_stats["summary"]["info"].get("offset")
    if with_stats.get("mode") == "track" or offset is None:
        return None
    frames = json.loads((folder / "tracks.json").read_bytes())["frames"]
    truth = eval_video_alone.truth_frames(stats, offset, with_stats["fps"], len(frames))

    def seen(kill):
        return any(np.hypot(x, y) < NEAR_DEG for back in LOOK_BACK for _, x, y in frames[kill - back]["t"])
    return frames, [kill for kill in truth if max(LOOK_BACK) <= kill < len(frames) and not seen(kill)]


def target_color(rgb, fixed, at):
    """The target's color round `at` (the crosshair in the crop), or None when it does not stand out from the wall's
    (the crop's median)."""
    x, y = at
    window = (slice(y - CROSSHAIR_REACH_PX, y + CROSSHAIR_REACH_PX + 1),
              slice(x - CROSSHAIR_REACH_PX, x + CROSSHAIR_REACH_PX + 1))
    free = ~fixed[window]
    if free.sum() < MIN_COLOR_PX:
        return None
    wall = np.median(rgb.reshape(-1, 3).astype(np.float32), 0)
    target = np.median(rgb[window][free].astype(np.float32), 0)
    return None if np.linalg.norm(target - wall) < MIN_CONTRAST else target


def parts(rgb, fixed, at, target=None):
    """The crop's parts of the target's color (labelled array, count): the color taken round `at` (the crosshair in
    the crop) unless given, None when it does not stand out from the wall's."""
    target = target_color(rgb, fixed, at) if target is None else target
    if target is None:
        return None
    wall = np.median(rgb.reshape(-1, 3).astype(np.float32), 0)
    pixels = rgb.astype(np.float32)
    from_wall = np.linalg.norm(pixels - wall, axis=2)
    colored = (np.linalg.norm(pixels - target, axis=2) < from_wall) & (from_wall > MIN_CONTRAST / 2) & ~fixed
    reach = np.ones((2 * BRIDGE_PX + 1, 2 * BRIDGE_PX + 1), bool)
    colored = ndimage.binary_fill_holes(colored | fixed & ndimage.binary_dilation(colored, reach))
    labelled, count = ndimage.label(colored)
    return labelled, count


def two_targets(mask):
    """Whether a part is two overlapping targets: its distance to the edge has a second peak (PEAK_SHARE of the first
    or more, farther from it than the first's distance) and dips between them under SADDLE_SHARE of the lower one. A
    sphere or a cube has one peak; a capsule's ridge has many, but no dip between them. A smaller peak straight above
    the first (within HEAD_SLANT of upright) is a bot's head on its body: one target."""
    distance = ndimage.distance_transform_edt(mask)
    top = distance.max()
    peaks = np.argwhere((distance == ndimage.maximum_filter(distance, size=2 * PEAK_PX + 1))
                        & (distance >= PEAK_SHARE * top))
    first = np.unravel_index(int(distance.argmax()), distance.shape)
    for second in peaks:
        gap = np.hypot(*(second - first))
        rise, across = first[0] - second[0], abs(second[1] - first[1])
        if gap <= top or (rise > 0 and across <= HEAD_SLANT * rise and distance[tuple(second)] < top):
            continue
        steps = np.linspace(0, 1, int(gap) + 2)[:, None]
        line = np.round(np.array(first) + steps * (second - np.array(first))).astype(int)
        lowest = distance[line[:, 0], line[:, 1]].min()
        if lowest < SADDLE_SHARE * min(top, distance[tuple(second)]):
            return True
    return False


def notched(mask, width, height):
    """Whether a part is two overlapping targets of unlike size, which `two_targets` misses (the smaller one gives no
    peak): the largest disc in its convex hull but off the part is NOTCH_SHARE of its shorter side or more. A single
    cube or sphere reaches 0.057 (its shading's edge), a pair 0.06 to 0.13 and 4 px or more (2026-10-07, 557
    auto-labelled parts), a target of 10 px 1 px (its pixel steps). Holes
    are filled first (the crosshair over a target); an upright part is left alone (a bot's neck)."""
    if height >= UPRIGHT_ASPECT * width:
        return False
    filled = ndimage.binary_fill_holes(mask)
    rows, columns = np.nonzero(filled)
    points = np.column_stack([columns, rows])
    try:
        corners = points[ConvexHull(points).vertices]
    except QhullError:                                  # a line of pixels
        return False
    hull = Image.new("1", filled.shape[::-1])
    ImageDraw.Draw(hull).polygon([tuple(corner) for corner in corners.tolist()], fill=1)
    notch = ndimage.distance_transform_edt(np.array(hull) & ~filled).max()
    return notch >= max(NOTCH_SHARE * min(width, height), MIN_NOTCH_PX)


def shape_of(mask):
    """A part's box (cx, cy, w, h in crop pixels), area, whether it has a target's shape, and whether it touches the
    crop's edge."""
    rows, columns = np.nonzero(mask)
    y0, y1, x0, x1 = rows.min(), rows.max() + 1, columns.min(), columns.max() + 1
    width, height, area = float(x1 - x0), float(y1 - y0), int(mask.sum())
    try:
        hull = ConvexHull(np.column_stack([columns, rows])).volume
    except QhullError:                                  # a line of pixels
        hull = 0.0
    solid = (area >= SOLID_SHARE * width * height and max(width, height) <= MAX_ASPECT * min(width, height)
             and width <= BAR_ASPECT * height and area >= CONVEX_SHARE * hull and not two_targets(mask)
             and not notched(mask, width, height))
    edge = x0 == 0 or y0 == 0 or x1 == CROP or y1 == CROP
    return [(x0 + x1) / 2, (y0 + y1) / 2, width, height], area, solid, edge


def label_crop(rgb, fixed, at, model_boxes):
    """A crop's target boxes, ignore boxes and target mask from its pixels (the module's rules), the killed target's
    color and area, or None when the part at the crosshair (`at`) is no target."""
    target = target_color(rgb, fixed, at)
    found = None if target is None else parts(rgb, fixed, at, target)
    if found is None:
        return None
    labelled, count = found
    under = labelled[at[1] - 1:at[1] + 2, at[0] - 1:at[0] + 2]
    numbers = np.unique(under[under > 0])
    if len(numbers) != 1:
        return None
    killed = labelled == numbers[0]
    box, area, solid, edge = shape_of(killed)
    if not solid or edge or area < MIN_AREA_PX:
        return None
    size = (area, box[3] >= UPRIGHT_ASPECT * box[2])
    boxes, ignore, mask = others(rgb, fixed, found, (numbers[0], *size), model_boxes)
    return [box, *boxes], ignore, (mask | killed).astype(np.uint8), target, size


def others(rgb, fixed, found, killed, model_boxes):
    """The parts (`found`: labelled array, count) but the killed target's as targets or ignored, their mask, and the
    detector's boxes over no part ignored. `killed` is the killed target's part number (0: none), area and whether it
    stands upright. A part is a target with a target's shape, off the crop's edge, and no bigger than AREA_RANGE[1]
    times the killed target; beside an upright one (a bot) no smaller than AREA_RANGE[0] times it either, where a
    smaller part is its head. Beside a sphere or a cube a small part is a target too: shimPressure's spheres differ in
    size (2026-10-07: 13 small spheres were left out, and 2 bots' heads would have been taken). A line (LINE_FILL) is
    left as wall, not ignored: train.py learns nothing in an ignore box, and a panel's outline blanked a whole crop."""
    (labelled, count), (killed_number, area, upright) = found, killed
    smallest = AREA_RANGE[0] * area if upright else 0.0
    boxes, ignore, mask = [], [], np.zeros(labelled.shape, bool)
    for number in range(1, count + 1):
        part = labelled == number
        if number == killed_number or part.sum() < MIN_AREA_PX:
            continue
        other, other_area, other_solid, other_edge = shape_of(part)
        if other_solid and not other_edge and smallest <= other_area <= AREA_RANGE[1] * area:
            boxes.append(other)
            mask |= part
        elif other_area >= LINE_FILL * other[2] * other[3]:
            ignore.append(other)
    wall = np.median(rgb.reshape(-1, 3).astype(np.float32), 0)
    for box in model_boxes:
        if not labelled[int(np.clip(box[1], 0, CROP - 1)), int(np.clip(box[0], 0, CROP - 1))]                 and stands_out(rgb, fixed, box, wall):
            ignore.append(list(box))
    return boxes, ignore, mask


def stands_out(rgb, fixed, box, wall):
    """Whether a detector box (cx, cy, w, h in crop pixels) may hold a target: STANDS_OUT_SHARE of its pixels off the
    fixed map differ from the wall's color by MIN_CONTRAST (a target's color stands out from the wall's), or too few of
    them to tell. One that does not is left as wall, not ignored: on the auto crops (2026-10-07) such boxes held
    Reactive Flick's purple rings and a wall panel's outline, none a target."""
    cx, cy, width, height = box
    x0, y0 = int(max(0, cx - width / 2)), int(max(0, cy - height / 2))
    x1, y1 = int(min(CROP, cx + width / 2 + 1)), int(min(CROP, cy + height / 2 + 1))
    free = ~fixed[y0:y1, x0:x1]
    if free.sum() < MIN_COLOR_PX:
        return True
    distance = np.linalg.norm(rgb[y0:y1, x0:x1][free].astype(np.float32) - wall, axis=1)
    return (distance >= MIN_CONTRAST).mean() >= STANDS_OUT_SHARE


def gone_crop(kill, place, frames, decoded, fixed):
    """The kill's crop GONE_FRAMES after it (the module's text), `place` its crop's (corner, crosshair, the killed
    target's color, its area and whether it stands upright): (frame, rgb, fixed, boxes, ignore, mask), or None when the
    killed target's color is still at the crosshair or the frame is missing."""
    later = kill + GONE_FRAMES
    if later not in decoded or later >= len(frames):
        return None
    (x0, y0), at, target, size = place
    window = (slice(y0, y0 + CROP), slice(x0, x0 + CROP))
    rgb, crop_fixed = decoded[later][window], fixed[window]
    found = parts(rgb, crop_fixed, at, target)
    labelled = found[0]
    near = labelled[at[1] - CROSSHAIR_REACH_PX:at[1] + CROSSHAIR_REACH_PX + 1,
                    at[0] - CROSSHAIR_REACH_PX:at[0] + CROSSHAIR_REACH_PX + 1]
    if near.any():
        return None
    model_boxes = [(box[0] - x0, box[1] - y0, box[2], box[3]) for box in build_disagreements.boxes_px(frames[later])
                   if x0 <= box[0] < x0 + CROP and y0 <= box[1] < y0 + CROP]
    boxes, ignore, mask = others(rgb, crop_fixed, found, (0, *size), model_boxes)
    return later, rgb, crop_fixed, boxes, ignore, mask.astype(np.uint8)


def corner_of(rnd):
    """A crop's corner round the crosshair, and the crosshair's place in the crop."""
    cx, cy = old_review.to_px(0, 0)
    x0 = int(np.clip(cx - CROP // 2 + rnd.randint(-JITTER_PX, JITTER_PX), 0, WIDTH - CROP))
    y0 = int(np.clip(cy - CROP // 2 + rnd.randint(-JITTER_PX, JITTER_PX), 0, HEIGHT - CROP))
    return (x0, y0), (int(cx) - x0, int(cy) - y0)


def crop_kill(kill, frames, decoded, fixed, rnd):
    """A kill's crop: (frame, rgb, fixed, boxes, ignore, mask) from the nearest frame before it whose pixels agree, or
    None."""
    (x0, y0), at = corner_of(rnd)
    window = (slice(y0, y0 + CROP), slice(x0, x0 + CROP))
    for back in LOOK_BACK:
        frame = kill - back
        if frame not in decoded:
            continue
        model_boxes = [(box[0] - x0, box[1] - y0, box[2], box[3]) for box in build_disagreements.boxes_px(frames[frame])
                       if x0 <= box[0] < x0 + CROP and y0 <= box[1] < y0 + CROP]
        rgb, crop_fixed = decoded[frame][window], fixed[window]
        labels = label_crop(rgb, crop_fixed, at, model_boxes)
        if labels is not None:
            boxes, ignore, mask, target, size = labels
            return (frame, rgb, crop_fixed, boxes, ignore, mask), ((x0, y0), at, target, size)
    return None


def crops_of(lib, video, stats, args, rnd):
    """One recording's crops (saved in train/); its manifest row."""
    found = missed_kills(lib, video, stats, args)
    if found is None:
        return dict(video=str(video), crops=0, reason="the stats file gives no clock offset")
    frames, missed = found
    picked = sorted(rnd.sample(missed, min(args.per_recording, len(missed))))
    wanted = {kill - back for kill in picked for back in LOOK_BACK} | {kill + GONE_FRAMES for kill in picked}
    decoded = build_mined.decode(video, sorted(frame for frame in wanted if 0 <= frame < len(frames)))
    fixed = old_review.fixed_map(build_data.keyframes(video, "yuv420p")).astype(bool)
    stem = hashlib.md5(video.name.encode()).hexdigest()[:10]
    (args.out / "train").mkdir(parents=True, exist_ok=True)
    written = gone = 0
    for kill in picked:
        found = crop_kill(kill, frames, decoded, fixed, rnd)
        if found is None:
            continue
        crop, place = found
        save_crop(args.out / "train" / f"{TAG}{stem}_{crop[0]:05d}.npz", crop,
                  f"auto: the stats file's kill at frame {kill}, the target found in the pixels")
        written += 1
        after = gone_crop(kill, place, frames, decoded, fixed)
        if after is not None:
            save_crop(args.out / "train" / f"{GONE_TAG}{stem}_{after[0]:05d}.npz", after,
                      f"auto: {GONE_FRAMES} frames after the stats file's kill at frame {kill}, its target gone")
            gone += 1
    print(f"{video.name}: {len(missed)} kills with no box near the crosshair; {written} of {len(picked)} labelled, "
          f"{gone} with the target gone after", flush=True)
    return dict(stem=stem, folder=video.parent.name, video=str(video), crops=written, gone=gone, missed=len(missed),
                tried=len(picked))


def save_crop(path, crop, why):
    """A crop (frame, rgb, fixed, boxes, ignore, mask) as an npz like build_kill_feedback.py's."""
    frame, rgb, crop_fixed, boxes, ignore, mask = crop
    np.savez_compressed(path, rgb=rgb, fixed=crop_fixed.astype(np.uint8), tmask=mask,
                        boxes=np.array(boxes, np.float32).reshape(-1, 4), scores=np.ones(len(boxes), np.float32),
                        mined=np.str_("auto_kill"), frame=np.int32(frame), why=np.str_(why),
                        ignore=np.array(ignore, np.float32).reshape(-1, 4))


def draw_sheet(files, path):
    """Every saved crop at THUMB px: each target's outline (its mask's edge) in green, the ignored boxes in yellow."""
    rows = (len(files) + SHEET_COLUMNS - 1) // SHEET_COLUMNS
    page = Image.new("RGB", (SHEET_COLUMNS * THUMB, max(1, rows) * THUMB))
    scale = THUMB / CROP
    for n, file in enumerate(files):
        crop = np.load(file)
        rgb = crop["rgb"].copy()
        mask = crop["tmask"].astype(bool)
        rgb[mask & ~ndimage.binary_erosion(mask)] = (0, 255, 0)
        thumb = Image.fromarray(rgb).resize((THUMB, THUMB))
        pen = ImageDraw.Draw(thumb)
        for cx, cy, width, height in crop["ignore"]:
            pen.rectangle([(cx - width / 2) * scale, (cy - height / 2) * scale, (cx + width / 2) * scale,
                           (cy + height / 2) * scale], outline="yellow")
        page.paste(thumb, (n % SHEET_COLUMNS * THUMB, n // SHEET_COLUMNS * THUMB))
    page.save(path)


def picked_recordings(lib, kinds, words, count, rnd):
    """build_kill_feedback.recordings for each kind, together, in random order: `count` of them."""
    pairs = [pair for kind in kinds for pair in build_kill_feedback.recordings(lib, kind, words, count, rnd)
             if not any(word in pair[0].name.lower() for word in LEFT_OUT)]
    rnd.shuffle(pairs)
    return pairs[:count]


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("out", type=Path)
    parser.add_argument("model")
    parser.add_argument("--match", default="tile", help="comma-separated words, one of which the scenario's name has")
    parser.add_argument("--kinds", default="static,dynamic,switching")
    parser.add_argument("--recordings", type=int, default=40)
    parser.add_argument("--per-recording", type=int, default=25)
    parser.add_argument("--seed", type=int, default=0)
    args = parser.parse_args()
    rnd = random.Random(args.seed)
    lib = aimview_tools.Library()
    args.program = eval_video_alone.review_program()
    words = [word.strip().lower() for word in args.match.split(",")]
    picked = picked_recordings(lib, args.kinds.split(","), words, args.recordings, rnd)
    rows = [crops_of(lib, video, stats, args, rnd) for video, stats in picked]
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "manifest.jsonl").write_text("".join(json.dumps(row) + "\n" for row in rows))
    draw_sheet(sorted((args.out / "train").glob(f"{TAG}*.npz")), args.out / "sheet.png")
    draw_sheet(sorted((args.out / "train").glob(f"{GONE_TAG}*.npz")), args.out / "sheet_gone.png")
    print(f"{sum(row['crops'] for row in rows)} crops and {sum(row.get('gone', 0) for row in rows)} with the target "
          f"gone from {len(rows)} recordings in {args.out}")


if __name__ == "__main__":
    main()
