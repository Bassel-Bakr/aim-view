"""Build the detector's dataset from the KovOBS library (REPRODUCE.md step 1).

Real frames, automatically labelled: for every static scenario (every target's MaxSpeed is 0 in its .sce file) it takes
up to --per-folder of the newest recordings, decodes their key frames only (one every ~2 s; a fraction of a second per
VOD), finds the screen's fixed parts (review.fixed_map) and labels the targets with the hand-written detector in
review.py. A VOD is kept only when its labels look trustworthy: targets found in most frames with a steady count.
Frames whose count is off are skipped.

Splits are by scenario folder (no scenario is in two splits): a stable hash puts 10% in val and 10% in test, and the
folders of the VODs used for the end-to-end evaluation are always in test.

Each kept frame gives 256 x 256 crops (one round the crosshair, one round a target, and every other frame one at
random), saved as compressed .npz:
  rgb    (256, 256, 3) uint8   the frame (1280 x 720 scale)
  fixed  (256, 256)    uint8   1 where the screen stays put (crosshair, HUD): the model's 4th input
  tmask  (256, 256)    uint8   1 on the labelled targets' pixels (for recolouring in training)
  boxes  (n, 4)        float32 cx, cy, w, h in px, for the targets whose centre is in the crop
  scores (n,)          float32 the model's score of each box (--model only)
manifest.jsonl lists every VOD with its split, label statistics and whether it was kept.
Usage: python python/model/build_data.py [--vods E:/OBS/KovOBS] [--out test_out/vod_model/data] [--per-folder 4]
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
import review  # noqa: E402

W, H, FRAME = review.W, review.H, review.FRAME
CROP = 256
SCEN = r"C:\Program Files (x86)\Steam\steamapps\common\FPSAimTrainer\FPSAimTrainer\Saved\SaveGames\Scenarios"
WORKSHOP = r"C:\Program Files (x86)\Steam\steamapps\workshop\content\824270"
# the end-to-end evaluation VODs' scenarios: never trained on
TEST_FOLDERS = {"1w4ts Voltaic", "10 Sphere Hipfire Extra Small", "Pokeball 5 Sphere Hipfire Extra Small LG56 AIMGOD",
                "Pokeball 1 Sphere Hipfire Extra Small LG56 AIMGO"}


def static_scenarios():
    """Scenario names (lower case) whose targets cannot move: every non-player Character Profile has MaxSpeed 0."""
    out = set()
    for p in glob.glob(os.path.join(SCEN, "*.sce")) + glob.glob(os.path.join(WORKSHOP, "*", "*.sce")):
        t = open(p, encoding="utf-8", errors="replace").read().split("[Map Data]")[0]
        chars = re.split(r"\r?\n(?=\[Character Profile\])", t)[1:]
        sp = [float(m) for c in chars if not re.search(r"^Name=Player", c, re.M)
              for m in re.findall(r"^MaxSpeed=([-\d.]+)", c, re.M)]
        if sp and max(sp) == 0:
            out.add(os.path.basename(p)[:-4].lower())
    return out


def target_counts():
    """Targets alive at once per scenario (lower-case name): review.target_counts."""
    return review.target_counts()


def split_of(folder):
    if folder in TEST_FOLDERS:
        return "test"
    h = int(hashlib.md5(folder.encode()).hexdigest(), 16) % 10
    return "val" if h == 0 else "test" if h == 1 else "train"


def keyframes(video, fmt):
    size = FRAME if fmt == "yuv420p" else W * H * 3
    p = subprocess.run(["ffmpeg", "-v", "error", "-skip_frame", "nokey", "-i", video, "-fps_mode", "passthrough",
                        "-vf", f"scale={W}:{H}:flags=area,format={fmt}", "-f", "rawvideo", "-"], capture_output=True)
    b = p.stdout
    return [b[i:i + size] for i in range(0, len(b) - size + 1, size)]


def labels(yuv, mask, cross):
    """The hand-written detector's targets in one frame: (target pixel mask, [(cx, cy, w, h)])."""
    found = review.detect(yuv, mask, cross)
    tmask = np.zeros((H, W), np.uint8)
    boxes = []
    if not found:
        return tmask, boxes
    blobs, _ = ndimage.label(review.contrast(yuv) > review.DIFF)
    objs = ndimage.find_objects(blobs)
    for xd, yd, area in found:
        x, y = review.to_px(xd, yd)
        yi, xi = min(H - 1, max(0, int(round(y)))), min(W - 1, max(0, int(round(x))))
        win = blobs[max(0, yi - 2):yi + 3, max(0, xi - 2):xi + 3]
        ids = [i for i in np.unique(win) if i]
        if not ids:
            continue
        i = ids[0]
        sl = objs[i - 1]
        tmask[sl][blobs[sl] == i] = 1
        boxes.append((x, y, float(sl[1].stop - sl[1].start), float(sl[0].stop - sl[0].start)))
    return tmask, boxes


def dark_scene(rgbs, mask):
    """Does this recording show dark targets on light walls (the user's theme forces enemy colours to black)? The
    median brightness where targets can be is over 140, and under 2% of it is dark."""
    lum = [np.frombuffer(r, np.uint8).reshape(H, W, 3).max(axis=2)[mask] for r in rgbs[::max(1, len(rgbs) // 12)]]
    return float(np.median([np.median(x) for x in lum])) > 140 and \
        float(np.median([(x < 70).mean() for x in lum])) < 0.02


def dark_labels(rgb, mask, fixed):
    """The targets in a frame of a dark_scene: blobs dark in every channel (max of R, G, B under 70), at least 6 px,
    filling a third of their box, no more than 2.5 times wider than tall (a health bar is wider) and up to 25 times
    taller (a thin capsule), under 400 px a side. A blob mostly on the fixed parts is the crosshair or HUD. Unlike
    review.detect, a big sphere, a capsule and a target held under the crosshair (tracking) are all found."""
    lab, n = ndimage.label((rgb.max(axis=2) < 70) & mask)
    tmask = np.zeros((H, W), np.uint8)
    boxes = []
    for i, sl in enumerate(ndimage.find_objects(lab) if n else [], 1):
        h, w = sl[0].stop - sl[0].start, sl[1].stop - sl[1].start
        on = lab[sl] == i
        area = int(on.sum())
        if area < 6 or h > 400 or w > 400 or area < h * w / 3 or w > 2.5 * h or h > 25 * w:
            continue
        if fixed[sl][on].mean() > 0.6:
            continue
        tmask[sl][on] = 1
        boxes.append(((sl[1].start + sl[1].stop) / 2, (sl[0].start + sl[0].stop) / 2, float(w), float(h)))
    return tmask, boxes


_DETECTOR = {}


def model_labels(model, rgb, mask, fixed):
    """A detector model's targets in one frame, at the threshold in the model's settings file: the boxes whose centre is
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
    det, thr = _DETECTOR[model]
    tmask = np.zeros((H, W), np.uint8)
    boxes = []
    for cx, cy, w, h, s in det(rgb, fixed, thr):
        if w > 2.5 * h or not mask[min(H - 1, max(0, int(round(cy)))), min(W - 1, max(0, int(round(cx))))]:
            continue
        x0, x1 = max(0, int(cx - w / 2)), min(W, int(cx + w / 2) + 1)
        y0, y1 = max(0, int(cy - h / 2)), min(H, int(cy + h / 2) + 1)
        yy, xx = np.ogrid[y0:y1, x0:x1]
        tmask[y0:y1, x0:x1][((xx - cx) / max(1.0, w / 2)) ** 2 + ((yy - cy) / max(1.0, h / 2)) ** 2 <= 1] = 1
        boxes.append((float(cx), float(cy), float(w), float(h), float(s)))
    return tmask, boxes


def check_runs():
    """The recordings the stats-file checks use (eval_vods.py, eval_moving.py and eval_video_alone.py, held-out runs
    too), as (folder, file): training on them would make the checks less independent."""
    import eval_moving                                    # it imports this module: loaded here, not at the top
    runs = list(eval_moving.STATIC) + [v for vs in eval_moving.picks().values() for v, _ in vs]
    out = {(Path(v).parent.name, Path(v).name) for v in runs}
    alone = json.loads((Path(__file__).resolve().parent / "video_alone_runs.json").read_text(encoding="utf-8"))
    return out | {tuple(r["id"].split("/", 1)) for r in alone}


def one(job):
    """Label one VOD's key frames and save its crops. Returns its manifest row."""
    folder, video, split, out, seed, dark, expect, model, other = job
    row = dict(folder=folder, file=Path(video).name, split=split, kept=False, crops=0)
    rnd = random.Random(seed)
    try:
        yuvs = keyframes(video, "yuv420p")
        rgbs = keyframes(video, "rgb24")
    except Exception as e:                            # a broken file must not stop the build
        return dict(row, error=str(e))
    row["keyframes"] = len(yuvs)
    if len(yuvs) < 8 or len(yuvs) != len(rgbs):
        return dict(row, reason="too few key frames")
    fixed = review.fixed_map(yuvs).astype(np.uint8)
    mask, _, cross = review.screen_mask(yuvs)
    if (dark or other) and dark_scene(rgbs, mask) != dark:
        return dict(row, reason="not dark targets on light walls" if dark else "dark targets on light walls")
    if model:
        labs = [model_labels(model, np.frombuffer(r, np.uint8).reshape(H, W, 3), mask, fixed) for r in rgbs]
    elif dark:
        labs = [dark_labels(np.frombuffer(r, np.uint8).reshape(H, W, 3), mask, fixed) for r in rgbs]
    else:
        labs = [labels(y, mask, cross) for y in yuvs]
    counts = np.array([len(b) for _, b in labs])
    med = float(np.median(counts))
    steady = float(np.mean(np.abs(counts - med) <= max(1, 0.25 * med)))
    stem = hashlib.md5(video.encode()).hexdigest()[:10]
    row.update(median=med, steady=round(steady, 3), stem=stem, counts=counts.tolist(), targets=expect)
    if med < 1 or med > 15 or steady < 0.7:
        return dict(row, reason="labels not steady")
    if (dark or model) and expect and med > expect + 1:   # a dark grid, props or tiles, not the targets
        return dict(row, reason="more labels than targets")
    d = Path(out) / split
    n = 0
    for k, ((tmask, boxes), rgb) in enumerate(zip(labs, rgbs)):
        if abs(len(boxes) - med) > max(1, 0.25 * med):
            continue
        img = np.frombuffer(rgb, np.uint8).reshape(H, W, 3)
        spots = [(int(review.CX), int(review.CY))]
        if boxes:
            b = rnd.choice(boxes)
            spots.append((int(b[0]) + rnd.randint(-90, 90), int(b[1]) + rnd.randint(-90, 90)))
        if k % 2 == 0:
            spots.append((rnd.randint(CROP // 2, W - CROP // 2), rnd.randint(CROP // 2, H - CROP // 2)))
        for j, (sx, sy) in enumerate(spots):
            x0 = min(W - CROP, max(0, sx - CROP // 2))
            y0 = min(H - CROP, max(0, sy - CROP // 2))
            inside = [b for b in boxes if x0 <= b[0] < x0 + CROP and y0 <= b[1] < y0 + CROP]
            bb = np.array([(x - x0, y - y0, w, h) for x, y, w, h, *_ in inside], np.float32).reshape(-1, 4)
            scores = dict(scores=np.array([b[4] for b in inside], np.float32)) if model else {}
            np.savez_compressed(d / f"{stem}_{k:03d}_{j}.npz", rgb=img[y0:y0 + CROP, x0:x0 + CROP],
                                fixed=fixed[y0:y0 + CROP, x0:x0 + CROP], tmask=tmask[y0:y0 + CROP, x0:x0 + CROP],
                                boxes=bb, **scores)
            n += 1
    return dict(row, kept=True, crops=n)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--vods", default=r"E:\OBS\KovOBS")
    ap.add_argument("--out", default="test_out/vod_model/data")
    ap.add_argument("--per-folder", type=int, default=4, help="the newest recordings of each scenario (0: every one)")
    ap.add_argument("--kinds", default="static", help="scenario kinds, comma separated: static, dynamic, tracking, "
                    "switching (review.scenario_kinds; static alone keeps the MaxSpeed rule the first datasets used)")
    scene = ap.add_mutually_exclusive_group()
    scene.add_argument("--dark", action="store_true", help="label with dark_labels (recordings of dark targets on light "
                       "walls only) instead of the hand-written detector")
    scene.add_argument("--other-themes", action="store_true", help="only recordings that are not dark targets on light "
                       "walls (the ones --dark leaves out)")
    ap.add_argument("--model", help="label with this detector model instead (an export in python/model/exports: its "
                    "_u8in export on the CPU, at the threshold in its settings file)")
    ap.add_argument("--skip-checks", action="store_true", help="leave out the stats-file checks' recordings")
    a = ap.parse_args()
    for s in ("train", "val", "test"):
        (Path(a.out) / s).mkdir(parents=True, exist_ok=True)
    kinds = set(a.kinds.split(","))
    if kinds == {"static"}:
        static = static_scenarios()
    else:
        static = {n for n, k in review.scenario_kinds().items() if k in kinds}
    counts = review.target_counts()
    skip = check_runs() if a.skip_checks else set()
    jobs, skipped = [], []
    for folder in sorted(p for p in Path(a.vods).iterdir() if p.is_dir()):
        if folder.name.lower() not in static:
            continue
        vids = sorted(folder.glob("*.mp4"), key=lambda p: -p.stat().st_mtime)[:a.per_folder or None]
        skipped += [dict(folder=folder.name, file=v.name, split=split_of(folder.name), kept=False, crops=0,
                         reason="a stats-file check's run", size=v.stat().st_size)
                    for v in vids if (folder.name, v.name) in skip]
        jobs += [(folder.name, str(v), split_of(folder.name), a.out, i, a.dark, counts.get(folder.name.lower()),
                  a.model, a.other_themes) for i, v in enumerate(vids) if (folder.name, v.name) not in skip]
    # incremental: a VOD already in the manifest (same file, same size) keeps its row and its crops
    man = Path(a.out) / "manifest.jsonl"
    old = {}
    if man.exists():
        for line in man.read_text(encoding="utf-8").splitlines():
            if line.strip():
                r = json.loads(line)
                old[(r["folder"], r["file"])] = r
    rows, todo = skipped, []
    for job in jobs:
        r = old.get((job[0], Path(job[1]).name))
        if r and r.get("size") == Path(job[1]).stat().st_size:
            rows.append(r)
        else:
            todo.append(job)
    jobs = todo
    print(f"{len(static)} {a.kinds} scenarios installed; {len(skipped)} VODs of the checks left out, "
          f"{len(rows) - len(skipped)} already labelled, {len(jobs)} to label", flush=True)
    with mp.Pool(max(1, mp.cpu_count() - 2)) as pool:
        for k, row in enumerate(pool.imap_unordered(one, jobs)):
            row["size"] = (Path(a.vods) / row["folder"] / row["file"]).stat().st_size
            rows.append(row)
            print(f"[{k + 1}/{len(jobs)}] {row['split']:5s} {'keep' if row['kept'] else 'drop'} {row['folder'][:44]:44s}"
                  f" {row.get('reason', '')} {row['crops']} crops", flush=True)
    with open(Path(a.out) / "manifest.jsonl", "w", encoding="utf-8") as f:
        for r in sorted(rows, key=lambda r: (r["split"], r["folder"], r["file"])):
            f.write(json.dumps(r) + "\n")
    for s in ("train", "val", "test"):
        rs = [r for r in rows if r["split"] == s]
        print(f"{s}: {sum(r['kept'] for r in rs)} of {len(rs)} VODs kept, "
              f"{len({r['folder'] for r in rs if r['kept']})} scenarios, {sum(r['crops'] for r in rs)} crops")


if __name__ == "__main__":
    main()
