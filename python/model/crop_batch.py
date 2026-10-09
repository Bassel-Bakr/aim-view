"""What the crop builders share (build_disagreements.py, build_kill_feedback.py, build_auto_labels.py,
build_label_batch.py, build_merged_pairs.py): a recording's stem and fixed map, a review's boxes and run window, the
gate's recordings and the recordings of a kind, the crop file's arrays, the manifest, the contact sheet, moving a run's
old crops aside, and the steps of a per-recording builder's main (run_builder). The reviews come from aimview-tool
review (tracks.json, readings.json); each builder writes its crops, manifest and sheets in its own out folder.
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

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
import aimview_tools  # noqa: E402
import build_data  # noqa: E402
import build_mined  # noqa: E402
import eval_moving  # noqa: E402
import eval_video_alone  # noqa: E402
import eval_vods  # noqa: E402
import old_review  # noqa: E402

CROP = build_mined.CROP
STEM_CHARS = 10                 # a recording's stem: this many hex digits of its file name's MD5
OUTLINE_RGB = (0, 255, 0)       # a target mask's outline on a sheet


def recording_stem(video):
    """A recording's stem in crop names and manifests: the first STEM_CHARS hex digits of its file name's MD5."""
    return hashlib.md5(Path(video).name.encode()).hexdigest()[:STEM_CHARS]


def fixed_mask(video, dtype):
    """The recording's fixed map (old_review.fixed_map: the pixels that never change, as the crosshair's and the HUD's)
    from its key frames, as `dtype` (bool or np.uint8), 1280 x 720."""
    return old_review.fixed_map(build_data.keyframes(video, "yuv420p")).astype(dtype)


def boxes_px(frame):
    """A tracks.json frame's boxes as (cx, cy, w, h, score) in 1280 x 720 pixels."""
    out = []
    for (_, x, y), (width_deg, height_deg), score in zip(frame["t"], frame.get("wh", []), frame.get("s", [])):
        cx, cy = old_review.to_px(x, y)
        out.append((cx, cy, *build_mined.px_size(x, y, width_deg, height_deg), score))
    return out


def run_window(folder, stats, fps, frame_count):
    """The run's frames (first, last) from a review's countdown and the stats file's length, EDGE_S in from each end
    (build_mined.py's window), or None without both."""
    readings = json.loads((folder / "readings.json").read_text())
    counting = [i for i, on in enumerate(readings.get("countdown") or []) if on]
    length = old_review.stats_length(str(stats)) if stats else None
    if not counting or not length:
        return None
    start = (counting[-1] + 1) / fps
    return (round((start + build_mined.EDGE_S) * fps),
            min(frame_count - 1, round((start + length - build_mined.EDGE_S) * fps)))


def gate_videos(lib):
    """The gate's own recordings, by file name: never mined, so the gate stays a test of them."""
    names = {Path(run["id"]).name for run in json.loads(eval_video_alone.RUNS.read_text(encoding="utf-8"))}
    names |= {Path(video).name for videos in eval_moving.picks(lib).values() for video, _ in videos}
    return names | {Path(video).name for video in eval_vods.DEFAULT}


def recordings(lib, kind, words, count, rnd):
    """Up to `count` (video, stats file) pairs of the kind with a stats file, outside the gate, whose scenario has one
    of the words: the newest of each scenario first, the scenarios in random order."""
    kinds, gate = lib.scenario_kinds(), gate_videos(lib)
    newest = {}
    for row in lib.list():                              # newest first
        more = lib.by_id[row["id"]]
        scenario = row["scenario"]
        if (more["stats_file"] and more["video"] and Path(more["video"]).name not in gate
                and kinds.get(scenario.lower()) == kind and any(word in scenario.lower() for word in words)):
            newest.setdefault(scenario, (Path(more["video"]), Path(more["stats_file"])))
    picked = list(newest.values())
    rnd.shuffle(picked)
    return picked[:count]


def crop_arrays(rgb, fixed, tmask, boxes, scores, frame, why, mined=None, **more):
    """A crop file's arrays in the order every builder saves them: rgb, fixed, tmask (the targets' mask), boxes (cx,
    cy, w, h in crop pixels), scores, mined (the rule that gave it, when given), frame, why, then `more` as given."""
    arrays = dict(rgb=rgb, fixed=fixed, tmask=tmask, boxes=boxes, scores=np.asarray(scores, np.float32))
    if mined is not None:
        arrays["mined"] = np.str_(mined)
    return dict(arrays, frame=np.int32(frame), why=np.str_(why), **more)


def save_crop_npz(path, *arrays, **named):
    """One crop file (crop_arrays' arguments), compressed."""
    np.savez_compressed(path, **crop_arrays(*arrays, **named))


def write_manifest(path, rows, keys=None):
    """A manifest.jsonl: one JSON line a row (only `keys` of it, when given), for crop_check/make_page.py."""
    path.write_text("".join(json.dumps(row if keys is None else {key: row[key] for key in keys}) + "\n"
                            for row in rows), encoding="utf-8")


def boxes_layer(key, color):
    """A sheet layer (draw_sheet) drawing the crop's boxes under `key` (cx, cy, w, h in crop pixels) in `color`."""
    def draw(pen, crop, file, scale):
        """Draws the boxes at the sheet's `scale` (sheet px per crop px)."""
        for cx, cy, width, height in crop[key]:
            pen.rectangle([(cx - width / 2) * scale, (cy - height / 2) * scale, (cx + width / 2) * scale,
                           (cy + height / 2) * scale], outline=color)
    return draw


def outline_mask(crop):
    """A crop's rgb with its target mask's edge in OUTLINE_RGB (draw_sheet's `recolor`)."""
    rgb = crop["rgb"].copy()
    mask = crop["tmask"].astype(bool)
    rgb[mask & ~ndimage.binary_erosion(mask)] = OUTLINE_RGB
    return rgb


def draw_sheet(files, path, layers, columns, thumb_px=CROP, recolor=None):
    """A sheet of the crop files, `columns` a row, each `thumb_px` square: its rgb (or `recolor`'s of the loaded crop)
    and each layer's drawing over it (a layer takes the pen, the crop, its file and the scale, sheet px per crop px)."""
    rows = max(1, (len(files) + columns - 1) // columns)
    page = Image.new("RGB", (columns * thumb_px, rows * thumb_px))
    scale = thumb_px / CROP
    for n, file in enumerate(files):
        crop = np.load(file)
        thumb = Image.fromarray(crop["rgb"] if recolor is None else recolor(crop))
        if thumb_px != CROP:
            thumb = thumb.resize((thumb_px, thumb_px))
        pen = ImageDraw.Draw(thumb)
        for layer in layers:
            layer(pen, crop, file, scale)
        page.paste(thumb, (n % columns * thumb_px, n // columns * thumb_px))
    page.save(path)


def retire_old(out, parts):
    """Moves the crops a run before left in <out>/<part>/ to <out>/retired/<part>/ (nothing is deleted), so a rerun's
    parts hold only its own crops; the reviews stay for it to reuse."""
    for part in parts:
        old = sorted((out / part).glob("*.npz"))
        if old:
            (out / "retired" / part).mkdir(parents=True, exist_ok=True)
            for path in old:
                path.replace(out / "retired" / part / path.name)


def report_crops(rows, args):
    """The usual last line: the crop count and the recordings it came from."""
    print(f"{sum(row['crops'] for row in rows)} crops from {len(rows)} recordings in {args.out}")


def run_builder(description, add_arguments, picks_of, crops_of, finish=report_crops, program=True):
    """A per-recording builder's main (Template Method): the arguments (out, model, the builder's own from
    `add_arguments(parser)`, --seed), the library, one seeded random source for every step, each pick of
    `picks_of(lib, args, rnd)` (a tuple) turned into a manifest row by `crops_of(lib, *pick, args, rnd)`,
    <out>/manifest.jsonl, then `finish(rows, args)`. With `program`, args.program is the review program
    (eval_video_alone.review_program) for the core's reports."""
    parser = argparse.ArgumentParser(description=description)
    parser.add_argument("out", type=Path)
    parser.add_argument("model")
    add_arguments(parser)
    parser.add_argument("--seed", type=int, default=0)
    args = parser.parse_args()
    rnd = random.Random(args.seed)
    lib = aimview_tools.Library()
    if program:
        args.program = eval_video_alone.review_program()
    rows = [crops_of(lib, *pick, args, rnd) for pick in picks_of(lib, args, rnd)]
    args.out.mkdir(parents=True, exist_ok=True)
    write_manifest(args.out / "manifest.jsonl", rows)
    finish(rows, args)
