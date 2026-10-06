"""The contract a detector model must meet before the review uses it (REPRODUCE.md, "After a training run").

The review (src/) relies on things about the detector's output that no training metric states. Each
check measures one of them on the model's _u8in export (the graph the app runs) with its settings file
(exports/detector_<name>.json: the threshold and score map; calibrate.py's output when there is no file yet), and
compares it with full_v3's results on the same data (the limits and their reasons are at "the limits" below):

  export           the export's form, which src/detect.rs, the service and the browser assume: inputs "rgb" uint8
                   (n, h, w, 3) and "fixed" uint8 (n, h, w) with a free batch axis (they send 4 frames at once), outputs
                   "score" (n, 1, h/4, w/4) holding only the peaks, from 0 to 1, and "reg" (n, 4, h/4, w/4), finite; a
                   frame in a batch of 4 gives what it gives alone; a settings file the pipeline accepts; the fixed map
                   (the model's 4th input) made as the training crops' was (DIFF 30, SHARE 0.8, in src/fixed.rs and
                   old_review.py alike). Pass or fail, no limit.
  crosshair        the crosshair is not a target. On the static recordings of eval_moving.py: the frames where the
                   room moved 0.5 degrees or more since the frame before, and at least half that either side (the
                   camera's turn, read from the video by old_review.camera_motion; a one-frame jump is a shot's flash), with
                   a box on the fixed map's crosshair that stayed put (moved under half the room's move, and no box of
                   the frame before moved there with the room). A static target moves with the room; only something
                   fixed to the screen stays. A frame where most boxes stayed put is left out: the reading is wrong.
  screen_fixed     the same on the fixed map's other parts in the area the review reads: the HUD and overlay are left
                   out. (A box elsewhere that seems to stay put is mostly a tiled wall's seam meeting another seam's
                   place; that is the wall, not the screen, and the acceptance checks judge it.)
  boxes_per_frame  at most 50 boxes in a frame on those recordings: link() compares every pair of boxes in two frames,
                   up to 2,500.
  box_fit          the boxes fit the targets: center error (px), and width and height against the val labels (the
                   target's area, pi/4 x w x h, gives the review's target radius; the box, its on-target test in
                   tracking); center error against the held-out hand labels (the user's clicks: true ground truth).
  one_box          one box per target: second boxes inside a found target's box (a capsule cut in two), per target.
  under_crosshair  a target under the crosshair is still found (the review keeps every box within 2 degrees of it,
                   whatever it scores, but only if it is over the threshold): kill-moment labels on the fixed map.
  calibrated       the same precision at the same (mapped) score as full_v3, in each band of scores over the
                   threshold: the review's 0.5 rule and the faint cut-off read the scores on full_v3's scale.
  targets_found    recall at the threshold on each kind of val crop (static, kill moments, moving).

Writes a JSON report (python/model/reports/contract_<name>.json by default) and exits 1 when a check fails. The
recordings' camera readings and the model's peaks on them are kept in test_out/vod_model/contract/ (new files only),
so a second run takes minutes, not a quarter of an hour.
Usage: python python/model/contract.py <model> [--report FILE] [--settings FILE]
  <model>: a name (small_v13), or an export (python/model/exports/detector_<name>_u8in.onnx, or any detector_<name>*
  file beside it).
  --settings: another settings file, or a calibration report (calibrate.py --report), to check settings before they
  are written.
"""
import argparse
import hashlib
import json
import math
import re
import sys
from pathlib import Path

import numpy as np
from scipy import ndimage

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
import calibrate  # noqa: E402
import infer  # noqa: E402
import old_review  # noqa: E402

REPORTS = HERE / "reports"
CACHE = Path("test_out/vod_model/contract")          # camera readings, fixed maps, and each export's peaks
HAND = ("test_out/vod_model/hand_data", "test_out/vod_model/hand_data2")   # their test splits: no model trained on them
KILLS = "data_kills4"                                # the kill-moment crops, among calibrate.VAL
TURN = 0.5                                           # degrees the room moves between two frames for a turning pair
NEAR = 24                                            # px from the crosshair's center where its fixed-map parts start
MAX_BOXES = 50                                       # link(): 2,500 pairings at most
BANDS = (0.4, 0.5, 0.6, 0.7, 0.8)                    # the calibration's bands over the threshold (and 1.0)
DRAWS = 400                                          # draws of the val scenarios for full_v3's spread
SETTINGS_FORMAT = 1
MAP_AXES, MAP_COLUMNS = 2, 2                         # a score map: a table of [raw, mapped] points
MIN_MAP_POINTS = 2
RGB_AXES, FIXED_AXES = 4, 3                          # the inputs: (n, h, w, 3) and (n, h, w)
SHIFT_PX = 37                                        # the batch's other frames: the frame rolled by multiples of this
CELL_PX = 4                                          # the score map's cells
SAMPLE_CELLS = (180, 320)                            # a 720 x 1280 frame's cells
BATCH_FRAMES = 4                                     # frames the service and the browser send at once
SAME_OUTPUT = 1e-4                                   # a frame in a batch gives what it gives alone, within this
FIXED_DIFF, FIXED_SHARE = 30, 0.8                    # the fixed map's rule, as the training crops were made
CROSSHAIR_DOT_PX = 3                                 # the disc at the crosshair's center the area always holds
GROW_PX = 2                                          # the crosshair area and the HUD grown by this
EXAMPLES = 5                                         # screen-fixed examples kept per recording
CROP_PX = 256
TINY = 1e-9
HASH_CHARS = 10


def recordings():
    """The static recordings of eval_moving.py: the four eval_vods.py runs, the valorant run and three 1wall 6targets
    extra small runs."""
    import eval_moving
    return list(eval_moving.STATIC)


def video_key(video):
    return hashlib.md5(str(video).encode()).hexdigest()[:HASH_CHARS]


# ---- the model with its settings -------------------------------------------------------------------------------------
def load_settings(model, given=None):
    """The model's settings: the file given, else exports/detector_<name>.json beside its export, else what
    calibrate.py gives (marked as such)."""
    name = calibrate.model_name(model)
    path = Path(given) if given else calibrate.u8in_of(model).parent / f"detector_{name}.json"
    if path.is_file():
        saved = json.loads(path.read_text())
        return saved.get("settings", saved), str(path)     # a calibration report holds them under "settings"
    return calibrate.calibrate(model)[0], "calibrate.py (no settings file yet)"


def settings_problems(settings, name):
    """What the pipeline would refuse in a settings file (src/model.rs, MODEL_FILE.md), or a wrong name or reference."""
    bad = []
    if settings.get("format") != SETTINGS_FORMAT:
        bad.append("format is not 1")
    if settings.get("name") != name:
        bad.append(f"name {settings.get('name')!r} is not {name!r}")
    if settings.get("reference") != infer.BEST:
        bad.append(f"reference {settings.get('reference')!r} is not {infer.BEST!r}")
    threshold = settings.get("threshold")
    if not isinstance(threshold, (int, float)) or not 0 <= threshold <= 1:
        bad.append("threshold is not a number from 0 to 1")
    score_map = settings.get("score_map")
    if score_map is not None:
        points = np.array(score_map, np.float64) if isinstance(score_map, list) else np.zeros((0, 0))
        if points.ndim != MAP_AXES or points.shape[0] < MIN_MAP_POINTS or points.shape[1] != MAP_COLUMNS:
            bad.append("score_map is not 2 or more [raw, mapped] points")
        elif (points < 0).any() or (points > 1).any() or (np.diff(points, axis=0) <= 0).any():
            bad.append("score_map has a value outside 0 to 1, or a raw or mapped value that does not rise")
    return bad


class Model:
    """The _u8in export with its settings: boxes (cx, cy, w, h, mapped score) over the threshold."""

    def __init__(self, model, settings):
        self.path = calibrate.u8in_of(model)
        self.session = calibrate.session(self.path, gpu=True)
        # where it runs: the GPU's numbers differ from the CPU's a little, so each keeps its own cache
        self.device = "cuda" if self.session.get_providers()[0] == "CUDAExecutionProvider" else "cpu"
        self.threshold, self.score_map = float(settings["threshold"]), settings["score_map"]

    def mapped(self, peaks, threshold=None):
        """Peaks with their scores mapped, over the threshold (or `threshold`)."""
        peaks = peaks.copy()
        peaks[:, 4] = calibrate.apply_map(self.score_map, peaks[:, 4]).astype(np.float32)
        return peaks[peaks[:, 4] > (self.threshold if threshold is None else threshold)]

    def raw(self, rgb, fixed):
        """rgb (n, 720, 1280, 3) uint8 and one fixed map (720, 1280) -> per frame, its peaks over calibrate.FLOOR with
        their raw scores."""
        if len(rgb) > 1 and self.session.get_inputs()[0].shape[0] == 1:    # an export traced for one frame
            return [peaks for frame in rgb for peaks in self.raw(frame[None], fixed)]
        count = len(rgb)
        score, reg = self.session.run(None, {"rgb": rgb,
                                             "fixed": np.broadcast_to(fixed, (count, *fixed.shape)).copy()})
        return [infer.decode_np(score[k:k + 1], reg[k:k + 1], calibrate.FLOOR) for k in range(count)]

    def crops(self, scored, threshold=None):
        """A calibrate.Scored's crops with the mapped scores, only the boxes over the threshold (or `threshold`), as a
        new Scored."""
        return calibrate.Scored([self.mapped(peaks, threshold) for peaks in scored.dets], scored.gts, scored.files)


# ---- 1. the export's form -------------------------------------------------------------------------------------------
def form_problems(inputs, outputs):
    """What is wrong with the export's inputs and outputs, and whether checking further makes sense."""
    if set(inputs) != {"rgb", "fixed"} or set(outputs) != {"score", "reg"}:
        return [f"inputs {sorted(inputs)} and outputs {sorted(outputs)}: want rgb, fixed and score, reg"], False
    bad = []
    if inputs["rgb"].type != "tensor(uint8)" or inputs["fixed"].type != "tensor(uint8)" \
            or len(inputs["rgb"].shape) != RGB_AXES or len(inputs["fixed"].shape) != FIXED_AXES:
        bad.append("rgb and fixed are not uint8 (n, h, w, 3) and (n, h, w)")
    if isinstance(inputs["rgb"].shape[0], int):
        bad.append(f"the batch axis is fixed at {inputs['rgb'].shape[0]} (export.py --u8in gives a free one)")
    return bad, True


def output_problems(score, reg):
    """What is wrong with the outputs for a real frame: their shape, their range, and cells that are not peaks."""
    bad = []
    rows, columns = SAMPLE_CELLS
    if score.shape != (1, 1, rows, columns) or reg.shape != (1, 4, rows, columns):
        bad.append(f"outputs {score.shape} and {reg.shape} for a 720 x 1280 frame: want cells of 4 px")
    if not (np.isfinite(score).all() and np.isfinite(reg).all()) or score.min() < 0 or score.max() > 1:
        bad.append("scores outside 0 to 1, or values that are not finite")
    cells = score[0, 0]
    padded = np.pad(cells, 1)
    around = np.max([padded[1 + dy:rows + 1 + dy, 1 + dx:columns + 1 + dx] for dy in (-1, 0, 1) for dx in (-1, 0, 1)],
                    axis=0)
    if ((cells > 0) & (cells < around)).any():
        bad.append("the score map holds cells that are not peaks (the 3 x 3 peak finding is not in the graph)")
    return bad


def batch_difference(session, rgb, fixed, score, reg):
    """The largest difference between a frame alone and the same frame second in a batch of 4 (others different)."""
    four = np.stack([np.roll(rgb, SHIFT_PX * k, axis=1) if k != 1 else rgb for k in range(BATCH_FRAMES)])
    batch_score, batch_reg = session.run(None, {"rgb": four, "fixed": np.repeat(fixed[None].astype(np.uint8),
                                                                                 BATCH_FRAMES, axis=0)})
    return float(max(np.abs(batch_score[1] - score[0]).max(), np.abs(batch_reg[1] - reg[0]).max()))


def fixed_map_rule():
    """The fixed map's rule as the core and old_review.py make it."""
    source = (ROOT / "src" / "fixed.rs").read_text(encoding="utf-8")
    diff = re.search(r"pub const DIFF: f32 = ([\d.]+);", source)
    share = re.search(r"pub const SHARE: f64 = ([\d.]+);", source)
    return dict(core_diff=float(diff[1]) if diff else None, core_share=float(share[1]) if share else None,
                python_diff=old_review.DIFF)


def check_export(model, settings, name):
    session = model.session
    inputs = {node.name: node for node in session.get_inputs()}
    outputs = {node.name: node for node in session.get_outputs()}
    bad = settings_problems(settings, name)
    problems, go_on = form_problems(inputs, outputs)
    bad += problems
    if not go_on:
        return dict(passed=False, problems=bad)
    import bench
    rgb, fixed = bench.sample()                            # a real 1280 x 720 KovOBS frame
    score, reg = session.run(None, {"rgb": rgb[None], "fixed": fixed[None].astype(np.uint8)})
    bad += output_problems(score, reg)
    batch = None
    if not isinstance(inputs["rgb"].shape[0], int):
        batch = batch_difference(session, rgb, fixed, score, reg)
        if batch > SAME_OUTPUT:
            bad.append(f"a frame in a batch of 4 differs from the same frame alone by {batch:.1e}")
    fixed_map = fixed_map_rule()
    if fixed_map["core_diff"] != FIXED_DIFF or fixed_map["core_share"] != FIXED_SHARE or old_review.DIFF != FIXED_DIFF:
        bad.append(f"the fixed map is not made as the training crops' was (DIFF 30, SHARE 0.8): {fixed_map}")
    return dict(passed=not bad, problems=bad, batch_of_4_largest_difference=batch, fixed_map=fixed_map,
                peaks_on_sample_frame=int((score[0, 0] > model.threshold).sum()))


# ---- 2 to 4. recordings: the crosshair, other screen-fixed boxes, boxes per frame -----------------------------------
def camera(video):
    """The recording's fixed map (old_review.fixed_map) and the room's move on screen per frame (old_review.camera_motion,
    degrees; NaN where it has no reading), cached in test_out/vod_model/contract/."""
    stat = Path(video).stat()
    path = CACHE / f"{video_key(video)}.npz"
    if path.is_file():
        cached = np.load(path)
        if int(cached["size"]) == stat.st_size and float(cached["mtime"]) == stat.st_mtime:
            return cached["fixed"], cached["room"]
    fixed = old_review.fixed_map(list(old_review._frames(video, keyframes=True)))
    turns = old_review.camera_motion(video, [], mask=old_review.MASK, fixed=fixed)
    room = np.array([(turn[0], turn[1]) if turn else (np.nan, np.nan) for turn in turns], np.float64).reshape(-1, 2)
    CACHE.mkdir(parents=True, exist_ok=True)
    np.savez_compressed(path, fixed=fixed, room=room, size=stat.st_size, mtime=stat.st_mtime)
    return fixed, room


def crosshair_area(fixed):
    """Where the crosshair is: the fixed map's parts within NEAR px of the crosshair's center, whole, grown by 2 px,
    and a disc of 3 px at the center (in case the fixed map misses a dot)."""
    parts, _ = ndimage.label(fixed)
    cx, cy = int(round(old_review.CX)), int(round(old_review.CY))
    ids = np.unique(parts[cy - NEAR:cy + NEAR + 1, cx - NEAR:cx + NEAR + 1])
    area = np.isin(parts, ids[ids > 0])
    yy, xx = np.mgrid[0:old_review.H, 0:old_review.W]
    return ndimage.binary_dilation(area, iterations=GROW_PX) | \
        ((xx - old_review.CX) ** 2 + (yy - old_review.CY) ** 2 <= CROSSHAIR_DOT_PX ** 2)


def peaks_on(model, video, need, batch=4):
    """The model's raw peaks on the frames `need` of a recording, cached in test_out/vod_model/contract/ per export
    (its size and time), device and recording."""
    stat, export = Path(video).stat(), model.path.stat()
    device = "" if model.device == "cpu" else f"_{model.device}"
    path = CACHE / f"peaks_{model.path.stem}{device}_{video_key(video)}.npz"
    key = np.array([stat.st_size, stat.st_mtime, export.st_size, export.st_mtime])
    if path.is_file():
        cached = np.load(path)
        if np.array_equal(cached["key"], key) and need <= set(cached["frames"].tolist()):
            rows = cached["rows"]
            return {int(i): rows[rows[:, 0] == i, 1:] for i in cached["frames"]}
    peaks, frames, numbers = {}, [], []
    fixed = camera(video)[0].astype(np.uint8)
    for i, frame in enumerate(old_review.rgb_frames(video)):
        if i in need:
            frames.append(frame)
            numbers.append(i)
        if len(frames) == batch:
            peaks.update(zip(numbers, model.raw(np.stack(frames), fixed)))
            frames, numbers = [], []
    if frames:
        peaks.update(zip(numbers, model.raw(np.stack(frames), fixed)))
    rows = np.concatenate([np.c_[np.full(len(found), i), found] for i, found in peaks.items()] + [np.zeros((0, 6))])
    CACHE.mkdir(parents=True, exist_ok=True)
    np.savez_compressed(path, key=key, frames=np.array(sorted(peaks)), rows=rows)
    return peaks


def moved_with_room(prev, room):
    """Where the boxes of the frame before would be now if they were static targets: moved by the room's turn."""
    out = [old_review.to_px(*(np.array(old_review.to_deg(float(box[0]), float(box[1]))) + room)) for box in prev]
    return np.array(out, np.float64).reshape(-1, 2)


def on(mask, box):
    """Whether a box's center is on a frame mask (clamped into the frame)."""
    return bool(mask[min(old_review.H - 1, int(box[1])), min(old_review.W - 1, int(box[0]))])


def fixed_boxes(now, prev, room_turn):
    """Per box of a frame: whether it stayed put (moved under half the room's move on screen) and no static target of
    the frame before lands there with the room's turn."""
    x, y = old_review.to_px(*room_turn)
    tolerance = math.hypot(x - old_review.CX, y - old_review.CY) / 2     # half the room's move on screen
    moved = moved_with_room(prev, room_turn)
    out = []
    for box in now:
        stay = len(prev) and np.hypot(prev[:, 0] - box[0], prev[:, 1] - box[1]).min() < tolerance
        static = len(prev) and np.hypot(moved[:, 0] - box[0], moved[:, 1] - box[1]).min() < tolerance
        out.append(bool(stay and not static))
    return out


def turning_pairs(room):
    """The frames where the room turned TURN degrees or more since the frame before, and at least half that either
    side (a one-frame jump in the reading is a shot's flash)."""
    deg = np.hypot(room[:, 0], room[:, 1])
    return deg, [i for i in range(1, len(deg) - 1)
                 if deg[i] >= TURN and deg[i - 1] >= TURN / 2 and deg[i + 1] >= TURN / 2]


def recording(model, video):
    fixed, room = camera(video)
    deg, pairs = turning_pairs(room)
    raw = peaks_on(model, video, set(pairs) | {i - 1 for i in pairs})
    cross = crosshair_area(fixed)
    hud = ndimage.binary_dilation(fixed, iterations=GROW_PX) & ~cross   # the fixed map's other parts: HUD, overlay

    def boxes(i):                                      # over the threshold, center where the review reads (KovOBS's
        found = model.mapped(raw[i])                   # layout)
        return found[[bool(old_review.MASK[min(old_review.H - 1, max(0, int(box[1]))),
                                           min(old_review.W - 1, max(0, int(box[0])))]) for box in found]] \
            if len(found) else found

    on_cross = elsewhere = any_cross = wrong = 0
    examples = []
    for i in pairs:
        now, prev = boxes(i), boxes(i - 1)
        if not len(now):
            continue
        stayed = fixed_boxes(now, prev, room[i])
        at = [on(cross, box) for box in now]
        others = [put for put, at_cross in zip(stayed, at) if not at_cross]
        if others and sum(others) > len(others) / 2:   # most targets stayed put: the camera's reading is wrong here
            wrong += 1
            continue
        on_hud = [on(hud, box) for box in now]
        on_cross += any(put and at_cross for put, at_cross in zip(stayed, at))
        elsewhere += any(put and at_hud for put, at_hud in zip(stayed, on_hud))
        any_cross += any(at)
        screen_fixed = [put and (at_cross or at_hud) for put, at_cross, at_hud in zip(stayed, at, on_hud)]
        if any(screen_fixed) and len(examples) < EXAMPLES:
            examples.append(dict(frame=i, room_deg=[round(float(value), 3) for value in room[i]],
                                 boxes=[[round(float(value), 2) for value in box]
                                        for box, fixed_box in zip(now, screen_fixed) if fixed_box]))
    counts = np.array([len(boxes(i)) for i in raw])
    turning = len(pairs) - wrong
    return dict(video=Path(video).name, frames=len(deg), turning_pairs=turning, camera_contradicted=wrong,
                crosshair_pixels=int(cross.sum()),
                crosshair_pairs=on_cross, crosshair_share=round(on_cross / max(1, turning), 4),
                screen_fixed_pairs=elsewhere, screen_fixed_share=round(elsewhere / max(1, turning), 4),
                any_box_on_crosshair_share=round(any_cross / max(1, turning), 4),
                boxes_max=int(counts.max()) if len(counts) else 0,
                boxes_p99=float(np.percentile(counts, 99)) if len(counts) else 0.0, screen_fixed_examples=examples)


# ---- 5 to 10. crops -------------------------------------------------------------------------------------------------
# Each number can be weighted per crop (`crop_weight`): a draw of the scenarios weights each crop by how often its
# scenario was drawn, which gives full_v3's spread.
def weighted_percentile(values, weights, percentile):
    """The weighted p-th percentile of the values (NaN left out)."""
    ok = ~np.isnan(values) & (weights > 0)
    if not ok.any():
        return None
    values, weights = values[ok], weights[ok]
    order = np.argsort(values)
    cumulative = np.cumsum(weights[order])
    return float(values[order][min(len(values) - 1, np.searchsorted(cumulative, percentile / 100 * cumulative[-1]))])


def box_fit(scored, crop_weight=None):
    """Over the threshold, the boxes that took a label: center error (px), IoU, and width and height over the
    label's."""
    weights = (np.ones(len(scored.dets)) if crop_weight is None else crop_weight)[scored.crop] * scored.hit

    def at(values, percentile):
        value = weighted_percentile(values, weights, percentile)
        return None if value is None else round(value, 4)
    return dict(boxes=int(scored.hit.sum()), centre_error_median=at(scored.err, 50),
                centre_error_p90=at(scored.err, 90), iou_median=at(scored.iou, 50), iou_p10=at(scored.iou, 10),
                width_ratio_median=at(scored.w_ratio, 50), height_ratio_median=at(scored.h_ratio, 50))


def under_crosshair(fixed_path, truth, took):
    """On a kill-moment crop: its labels under the crosshair (touching the fixed map), and how many were found."""
    fixed = np.load(fixed_path)["fixed"]
    under = found = 0
    for j, box in enumerate(truth):
        x0, x1 = int(max(0, box[0] - box[2] / 2)), int(min(CROP_PX, math.ceil(box[0] + box[2] / 2)))
        y0, y1 = int(max(0, box[1] - box[3] / 2)), int(min(CROP_PX, math.ceil(box[1] + box[3] / 2)))
        if fixed[y0:y1, x0:x1].any():
            under += 1
            found += j in took
    return under, found


def per_crop(scored):
    """Per crop: targets found, second boxes (boxes that took no label with their center inside a label another box
    took), and on the kill-moment crops the labels under the crosshair (touching the fixed map) and how many of those
    were found; and the crop's kind (KINDS)."""
    found, extra, under, under_found = (np.zeros(len(scored.dets)) for _ in range(4))
    for crop, (peaks, truth, file) in enumerate(zip(scored.dets, scored.gts, scored.files)):
        took = calibrate.match(peaks, truth)
        found[crop] = (took >= 0).sum()
        taken = truth[took[took >= 0]]
        for box in peaks[took < 0]:
            extra[crop] += bool(len(taken) and ((np.abs(taken[:, 0] - box[0]) <= taken[:, 2] / 2) &
                                                (np.abs(taken[:, 1] - box[1]) <= taken[:, 3] / 2)).any())
        if KILLS in str(file) and len(truth):
            labels_under, labels_found = under_crosshair(file, truth, took)
            under[crop] += labels_under
            under_found[crop] += labels_found
    return dict(found=found, extra=extra, under=under, under_found=under_found,
                kind=np.array([1 if KILLS in str(file) else 2 if "moving" in str(file) else 0 for file in scored.files]))


KINDS = ("static", "kill_moments", "moving")


def bands(threshold):
    return [threshold] + [band for band in BANDS if band > threshold] + [1.0]


def band_counts(scored, edges):
    """The boxes scoring in each band (over its lower edge, up to its upper)."""
    return [int(((scored.score > low) & (scored.score <= high)).sum()) for low, high in zip(edges[:-1], edges[1:])]


def crop_checks(scored, per, edges, crop_weight=None):
    """The val numbers of the checks box_fit, one_box, under_crosshair, calibrated and targets_found."""
    crop_weight = np.ones(len(scored.dets)) if crop_weight is None else crop_weight

    def ratio(a, b):
        return float((crop_weight * a).sum() / max(TINY, (crop_weight * b).sum()))
    weights = crop_weight[scored.crop]
    precision = []
    for low, high in zip(edges[:-1], edges[1:]):
        in_band = (scored.score > low) & (scored.score <= high)
        precision.append(float((weights * in_band * scored.hit).sum() / max(TINY, (weights * in_band).sum())))
    return dict(box_fit=box_fit(scored, crop_weight), one_box=ratio(per["extra"], per["found"]),
                under_crosshair=ratio(per["under_found"], per["under"]), band_precision=precision,
                recall_by_kind={kind: ratio(per["found"] * (per["kind"] == i), scored.crop_labels * (per["kind"] == i))
                                for i, kind in enumerate(KINDS)})


def flat(numbers, prefix=""):
    """A nested dict of numbers as {"a.b": value} (a list's items by index)."""
    out = {}
    for key, value in (numbers.items() if isinstance(numbers, dict) else enumerate(numbers)):
        if isinstance(value, (dict, list)):
            out.update(flat(value, f"{prefix}{key}."))
        else:
            out[f"{prefix}{key}"] = value
    return out


def spread(scored, per, hand, edges, draws=DRAWS, seed=0):
    """full_v3's standard deviation over draws of the val scenarios (for the hand crops: draws of the crops) of each
    number with a relative limit."""
    rng = np.random.default_rng(seed)
    rows, hand_rows = [], []
    for _ in range(draws):
        draw = np.bincount(rng.integers(0, scored.units, scored.units), minlength=scored.units).astype(float)
        rows.append(flat(crop_checks(scored, per, edges, draw[scored.unit])))
        crops = len(hand.dets)
        hand_draw = np.bincount(rng.integers(0, crops, crops), minlength=crops).astype(float)
        hand_rows.append(box_fit(hand, hand_draw)["centre_error_median"])
    deviation = {key: round(float(np.std([row[key] for row in rows if row.get(key) is not None])), 4)
                 for key in rows[0] if isinstance(rows[0][key], float)}
    deviation["hand.centre_error_median"] = round(float(np.std([value for value in hand_rows if value is not None])), 4)
    return deviation


def crop_numbers(model, val, hand):
    """The model's crop numbers (mapped scores, boxes over the threshold), and what the spread needs."""
    val_crops, hand_crops = model.crops(val), model.crops(hand)
    edges = bands(model.threshold)
    per = per_crop(val_crops)
    numbers = crop_checks(val_crops, per, edges)
    numbers["band_boxes"] = band_counts(val_crops, edges)
    numbers["hand"] = dict(box_fit(hand_crops), targets=hand_crops.labels, found=int(hand_crops.hit.sum()),
                           false_boxes=int((~hand_crops.hit).sum()))
    numbers["bands"], numbers["at_threshold"] = edges, val_crops.at(model.threshold)
    return numbers, (val_crops, per, hand_crops, edges)


# ---- the limits ------------------------------------------------------------------------------------------------------
# Recordings (against full_v3 on the same recordings). full_v3 boxes the crosshair on two of the eight recordings: in
# nearly every turning pair of the valorant run (score about 0.6) and in 0.9% of 1wall 6targets 849.91's (scores 0.3 to
# 0.4); the review's crosshair-spot rule (src/matching.rs crosshair_spots) keeps such boxes from being taken for the
# killed target, but the tracking summary has no such rule. small_v10, which took KovaaK's crosshair for a target
# (MODEL_STATUS.md), boxes it in 14.9% of 1w4ts's turning pairs, where full_v3 boxes it in none. The review's rule
# engages only when such boxes pile up in 2% of the turning frames; below that it takes them for targets. So the pairs
# where a model boxes the crosshair (or the fixed map's HUD) and full_v3 does not, recording by recording, may be at most
# 0.5% of all the turning pairs, and on any one recording under 2% of its pairs (under 2 pairs, on a recording with
# fewer than 100).
SCREEN_ALL, SCREEN_ONE = 0.005, 0.02
# Crops (relative). No worse than full_v3 by more than 2 standard deviations of full_v3's own number over 400 draws of
# the val scenarios: the range full_v3's number keeps on 95% of the draws, so a model that fails is worse than full_v3
# by more than full_v3's own results can tell apart. Both models run on the same crops, so a model as good as full_v3
# lands well inside it. For shares (recall, precision, second boxes) the gap allowed is never under 1 point: full_v3
# finds nearly every kill-moment target (0.998, spread 0.001), and a 2-SD gap there is 4 labels in 2,500, which nothing
# downstream resolves (on the four stats-file recordings small_v13, at 0.995 there, matches all 496 kills, as full_v3
# does, and confirms 410 against 400).
SDS = 2.0
SHARE_FLOOR = 0.01
SHARES = ("one_box", "under_crosshair", "band_precision", "recall_by_kind")
BAND_BOXES = 50                     # a calibration band needs this many boxes for its precision to count

SCREEN_ONE_PAIRS = 2                # or under 2 pairs, on a recording with fewer than 100
VIDEO_CHARS = 48


def recording_rows(report, reference_rows, key):
    """Each recording's crosshair or screen_fixed share against full_v3's on it."""
    return [dict(video=row["video"][:VIDEO_CHARS], pairs=row["turning_pairs"], value=row[f"{key}_share"],
                 reference=ref[f"{key}_share"],
                 passed=bool(row[f"{key}_share"] - ref[f"{key}_share"]
                             < max(SCREEN_ONE, SCREEN_ONE_PAIRS / max(1, row["turning_pairs"]))))
            for row, ref in zip(report, reference_rows)]


def screen_check(rows, reference_rows, key):
    """The crosshair or screen_fixed check: the pairs more than full_v3's, recording by recording (fewer on one
    recording does not make up for more on another), over all the pairs."""
    def share(recorded):
        return sum(row[f"{key}_pairs"] for row in recorded) / max(1, sum(row["turning_pairs"] for row in recorded))
    per = recording_rows(rows, reference_rows, key)
    more = sum(max(0.0, row[f"{key}_share"] - ref[f"{key}_share"]) * row["turning_pairs"]
               for row, ref in zip(rows, reference_rows)) / max(1, sum(row["turning_pairs"] for row in rows))
    return dict(value=round(share(rows), 4), reference=round(share(reference_rows), 4),
                more_than_reference=round(more, 4), allowed_gap=SCREEN_ALL, allowed_gap_one_recording=SCREEN_ONE,
                recordings=per, passed=bool(more <= SCREEN_ALL and all(row["passed"] for row in per)))


def relative(flat_reference, spread_of, key, value, worse):
    """value against full_v3's: worse +1 when larger is worse, -1 when smaller is, 0 either way."""
    reference = flat_reference[key]
    gap = max(SDS * spread_of[key], SHARE_FLOOR if key.split(".")[0] in SHARES else 0.0)
    if value is None:
        return dict(value=None, reference=reference, allowed_gap=gap, passed=False)
    difference = value - reference if worse > 0 else reference - value if worse < 0 else abs(value - reference)
    return dict(value=round(value, 4), reference=round(reference, 4), allowed_gap=round(gap, 4),
                passed=bool(difference <= gap))


def judge(rep, ref, sd, ref_rec):
    """Pass or fail per check: `ref` holds full_v3's numbers on the same crops, `sd` their spread, `ref_rec` its
    results on the same recordings."""
    out = {key: screen_check(rep["recordings"], ref_rec, key) for key in ("crosshair", "screen_fixed")}
    most = max(row["boxes_max"] for row in rep["recordings"])
    out["boxes_per_frame"] = dict(value=most, limit=MAX_BOXES, passed=most <= MAX_BOXES)
    flat_reference = flat(ref)

    def rel(key, value, worse):
        return relative(flat_reference, sd, key, value, worse)

    fit = rep["box_fit"]
    rows = {key: rel(f"box_fit.{key}", fit[key], worse)
            for key, worse in (("centre_error_median", 1), ("centre_error_p90", 1), ("width_ratio_median", 0),
                               ("height_ratio_median", 0))}
    rows["hand_centre_error_median"] = rel("hand.centre_error_median", rep["hand"]["centre_error_median"], 1)
    out["box_fit"] = dict(rows, passed=all(row["passed"] for row in rows.values()))
    out["one_box"] = rel("one_box", rep["one_box"], 1)
    out["under_crosshair"] = rel("under_crosshair", rep["under_crosshair"], -1)
    bands_rows = []
    for k, (low, high) in enumerate(zip(rep["bands"][:-1], rep["bands"][1:])):
        row = dict(band=[low, high], **rel(f"band_precision.{k}", rep["band_precision"][k], 0),
                   boxes=rep["band_boxes"][k], reference_boxes=ref["band_boxes"][k])
        row["passed"] = row["passed"] and row["boxes"] >= BAND_BOXES
        bands_rows.append(row)
    out["calibrated"] = dict(bands=bands_rows, passed=all(row["passed"] for row in bands_rows))
    rows = {kind: rel(f"recall_by_kind.{kind}", rep["recall_by_kind"][kind], -1) for kind in KINDS}
    out["targets_found"] = dict(rows, passed=all(row["passed"] for row in rows.values()))
    return out


def crop_part(model, reference):
    """The crop checks' numbers for the model and full_v3, and full_v3's spread."""
    files, hand_files = calibrate.crop_files(calibrate.VAL, "val"), calibrate.crop_files(HAND, "test")
    val = calibrate.Scored(*calibrate.peaks(model.session, files), files)
    hand = calibrate.Scored(*calibrate.peaks(model.session, hand_files), hand_files)
    numbers, (_, _, _, edges) = crop_numbers(model, val, hand)
    if reference is not model:
        val = calibrate.Scored(*calibrate.peaks(reference.session, files), files)
        hand = calibrate.Scored(*calibrate.peaks(reference.session, hand_files), hand_files)
    ref, (_, _, ref_hand, _) = crop_numbers(reference, val, hand)
    # full_v3's bands are those of the model's threshold, from the lower of the two thresholds
    ref_crops = reference.crops(val, min(model.threshold, reference.threshold))
    ref_per = per_crop(ref_crops)
    ref["band_precision"] = crop_checks(ref_crops, ref_per, edges)["band_precision"]
    ref["band_boxes"] = band_counts(ref_crops, edges)
    return numbers, ref, spread(ref_crops, ref_per, ref_hand, edges)


def recording_part(model, reference, videos):
    """The recording checks' rows for the model and full_v3, each printed as it is done."""
    rows, ref_rows = [], []
    for video in videos:
        row = recording(model, video)
        rows.append(row)
        ref_rows.append(row if reference is model else recording(reference, video))
        print(f"  {row['video'][:50]}: {row['turning_pairs']} turning pairs, crosshair {row['crosshair_share']:.2%} "
              f"(full_v3 {ref_rows[-1]['crosshair_share']:.2%}), elsewhere {row['screen_fixed_share']:.2%}, "
              f"boxes at most {row['boxes_max']}", flush=True)
    return rows, ref_rows


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("model")
    parser.add_argument("--settings", help="a settings file or calibration report [exports/detector_<name>.json beside "
                        "the export]")
    parser.add_argument("--report", help="the JSON report [python/model/reports/contract_<name>.json]")
    parser.add_argument("--vods", nargs="*", help="static recordings for the screen checks [eval_moving.py's static "
                        "ones]")
    args = parser.parse_args()
    name = calibrate.model_name(args.model)
    settings, source = load_settings(args.model, args.settings)
    model = Model(args.model, settings)
    reference = model if name == infer.BEST else Model(infer.BEST, load_settings(infer.BEST)[0])
    rep = dict(model=name, export_file=str(model.path), settings=settings, settings_from=source)
    print(f"{name}: settings from {source}: threshold {model.threshold}, score map {model.score_map}", flush=True)
    rep["export"] = check_export(model, settings, name)
    export = rep["export"]
    print(f"  export: {'pass' if export['passed'] else 'FAIL: ' + '; '.join(export['problems'])}", flush=True)
    numbers, ref, sd = crop_part(model, reference)
    rep.update(numbers)
    print("  crops done", flush=True)
    rep["recordings"], ref_rec = recording_part(model, reference, args.vods or recordings())
    rep["reference"] = dict(model=infer.BEST, **ref, spread=sd,
                            recordings=[{key: row[key] for key in ("video", "turning_pairs", "crosshair_share",
                                                                     "screen_fixed_share", "boxes_max")}
                                        for row in ref_rec])
    rep["checks"] = dict(export=dict(passed=export["passed"], problems=export["problems"]),
                         **judge(rep, ref, sd, ref_rec))
    rep["passed"] = all(check["passed"] for check in rep["checks"].values())
    out = Path(args.report) if args.report else REPORTS / f"contract_{name}.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    json.dump(rep, open(out, "w"), indent=1, default=float)
    for key, check in rep["checks"].items():
        print(f"  {key}: {'pass' if check['passed'] else 'FAIL'}")
    print(f"{name}: {'meets the contract' if rep['passed'] else 'FAILS the contract'}; report {out}")
    sys.exit(0 if rep["passed"] else 1)


if __name__ == "__main__":
    main()
