"""Build the detector's dataset from the KovOBS library (REPRODUCE.md step 1).

Real frames, automatically labelled: for every static scenario (every target's MaxSpeed is 0 in its .sce file) it takes
up to --per-folder of the newest recordings, decodes their key frames only (one every ~2 s; a fraction of a second per
VOD), finds the screen's fixed parts (old_review.fixed_map) and labels the targets with the hand-written detector in
old_review.py. A VOD is kept only when its labels look trustworthy: targets found in most frames with a steady count.
Frames whose count is off are skipped.

Splits are by scenario folder (no scenario is in two splits): a stable hash puts 10% in val and 10% in test, and the
folders of the VODs used for the end-to-end evaluation are always in test.

Each kept frame gives 256 x 256 crops (one round the crosshair, one round a target, and every other frame one at
random), saved as compressed .npz:
  rgb    (256, 256, 3) uint8   the frame (1280 x 720 scale)
  fixed  (256, 256)    uint8   1 where the screen stays put (crosshair, HUD): the model's 4th input
  tmask  (256, 256)    uint8   1 on the labelled targets' pixels (for recoloring in training)
  boxes  (n, 4)        float32 cx, cy, w, h in px, for the targets whose center is in the crop
  scores (n,)          float32 the model's score of each box (--model only)
manifest.jsonl lists every VOD with its split, label statistics and whether it was kept.
Usage: python python/model/build_data.py [--vods <recordings' folder>] [--out test_out/vod_model/data] [--per-folder 4]
"""
import argparse
import glob
import hashlib
import json
import multiprocessing as mp
import os
import random
import re
import subprocess
import sys
from pathlib import Path

import numpy as np
from scipy import ndimage

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import aimview_tools  # noqa: E402
import local_config  # noqa: E402
import old_review  # noqa: E402

WIDTH, HEIGHT, FRAME_BYTES = old_review.W, old_review.H, old_review.FRAME
CROP = 256
SCEN, WORKSHOP = old_review.SCENARIOS
# the end-to-end evaluation VODs' scenarios: never trained on
TEST_FOLDERS = {"1w4ts Voltaic", "10 Sphere Hipfire Extra Small", "Pokeball 5 Sphere Hipfire Extra Small LG56 AIMGOD",
                "Pokeball 1 Sphere Hipfire Extra Small LG56 AIMGO"}
SPLIT_BUCKETS = 10                  # a folder's hash picks one of 10: one is val, one is test
# dark_scene: the median brightness where targets can be is over LIGHT_WALL, and under DARK_SHARE of it is DARK
LIGHT_WALL, DARK, DARK_SHARE = 140, 70, 0.02
SCENE_SAMPLES = 12                  # key frames looked at for dark_scene
# dark_labels: a blob of at least MIN_AREA px, filling a third of its box, at most WIDEST times wider than tall and
# TALLEST times taller than wide, under MAX_SIDE px a side, and mostly off the fixed map
MIN_AREA, MAX_SIDE, FILL_PARTS, WIDEST, TALLEST, ON_FIXED = 6, 400, 3, 2.5, 25, 0.6
LOOK_PX = 2                         # labels: the blob under a found target, within this
MIN_KEY_FRAMES = 8
MIN_MEDIAN, MAX_MEDIAN, STEADY_SHARE = 1, 15, 0.7   # a VOD's labels: median count from 1 to 15, steady in 7 of 10
STEADY_SLACK = 0.25                 # a frame's count is steady within a quarter of the median (or 1)
SPOT_JITTER_PX = 90                 # the crop round a target moves up to this far
HASH_CHARS = 10
SPLITS = ("train", "val", "test")


def static_scenarios():
    """Scenario names (lower case) whose targets cannot move: every non-player Character Profile has MaxSpeed 0."""
    out = set()
    for path in glob.glob(os.path.join(SCEN, "*.sce")) + glob.glob(os.path.join(WORKSHOP, "*", "*.sce")):
        text = open(path, encoding="utf-8", errors="replace").read().split("[Map Data]")[0]
        characters = re.split(r"\r?\n(?=\[Character Profile\])", text)[1:]
        speeds = [float(speed) for character in characters if not re.search(r"^Name=Player", character, re.M)
                  for speed in re.findall(r"^MaxSpeed=([-\d.]+)", character, re.M)]
        if speeds and max(speeds) == 0:
            out.add(os.path.basename(path)[:-4].lower())
    return out


def target_counts():
    """Targets alive at once per scenario (lower-case name): old_review.target_counts."""
    return old_review.target_counts()


def split_of(folder):
    if folder in TEST_FOLDERS:
        return "test"
    bucket = int(hashlib.md5(folder.encode()).hexdigest(), 16) % SPLIT_BUCKETS
    return "val" if bucket == 0 else "test" if bucket == 1 else "train"


def keyframes(video, pixel_format):
    size = FRAME_BYTES if pixel_format == "yuv420p" else WIDTH * HEIGHT * 3
    decoded = subprocess.run(["ffmpeg", "-v", "error", "-skip_frame", "nokey", "-i", video, "-fps_mode", "passthrough",
                              "-vf", f"scale={WIDTH}:{HEIGHT}:flags=area,format={pixel_format}", "-f", "rawvideo", "-"],
                             capture_output=True)
    data = decoded.stdout
    return [data[i:i + size] for i in range(0, len(data) - size + 1, size)]


def labels(yuv, mask, cross):
    """The hand-written detector's targets in one frame: (target pixel mask, [(cx, cy, w, h)])."""
    found = old_review.detect(yuv, mask, cross)
    target_mask = np.zeros((HEIGHT, WIDTH), np.uint8)
    boxes = []
    if not found:
        return target_mask, boxes
    blobs, _ = ndimage.label(old_review.contrast(yuv) > old_review.DIFF)
    extents = ndimage.find_objects(blobs)
    for x_deg, y_deg, _ in found:
        x, y = old_review.to_px(x_deg, y_deg)
        row, column = min(HEIGHT - 1, max(0, int(round(y)))), min(WIDTH - 1, max(0, int(round(x))))
        window = blobs[max(0, row - LOOK_PX):row + LOOK_PX + 1, max(0, column - LOOK_PX):column + LOOK_PX + 1]
        ids = [blob for blob in np.unique(window) if blob]
        if not ids:
            continue
        blob = ids[0]
        extent = extents[blob - 1]
        target_mask[extent][blobs[extent] == blob] = 1
        boxes.append((x, y, float(extent[1].stop - extent[1].start), float(extent[0].stop - extent[0].start)))
    return target_mask, boxes


def dark_scene(rgbs, mask):
    """Does this recording show dark targets on light walls (the user's theme forces enemy colors to black)? The
    median brightness where targets can be is over 140, and under 2% of it is dark."""
    brightness = [np.frombuffer(rgb, np.uint8).reshape(HEIGHT, WIDTH, 3).max(axis=2)[mask]
                  for rgb in rgbs[::max(1, len(rgbs) // SCENE_SAMPLES)]]
    return float(np.median([np.median(values) for values in brightness])) > LIGHT_WALL and \
        float(np.median([(values < DARK).mean() for values in brightness])) < DARK_SHARE


def dark_labels(rgb, mask, fixed):
    """The targets in a frame of a dark_scene: blobs dark in every channel (max of R, G, B under 70), at least 6 px,
    filling a third of their box, no more than 2.5 times wider than tall (a health bar is wider) and up to 25 times
    taller (a thin capsule), under 400 px a side. A blob mostly on the fixed parts is the crosshair or HUD. Unlike
    old_review.detect, a big sphere, a capsule and a target held under the crosshair (tracking) are all found."""
    blobs, count = ndimage.label((rgb.max(axis=2) < DARK) & mask)
    target_mask = np.zeros((HEIGHT, WIDTH), np.uint8)
    boxes = []
    for blob, extent in enumerate(ndimage.find_objects(blobs) if count else [], 1):
        height, width = extent[0].stop - extent[0].start, extent[1].stop - extent[1].start
        on = blobs[extent] == blob
        area = int(on.sum())
        if area < MIN_AREA or height > MAX_SIDE or width > MAX_SIDE or area < height * width / FILL_PARTS \
                or width > WIDEST * height or height > TALLEST * width:
            continue
        if fixed[extent][on].mean() > ON_FIXED:
            continue
        target_mask[extent][on] = 1
        boxes.append(((extent[1].start + extent[1].stop) / 2, (extent[0].start + extent[0].stop) / 2, float(width),
                      float(height)))
    return target_mask, boxes


_DETECTOR = {}


def model_labels(model, rgb, mask, fixed):
    """A detector model's targets in one frame, at the threshold in the model's settings file: the boxes whose center is
    where targets can be (screen_mask: not on the HUD or KovOBS's boxes) and that are no more than 2.5 times wider than
    tall (a health bar is wider, as in dark_labels), as (cx, cy, w, h, score). The target mask is the ellipse that fills
    each box. The model runs on the CPU (ONNX Runtime, one thread in each process)."""
    if model not in _DETECTOR:
        import contract
        import infer
        settings, _ = contract.load_settings(model)
        if settings.get("score_map"):
            sys.exit(f"{model}: a model with a score map is not supported here")
        _DETECTOR[model] = infer.OnnxDetector(contract.calibrate.u8in_of(model), threads=1), float(settings["threshold"])
    detector, threshold = _DETECTOR[model]
    target_mask = np.zeros((HEIGHT, WIDTH), np.uint8)
    boxes = []
    for cx, cy, width, height, score in detector(rgb, fixed, threshold):
        if width > WIDEST * height or not mask[min(HEIGHT - 1, max(0, int(round(cy)))),
                                               min(WIDTH - 1, max(0, int(round(cx))))]:
            continue
        x0, x1 = max(0, int(cx - width / 2)), min(WIDTH, int(cx + width / 2) + 1)
        y0, y1 = max(0, int(cy - height / 2)), min(HEIGHT, int(cy + height / 2) + 1)
        yy, xx = np.ogrid[y0:y1, x0:x1]
        target_mask[y0:y1, x0:x1][((xx - cx) / max(1.0, width / 2)) ** 2 + ((yy - cy) / max(1.0, height / 2)) ** 2
                                  <= 1] = 1
        boxes.append((float(cx), float(cy), float(width), float(height), float(score)))
    return target_mask, boxes


def check_runs(lib):
    """The recordings the stats-file checks use (eval_vods.py, eval_moving.py and eval_video_alone.py, held-out runs
    too), as (folder, file): training on them would make the checks less independent. lib: an aimview_tools.Library,
    for the scenarios' kinds."""
    import eval_moving                                    # it imports this module: loaded here, not at the top
    runs = list(eval_moving.STATIC) + [video for videos in eval_moving.picks(lib).values() for video, _ in videos]
    out = {(Path(video).parent.name, Path(video).name) for video in runs}
    alone = json.loads((Path(__file__).resolve().parent / "video_alone_runs.json").read_text(encoding="utf-8"))
    return out | {tuple(run["id"].split("/", 1)) for run in alone}


def frame_labels(yuvs, rgbs, mask, cross, fixed, labeller):
    """Every key frame's labels, by the model, the dark-target rule or the hand-written detector."""
    dark, model = labeller
    if model:
        return [model_labels(model, np.frombuffer(rgb, np.uint8).reshape(HEIGHT, WIDTH, 3), mask, fixed) for rgb in rgbs]
    if dark:
        return [dark_labels(np.frombuffer(rgb, np.uint8).reshape(HEIGHT, WIDTH, 3), mask, fixed) for rgb in rgbs]
    return [labels(yuv, mask, cross) for yuv in yuvs]


def save_crops(folder, stem, frames, fixed, median, rnd, with_scores):
    """The crops of the frames whose label count is steady: one round the crosshair, one round a target, and every
    other frame one at random. Returns how many."""
    written = 0
    for k, ((target_mask, boxes), rgb) in enumerate(frames):
        if abs(len(boxes) - median) > max(1, STEADY_SLACK * median):
            continue
        image = np.frombuffer(rgb, np.uint8).reshape(HEIGHT, WIDTH, 3)
        spots = [(int(old_review.CX), int(old_review.CY))]
        if boxes:
            box = rnd.choice(boxes)
            spots.append((int(box[0]) + rnd.randint(-SPOT_JITTER_PX, SPOT_JITTER_PX),
                          int(box[1]) + rnd.randint(-SPOT_JITTER_PX, SPOT_JITTER_PX)))
        if k % 2 == 0:
            spots.append((rnd.randint(CROP // 2, WIDTH - CROP // 2), rnd.randint(CROP // 2, HEIGHT - CROP // 2)))
        for j, (spot_x, spot_y) in enumerate(spots):
            x0 = min(WIDTH - CROP, max(0, spot_x - CROP // 2))
            y0 = min(HEIGHT - CROP, max(0, spot_y - CROP // 2))
            inside = [box for box in boxes if x0 <= box[0] < x0 + CROP and y0 <= box[1] < y0 + CROP]
            crop_boxes = np.array([(x - x0, y - y0, width, height) for x, y, width, height, *_ in inside],
                                  np.float32).reshape(-1, 4)
            scores = dict(scores=np.array([box[4] for box in inside], np.float32)) if with_scores else {}
            np.savez_compressed(folder / f"{stem}_{k:03d}_{j}.npz", rgb=image[y0:y0 + CROP, x0:x0 + CROP],
                                fixed=fixed[y0:y0 + CROP, x0:x0 + CROP],
                                tmask=target_mask[y0:y0 + CROP, x0:x0 + CROP], boxes=crop_boxes, **scores)
            written += 1
    return written


def one(job):
    """Label one VOD's key frames and save its crops. Returns its manifest row."""
    folder, video, split, out, seed, dark, expect, model, other = job
    row = dict(folder=folder, file=Path(video).name, split=split, kept=False, crops=0)
    rnd = random.Random(seed)
    try:
        yuvs = keyframes(video, "yuv420p")
        rgbs = keyframes(video, "rgb24")
    except Exception as error:                        # a broken file must not stop the build
        return dict(row, error=str(error))
    row["keyframes"] = len(yuvs)
    if len(yuvs) < MIN_KEY_FRAMES or len(yuvs) != len(rgbs):
        return dict(row, reason="too few key frames")
    fixed = old_review.fixed_map(yuvs).astype(np.uint8)
    mask, _, cross = old_review.screen_mask(yuvs)
    if (dark or other) and dark_scene(rgbs, mask) != dark:
        return dict(row, reason="not dark targets on light walls" if dark else "dark targets on light walls")
    found = frame_labels(yuvs, rgbs, mask, cross, fixed, (dark, model))
    counts = np.array([len(boxes) for _, boxes in found])
    median = float(np.median(counts))
    steady = float(np.mean(np.abs(counts - median) <= max(1, STEADY_SLACK * median)))
    stem = hashlib.md5(video.encode()).hexdigest()[:HASH_CHARS]
    row.update(median=median, steady=round(steady, 3), stem=stem, counts=counts.tolist(), targets=expect)
    if median < MIN_MEDIAN or median > MAX_MEDIAN or steady < STEADY_SHARE:
        return dict(row, reason="labels not steady")
    if (dark or model) and expect and median > expect + 1:   # a dark grid, props or tiles, not the targets
        return dict(row, reason="more labels than targets")
    written = save_crops(Path(out) / split, stem, list(zip(found, rgbs)), fixed, median, rnd, bool(model))
    return dict(row, kept=True, crops=written)


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--vods", default=local_config.folder("vods"))
    parser.add_argument("--out", default="test_out/vod_model/data")
    parser.add_argument("--per-folder", type=int, default=4, help="the newest recordings of each scenario (0: every "
                        "one)")
    parser.add_argument("--kinds", default="static", help="scenario kinds, comma separated: static, dynamic, tracking, "
                        "switching (old_review.scenario_kinds; static alone keeps the MaxSpeed rule the first datasets "
                        "used)")
    scene = parser.add_mutually_exclusive_group()
    scene.add_argument("--dark", action="store_true", help="label with dark_labels (recordings of dark targets on "
                       "light walls only) instead of the hand-written detector")
    scene.add_argument("--other-themes", action="store_true", help="only recordings that are not dark targets on "
                       "light walls (the ones --dark leaves out)")
    parser.add_argument("--model", help="label with this detector model instead (an export in python/model/exports: "
                        "its _u8in export on the CPU, at the threshold in its settings file)")
    parser.add_argument("--skip-checks", action="store_true", help="leave out the stats-file checks' recordings")
    return parser.parse_args()


def jobs_of(args, scenarios, counts, skip):
    """The VODs to label as jobs for one(), and the rows of the checks' VODs left out."""
    jobs, skipped = [], []
    for folder in sorted(path for path in Path(args.vods).iterdir() if path.is_dir()):
        if folder.name.lower() not in scenarios:
            continue
        videos = sorted(folder.glob("*.mp4"), key=lambda path: -path.stat().st_mtime)[:args.per_folder or None]
        skipped += [dict(folder=folder.name, file=video.name, split=split_of(folder.name), kept=False, crops=0,
                         reason="a stats-file check's run", size=video.stat().st_size)
                    for video in videos if (folder.name, video.name) in skip]
        jobs += [(folder.name, str(video), split_of(folder.name), args.out, i, args.dark,
                  counts.get(folder.name.lower()), args.model, args.other_themes)
                 for i, video in enumerate(videos) if (folder.name, video.name) not in skip]
    return jobs, skipped


def old_rows(manifest):
    """The manifest's rows by (folder, file), when it exists."""
    old = {}
    if manifest.exists():
        for line in manifest.read_text(encoding="utf-8").splitlines():
            if line.strip():
                row = json.loads(line)
                old[(row["folder"], row["file"])] = row
    return old


def main():
    args = parse_args()
    for split in SPLITS:
        (Path(args.out) / split).mkdir(parents=True, exist_ok=True)
    kinds = set(args.kinds.split(","))
    if kinds == {"static"}:
        scenarios = static_scenarios()
    else:
        scenarios = {name for name, kind in old_review.scenario_kinds().items() if kind in kinds}
    skip = check_runs(aimview_tools.Library(args.vods)) if args.skip_checks else set()
    jobs, skipped = jobs_of(args, scenarios, old_review.target_counts(), skip)
    # incremental: a VOD already in the manifest (same file, same size) keeps its row and its crops
    old = old_rows(Path(args.out) / "manifest.jsonl")
    rows, todo = list(skipped), []
    for job in jobs:
        row = old.get((job[0], Path(job[1]).name))
        if row and row.get("size") == Path(job[1]).stat().st_size:
            rows.append(row)
        else:
            todo.append(job)
    print(f"{len(scenarios)} {args.kinds} scenarios installed; {len(skipped)} VODs of the checks left out, "
          f"{len(rows) - len(skipped)} already labelled, {len(todo)} to label", flush=True)
    with mp.Pool(max(1, mp.cpu_count() - 2)) as pool:
        for k, row in enumerate(pool.imap_unordered(one, todo)):
            row["size"] = (Path(args.vods) / row["folder"] / row["file"]).stat().st_size
            rows.append(row)
            print(f"[{k + 1}/{len(todo)}] {row['split']:5s} {'keep' if row['kept'] else 'drop'} "
                  f"{row['folder'][:44]:44s} {row.get('reason', '')} {row['crops']} crops", flush=True)
    with open(Path(args.out) / "manifest.jsonl", "w", encoding="utf-8") as manifest:
        for row in sorted(rows, key=lambda row: (row["split"], row["folder"], row["file"])):
            manifest.write(json.dumps(row) + "\n")
    for split in SPLITS:
        split_rows = [row for row in rows if row["split"] == split]
        print(f"{split}: {sum(row['kept'] for row in split_rows)} of {len(split_rows)} VODs kept, "
              f"{len({row['folder'] for row in split_rows if row['kept']})} scenarios, "
              f"{sum(row['crops'] for row in split_rows)} crops")


if __name__ == "__main__":
    main()
