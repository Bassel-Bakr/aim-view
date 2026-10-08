"""A batch of automatic labels for the user to check on the Crops page (crop_check/README.md): crops from many
recordings, each labelled twice, by the detector and by the target's color, and split by whether the two agree. Nothing
here goes into training data; the crops are only for review.

In: the library's recordings that have a stats file, outside the gate's own runs and build_auto_labels.LEFT_OUT, by
scenario kind (aimview-tool scenarios: kind and hitbox). Each kind has a quota (KIND_QUOTAS, scaled to --recordings):
one recording per scenario before a second, the priority scenarios first (tracking: thin capsules; static: small
targets), then scenarios that no dataset in the data folder's vod_model/*/manifest.jsonl holds, the rest at random
(--seed). A recording over MAX_VIDEO_BYTES is left out.

Each recording is reviewed as build_auto_labels.py reviews it (aimview-tool review, the excluded areas of
eval_video_alone.py), kept in <out>/reviews/<model>/<stem>/ so a rerun skips it. Its target's color is learned at the
stats file's kills: in the LOOK_BACK frames before each kill (the nearest first), build_auto_labels.target_color at the
crosshair; the median over the kills that give one, MIN_COLOR_KILLS or more. Short of that (a tracking stats file has
no kills), the same at up to ON_TARGET_FRAMES frames where the crosshair lies in a model box; short again, the
recording has no color. A pixel part may be as long against its width as HITBOX_SLACK times the scenario's hitbox
(build_auto_labels.MAX_ASPECT at least), so a thin pole can be a target. Then
--per-recording frames spread evenly over the run window (build_disagreements.run_window: after KovaaK's countdown, to
the stats file's length; without a countdown, the challenge's span from the stats file) give a 256 x 256 crop each,
every other one round the crosshair and the rest round a random model box of the frame, shifted up to 48 px at random.

Out, in <out>:
- agreed/: crops where every model box matches a pixel box (label_score.matched, IoU 0.5) and every pixel box a model
  box; their boxes are the pixel boxes, which are tighter. agreed/sample.txt names a seeded AGREED_SHARE of them.
- review/: every other crop with a box, its boxes the union (a matched pair as its pixel box), each tagged in `why`
  and `sources` with where it came from: "both", "pixels" or "model". A crop with no box from either goes here only
  once in EMPTY_EVERY (an empty wall to check). A recording with no color sends all its crops here.
- robots/: the crops of robot scenarios (ROBOT_WORDS, or a switching scenario's cylinder wider than THIN_CAPSULE), with
  the model's boxes only, for teacher_label.py.
Each crop is saved like build_disagreements.py's (rgb, fixed, tmask, boxes, scores, mined, frame, why) with
model_boxes, model_scores, pixel_boxes and sources besides. Each part has a manifest.jsonl (stem, folder, kind) for
make_page.py and a sheet.png of its first SHEET_CROPS crops (pixel boxes green, model boxes blue); summary.json holds
the counts. Last, crop_check/make_page.py makes the page <pages>/check_<out's name> with three sets: auto_review,
auto_robots and auto_agreed_sample.

Usage: python python/model/build_label_batch.py <out> [--model large_v13e4] [--recordings 150] [--per-recording 10]
       [--seed 0] [--pages <folder>]
"""
import argparse
import hashlib
import json
import random
import re
import subprocess
import sys
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw
from scipy.spatial import ConvexHull, QhullError

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
import aimview_tools  # noqa: E402
import build_auto_labels as auto  # noqa: E402
import build_data  # noqa: E402
import build_disagreements  # noqa: E402
import build_kill_feedback  # noqa: E402
import build_mined  # noqa: E402
import eval_video_alone  # noqa: E402
import infer  # noqa: E402
import label_score  # noqa: E402
import old_review  # noqa: E402
import teacher_label  # noqa: E402
from local_config import folder  # noqa: E402

CROP, JITTER_PX = build_mined.CROP, build_mined.JITTER_PX
WIDTH, HEIGHT = build_mined.WIDTH, build_mined.HEIGHT
KIND_QUOTAS = {"tracking": 45, "dynamic": 40, "static": 35, "switching": 30}   # recordings of each kind in 150
THIN_CAPSULE = 0.35             # a cylindrical hitbox's width over height under this is a thin capsule
SMALL_WORDS = ("small", "micro", "tiny")        # static scenarios with these in their names have small targets
# a robot scenario's name ("Bot 1" in Voltaic's names is a numbered target, a sphere or pill, so "bot" alone is not one;
# Valorant's botmap has its agents)
ROBOT_WORDS = re.compile(r"robot|humanoid|botmap")
MAX_VIDEO_BYTES = 400e6         # a recording larger than this is left out (its decode is slow)
COLOR_KILLS = 30                # the stats file's kills (at most) the target's color is learned at
MIN_COLOR_KILLS = 5             # a recording's color needs this many kills (or on-target frames) that give one
ON_TARGET_FRAMES = 30           # without enough kills, the frames (at most) with the crosshair in a model box it is
                                # learned at instead (a tracking stats file has no kills)
INSIDE_PX = 1                   # the crosshair is in a model box when this far inside its edges or more
HITBOX_SLACK = 1.5              # a part may be this many times as long against its width as the scenario's hitbox
LOOK_BACK = auto.LOOK_BACK      # frames before a kill its target is looked for in, nearest first
EMPTY_EVERY = 5                 # one crop in this many with no box from either labeller goes to review/
AGREED_SHARE = 0.15             # the share of agreed/ the user checks
PIXEL_SCORE = 1.0               # the score a box the pixels alone found shows on the page
SHEET_CROPS, SHEET_COLUMNS = 40, 8
PARTS = ("agreed", "review", "robots")
SETS = {"review": ("auto_review", "Auto labels: model and pixels disagree, or no color"),
        "robots": ("auto_robots", "Auto labels: robots (the model's boxes)"),
        "agreed": ("auto_agreed_sample", "Auto labels: model and pixels agree (a sample)")}


def retire_old_crops(out):
    """Moves the crops a run before left in the parts to <out>/retired/<part>/ (nothing is deleted), so a rerun's parts
    hold only its own crops; the reviews stay for it to reuse."""
    for part in PARTS:
        old = sorted((out / part).glob("*.npz"))
        if old:
            (out / "retired" / part).mkdir(parents=True, exist_ok=True)
            for path in old:
                path.replace(out / "retired" / part / path.name)


def dataset_folders():
    """The scenario folders the existing datasets (the data folder's vod_model/*/manifest.jsonl) took crops from."""
    used = set()
    for manifest in (folder("data") / "vod_model").glob("*/manifest.jsonl"):
        for line in manifest.read_text(encoding="utf-8").splitlines():
            row = json.loads(line)
            if row.get("crops", 1):
                used.add(row.get("folder") or Path(row.get("video", "")).parent.name)
    return used


def priority(kind, name, facts):
    """0 for a scenario this batch wants first (a thin capsule tracked, small static targets), else 1."""
    hitbox = facts.get("hitbox") or {}
    if kind == "tracking":
        return 0 if hitbox.get("kind") == "cylindrical" and hitbox["widthToHeight"] < THIN_CAPSULE else 1
    if kind == "static":
        return 0 if any(word in name for word in SMALL_WORDS) else 1
    return 1


def is_robot(kind, name, facts):
    """Whether a scenario's targets are robots, whose many colors no one color covers."""
    hitbox = facts.get("hitbox") or {}
    wide = hitbox.get("kind") == "cylindrical" and hitbox["widthToHeight"] > THIN_CAPSULE
    return bool(ROBOT_WORDS.search(name)) or (kind == "switching" and wide)


def quotas_for(count):
    """KIND_QUOTAS scaled to `count` recordings (largest remainders)."""
    total = sum(KIND_QUOTAS.values())
    exact = {kind: count * quota / total for kind, quota in KIND_QUOTAS.items()}
    out = {kind: int(share) for kind, share in exact.items()}
    for kind in sorted(exact, key=lambda kind: exact[kind] - out[kind], reverse=True)[:count - sum(out.values())]:
        out[kind] += 1
    return out


def candidates(lib):
    """{scenario: [(video, stats file)]}: the recordings with a stats file, outside the gate and LEFT_OUT, of a kind
    with a quota, under MAX_VIDEO_BYTES."""
    facts, gate = lib.scenarios(), build_kill_feedback.gate_videos(lib)
    found = defaultdict(list)
    for row in lib.list():
        more, scenario = lib.by_id[row["id"]], row["scenario"]
        if not (more["stats_file"] and more["video"]) or any(word in scenario.lower() for word in auto.LEFT_OUT):
            continue
        video = Path(more["video"])
        kind = facts.get(scenario.lower(), {}).get("kind")
        if (video.name in gate or kind not in KIND_QUOTAS or not video.is_file()
                or video.stat().st_size > MAX_VIDEO_BYTES):
            continue
        found[scenario].append((video, Path(more["stats_file"])))
    return found


def pick_recordings(lib, count, rnd):
    """{kind: [(video, stats file, scenario)]} by the module's quotas and order."""
    facts, used, found = lib.scenarios(), dataset_folders(), candidates(lib)
    ranked = defaultdict(list)
    for scenario in sorted(found):
        recordings = found[scenario]
        rnd.shuffle(recordings)
        name = scenario.lower()
        kind, tie = facts[name]["kind"], rnd.random()
        first = priority(kind, name, facts[name])
        for turn, (video, stats) in enumerate(recordings):
            ranked[kind].append(((turn, first, video.parent.name in used, tie), (video, stats, scenario)))
    picked = {}
    for kind, quota in quotas_for(count).items():
        picked[kind] = [pick for _, pick in sorted(ranked[kind], key=lambda ranked_pick: ranked_pick[0])[:quota]]
        print(f"{kind}: {len(picked[kind])} of {quota} wanted", flush=True)
        for video, _, scenario in picked[kind]:
            print(f"  {scenario} | {video.name}", flush=True)
    return picked


def reviewed(lib, video, stats, args):
    """A recording reviewed (once, under <out>/reviews): (its folder, its track frames, fps, the stats file's clock
    offset in seconds or None)."""
    folder_of = args.out / "reviews" / args.model / video.stem
    if not (folder_of / "tracks.json").is_file():
        lib.review_video(str(video), args.model, str(folder_of), stats=str(stats), areas=eval_video_alone.AREAS,
                         quiet=True)
    tracks = json.loads((folder_of / "tracks.json").read_bytes())
    report = eval_video_alone.core_review(args.program, folder_of, video, stats)
    return folder_of, tracks["frames"], tracks["fps"], report["summary"]["info"].get("offset")


def run_window(folder_of, stats, fps, frame_count, offset):
    """The run's frames (first, last): build_disagreements.run_window, or without a countdown the challenge's span
    from the stats file, EDGE_S in from each end; None without either."""
    window = build_disagreements.run_window(folder_of, stats, fps, frame_count)
    if window is not None or offset is None:
        return window
    first, last = eval_video_alone.challenge(stats, offset, fps)
    edge = round(build_mined.EDGE_S * fps)
    first, last = max(0, first + edge), min(frame_count - 1, last - edge)
    return (first, last) if first < last else None


def crosshair_crop(frame_rgb, fixed):
    """The crop centered on the crosshair (rgb, fixed) and the crosshair's place in it."""
    cx, cy = (int(value) for value in old_review.to_px(0, 0))
    x0, y0 = cx - CROP // 2, cy - CROP // 2
    return frame_rgb[y0:y0 + CROP, x0:x0 + CROP], fixed[y0:y0 + CROP, x0:x0 + CROP], (cx - x0, cy - y0)


def kill_colors(kills, decoded, fixed):
    """The target's color (RGB) at each kill that gives one: at the crosshair in the nearest LOOK_BACK frame that
    does."""
    colors = []
    for kill in kills:
        for back in LOOK_BACK:
            if kill - back not in decoded:
                continue
            rgb, crop_fixed, at = crosshair_crop(decoded[kill - back], fixed)
            color = auto.target_color(rgb, crop_fixed, at)
            if color is not None:
                colors.append(color)
                break
    return colors


def on_target(frames, window):
    """The frames of the run window where the crosshair lies in a model box, INSIDE_PX or more inside its edges."""
    x, y = old_review.to_px(0, 0)
    return [i for i in range(window[0], window[1] + 1)
            if any(abs(box[0] - x) <= box[2] / 2 - INSIDE_PX and abs(box[1] - y) <= box[3] / 2 - INSIDE_PX
                   for box in build_disagreements.boxes_px(frames[i]))]


def on_target_colors(video, frames, window, fixed):
    """The target's color (RGB) at the crosshair in up to ON_TARGET_FRAMES on-target frames spread over the run window,
    for each that gives one."""
    found = on_target(frames, window)
    picks = sorted({found[int((k + 0.5) * len(found) / ON_TARGET_FRAMES)] for k in range(ON_TARGET_FRAMES)}
                   if found else [])
    colors = []
    for frame_rgb in build_mined.decode(video, picks).values():
        rgb, crop_fixed, at = crosshair_crop(frame_rgb, fixed)
        color = auto.target_color(rgb, crop_fixed, at)
        if color is not None:
            colors.append(color)
    return colors


def learned_color(video, frames, window, kills, decoded, fixed):
    """The target's color (RGB): the median of its colors at the kills, or without MIN_COLOR_KILLS of them at the
    on-target frames; with where it came from ("kills", "on_target" or "none") and how many gave one."""
    colors, source = kill_colors(kills, decoded, fixed), "kills"
    if len(colors) < MIN_COLOR_KILLS:
        colors, source = on_target_colors(video, frames, window, fixed), "on_target"
    if len(colors) < MIN_COLOR_KILLS:
        return None, "none", len(colors)
    return np.median(np.array(colors), 0), source, len(colors)


def longest_aspect(facts):
    """The longest side a target's part may have against its shortest: build_auto_labels.MAX_ASPECT, or longer for a
    scenario whose hitbox is longer (HITBOX_SLACK times its own), so a thin pole can be a target."""
    hitbox = facts.get("hitbox") or {}
    ratio = hitbox.get("widthToHeight") or 0
    return max(auto.MAX_ASPECT, HITBOX_SLACK / min(ratio, 1 / ratio)) if ratio > 0 else auto.MAX_ASPECT


def target_shape(mask, max_aspect):
    """build_auto_labels.shape_of with its MAX_ASPECT as `max_aspect`: the part's box (cx, cy, w, h in crop pixels),
    area, whether it has a target's shape, and whether it touches the crop's edge."""
    rows, columns = np.nonzero(mask)
    y0, y1, x0, x1 = rows.min(), rows.max() + 1, columns.min(), columns.max() + 1
    width, height, area = float(x1 - x0), float(y1 - y0), int(mask.sum())
    try:
        hull = ConvexHull(np.column_stack([columns, rows])).volume
    except QhullError:                                  # a line of pixels
        hull = 0.0
    solid = (area >= auto.SOLID_SHARE * width * height and max(width, height) <= max_aspect * min(width, height)
             and width <= auto.BAR_ASPECT * height and area >= auto.CONVEX_SHARE * hull
             and not auto.two_targets(mask) and not auto.notched(mask, width, height))
    edge = x0 == 0 or y0 == 0 or x1 == CROP or y1 == CROP
    return [(x0 + x1) / 2, (y0 + y1) / 2, width, height], area, solid, edge


def pixel_boxes(rgb, fixed, color, max_aspect):
    """label_score.pixel_boxes with a part's longest side against its shortest up to `max_aspect`."""
    found = auto.parts(rgb, fixed, None, color)
    if found is None:
        return []
    labelled_parts, count = found
    boxes = []
    for number in range(1, count + 1):
        box, area, solid, edge = target_shape(labelled_parts == number, max_aspect)
        if solid and not edge and area >= auto.MIN_AREA_PX:
            boxes.append([float(value) for value in box])
    return boxes


def corner_round(center, rnd):
    """A crop's corner round `center` (x, y in frame pixels), shifted up to JITTER_PX at random."""
    x0 = int(np.clip(center[0] - CROP // 2 + rnd.randint(-JITTER_PX, JITTER_PX), 0, WIDTH - CROP))
    y0 = int(np.clip(center[1] - CROP // 2 + rnd.randint(-JITTER_PX, JITTER_PX), 0, HEIGHT - CROP))
    return x0, y0


def model_boxes_in(boxes, corner):
    """The frame's model boxes (cx, cy, w, h, score in frame pixels) whose center is in the crop at `corner`, in crop
    pixels."""
    x0, y0 = corner
    return [(box[0] - x0, box[1] - y0, box[2], box[3], box[4]) for box in boxes
            if x0 <= box[0] < x0 + CROP and y0 <= box[1] < y0 + CROP]


def labelled(model, pixel, color, robot):
    """A crop's part and labels: (part, boxes, scores, sources), the part None for an empty crop (left to the caller's
    EMPTY_EVERY). `model` holds (cx, cy, w, h, score) boxes and `pixel` (cx, cy, w, h) ones, in crop pixels."""
    if robot:
        return "robots", [box[:4] for box in model], [box[4] for box in model], ["model"] * len(model)
    if not model and not pixel:
        return None, [], [], []
    pairs = label_score.matched(pixel, model)
    if color is not None and len(pairs) == len(pixel) == len(model):
        by_pixel = dict(pairs)
        return "agreed", pixel, [model[by_pixel[i]][4] for i in range(len(pixel))], ["both"] * len(pixel)
    by_pixel = dict(pairs)
    boxes = [list(box) for box in pixel]
    scores = [model[by_pixel[i]][4] if i in by_pixel else PIXEL_SCORE for i in range(len(pixel))]
    sources = ["both" if i in by_pixel else "pixels" for i in range(len(pixel))]
    for j, box in enumerate(model):
        if j not in by_pixel.values():
            boxes.append(box[:4])
            scores.append(box[4])
            sources.append("model")
    return "review", boxes, scores, sources


def save_crop(path, rgb, crop_fixed, labels, model, pixel, frame, why):
    """One crop as an npz build_disagreements.py's way, with both labellers' boxes besides."""
    part, boxes, scores, sources = labels
    boxes = np.array(boxes, np.float32).reshape(-1, 4)
    path.parent.mkdir(parents=True, exist_ok=True)
    np.savez_compressed(path, rgb=rgb, fixed=crop_fixed.astype(np.uint8),
                        tmask=teacher_label.pill_mask(boxes, (CROP, CROP)), boxes=boxes,
                        scores=np.array(scores, np.float32), mined=np.str_("auto_batch"), frame=np.int32(frame),
                        why=np.str_(why), sources=np.array(sources, dtype=np.str_),
                        model_boxes=np.array([box[:4] for box in model], np.float32).reshape(-1, 4),
                        model_scores=np.array([box[4] for box in model], np.float32),
                        pixel_boxes=np.array(pixel, np.float32).reshape(-1, 4))


def why_of(part, sources, color, row):
    """A crop's note for the page: its part, each box's source, and where the color came from."""
    told = {"agreed": "the model and the pixels agree on every box",
            "review": "the model and the pixels disagree, or the pixels had no color",
            "robots": "a robot scenario: the model's boxes, for a teacher to box"}[part]
    boxes = ", ".join(f"box {i} {source}" for i, source in enumerate(sources)) or "no box"
    hue = ("no target color" if color is None
           else f"target color {tuple(int(value) for value in color)} from {row.get('color_count')} "
                f"{row.get('color_source')}")
    return f"auto labels: {told}; {boxes}; {hue}"


def frame_picks(window, count):
    """`count` frames spread evenly over the window (first, last): each the middle of its share."""
    first, last = window
    return sorted({round(first + (k + 0.5) * (last - first) / count) for k in range(count)})


def crops_of(lib, video, stats, scenario, kind, args, rnd, totals):
    """Writes one recording's crops into their parts and gives its manifest row (counts in `totals`)."""
    facts = lib.scenarios()[scenario.lower()]
    robot = is_robot(kind, scenario.lower(), facts)
    folder_of, frames, fps, offset = reviewed(lib, video, stats, args)
    row = dict(stem=hashlib.md5(video.name.encode()).hexdigest()[:10], folder=video.parent.name, kind=kind,
               scenario=scenario, video=str(video), robot=robot, crops=Counter())
    window = run_window(folder_of, stats, fps, len(frames), offset)
    if window is None:
        print(f"{video.name}: no run window; skipped", flush=True)
        return dict(row, reason="no run window")
    picks = frame_picks(window, args.per_recording)
    kills = []
    if not robot and offset is not None:
        truth = [kill for kill in eval_video_alone.truth_frames(stats, offset, fps, len(frames))
                 if kill > max(LOOK_BACK)]
        kills = sorted(rnd.sample(truth, min(COLOR_KILLS, len(truth))))
    decoded = build_mined.decode(video, sorted(set(picks) | {kill - back for kill in kills for back in LOOK_BACK}))
    fixed = old_review.fixed_map(build_data.keyframes(video, "yuv420p")).astype(bool)
    color, source, count = (None, "none", 0) if robot else learned_color(video, frames, window, kills, decoded, fixed)
    row.update(kills=len(kills), color_source=source, color_count=count, max_aspect=round(longest_aspect(facts), 2),
               color=None if color is None else color.round(1).tolist())
    for n, frame in enumerate(picks):
        if frame in decoded:
            crop_frame(decoded[frame], fixed, frames[frame], frame, n, row, color, args, rnd, totals)
    print(f"{video.name}: {len(kills)} kills, color from {count} {source}; crops {dict(row['crops'])}", flush=True)
    return row


def crop_frame(frame_rgb, fixed, tracks_frame, frame, n, row, color, args, rnd, totals):
    """Labels and saves one frame's crop: the even picks round the crosshair, the odd ones round a random model box
    (the crosshair when the frame has none)."""
    boxes = build_disagreements.boxes_px(tracks_frame)
    center = rnd.choice(boxes)[:2] if n % 2 and boxes else old_review.to_px(0, 0)
    x0, y0 = corner_round(center, rnd)
    rgb, crop_fixed = frame_rgb[y0:y0 + CROP, x0:x0 + CROP], fixed[y0:y0 + CROP, x0:x0 + CROP]
    model = model_boxes_in(boxes, (x0, y0))
    pixel = [] if color is None else pixel_boxes(rgb, crop_fixed, color, row["max_aspect"])
    labels = labelled(model, pixel, color, row["robot"])
    if labels[0] is None:
        totals["empty"] += 1
        if color is not None and totals["empty"] % EMPTY_EVERY:
            return
        labels = ("review", [], [], [])
    part, _, _, sources = labels
    why = why_of(part, sources, color, row)
    save_crop(args.out / part / f"{row['stem']}_{frame:05d}_{'cb'[n % 2]}.npz", rgb, crop_fixed,
              labels, model, pixel, frame, why)
    row["crops"][part] += 1
    totals[f"crops {part}"] += 1
    for source in sources:
        totals[f"boxes {part} {source}"] += 1


def draw_sheet(files, path):
    """The crops at full size, SHEET_COLUMNS a row: pixel boxes green, model boxes blue."""
    rows = max(1, (len(files) + SHEET_COLUMNS - 1) // SHEET_COLUMNS)
    page = Image.new("RGB", (SHEET_COLUMNS * CROP, rows * CROP))
    for n, file in enumerate(files):
        crop = np.load(file)
        thumb = Image.fromarray(crop["rgb"])
        pen = ImageDraw.Draw(thumb)
        for name, color in (("model_boxes", "blue"), ("pixel_boxes", "lime")):
            for cx, cy, width, height in crop[name]:
                pen.rectangle([cx - width / 2, cy - height / 2, cx + width / 2, cy + height / 2], outline=color)
        page.paste(thumb, (n % SHEET_COLUMNS * CROP, n // SHEET_COLUMNS * CROP))
    page.save(path)


def write_parts(rows, args, rnd):
    """Each part's manifest.jsonl and sheet.png, agreed/'s seeded sample (sample.txt and sample.jsonl, the latter for
    make_page.py); the sample's file names."""
    manifest = "".join(json.dumps({key: row[key] for key in ("stem", "folder", "kind", "scenario", "video")}) + "\n"
                       for row in rows)
    for part in PARTS:
        (args.out / part).mkdir(parents=True, exist_ok=True)
        (args.out / part / "manifest.jsonl").write_text(manifest, encoding="utf-8")
        files = sorted((args.out / part).glob("*.npz"))
        draw_sheet(files[:SHEET_CROPS], args.out / part / "sheet.png")
    agreed = sorted(path.name for path in (args.out / "agreed").glob("*.npz"))
    sample = sorted(rnd.sample(agreed, min(len(agreed), max(1, round(AGREED_SHARE * len(agreed))) if agreed else 0)))
    (args.out / "agreed" / "sample.txt").write_text("".join(name + "\n" for name in sample), encoding="utf-8")
    known = {row["stem"]: row for row in rows}
    (args.out / "agreed" / "sample.jsonl").write_text("".join(
        json.dumps(dict(file=name, folder=known[name[:10]]["folder"], kind=known[name[:10]]["kind"])) + "\n"
        for name in sample), encoding="utf-8")
    return sample


def make_pages(args):
    """The Crops page's sets (crop_check/make_page.py) in <pages>/check_<out's name>: all of review/ and robots/, and
    agreed/'s sample."""
    page = args.pages / f"check_{args.out.name}"
    sources = {"review": args.out / "review", "robots": args.out / "robots",
               "agreed": args.out / "agreed" / "sample.jsonl"}
    for part, source in sources.items():
        has_crops = any((args.out / part).glob("*.npz")) and (part != "agreed" or source.read_text().strip())
        if has_crops:
            set_name, title = SETS[part]
            subprocess.run([sys.executable, str(HERE / "crop_check" / "make_page.py"), str(page), set_name,
                            str(source), "--title", title], check=True)
    return page


def main():
    """Picks the recordings, writes their crops in their parts, the manifests, sheets, summary.json and the page."""
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("out", type=Path)
    parser.add_argument("--model", default=infer.BEST)
    parser.add_argument("--recordings", type=int, default=sum(KIND_QUOTAS.values()))
    parser.add_argument("--per-recording", type=int, default=10)
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--pages", type=Path, default=folder("data") / "vod_model",
                        help="the folder the page folder check_<out's name> goes in")
    args = parser.parse_args()
    sys.stdout.reconfigure(encoding="utf-8")        # scenario names hold characters Windows' console page lacks
    rnd = random.Random(args.seed)
    lib = aimview_tools.Library()
    args.program = eval_video_alone.review_program()
    picked = pick_recordings(lib, args.recordings, rnd)
    retire_old_crops(args.out)
    totals, rows = Counter(), []
    for kind, picks in picked.items():
        for video, stats, scenario in picks:
            rows.append(crops_of(lib, video, stats, scenario, kind, args, rnd, totals))
    sample = write_parts(rows, args, random.Random(args.seed))
    summary = dict(model=args.model, picked={kind: len(picks) for kind, picks in picked.items()},
                   reviewed=sum("reason" not in row for row in rows),
                   colored=sum(row.get("color") is not None for row in rows),
                   color_sources={kind: dict(Counter(row.get("color_source", "none") for row in rows
                                                     if row["kind"] == kind)) for kind in picked},
                   robots=sum(row["robot"] for row in rows), agreed_sample=len(sample), totals=dict(sorted(totals.items())),
                   recordings=[{key: value for key, value in row.items() if key != "video"} for row in rows])
    (args.out / "summary.json").write_text(json.dumps(summary, indent=1), encoding="utf-8")
    page = make_pages(args)
    print(json.dumps({key: value for key, value in summary.items() if key != "recordings"}, indent=1))
    print(f"the page: {page}")


if __name__ == "__main__":
    main()
