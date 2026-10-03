"""The contract a detector model must meet before the review uses it (REPRODUCE.md, "After a training run").

The review (src/, python/review.py) relies on things about the detector's output that no training metric states. Each
check measures one of them on the model's _u8in export (the graph the app runs) with its settings file
(exports/detector_<name>.json: the threshold and score map; calibrate.py's output when there is no file yet), and
compares it with full_v3's results on the same data (the limits and their reasons are at "the limits" below):

  export           the export's form, which src/detect.rs, the service and the browser assume: inputs "rgb" uint8
                   (n, h, w, 3) and "fixed" uint8 (n, h, w) with a free batch axis (they send 4 frames at once), outputs
                   "score" (n, 1, h/4, w/4) holding only the peaks, from 0 to 1, and "reg" (n, 4, h/4, w/4), finite; a
                   frame in a batch of 4 gives what it gives alone; a settings file the pipeline accepts; the fixed map
                   (the model's 4th input) made as the training crops' was (DIFF 30, SHARE 0.8, in src/fixed.rs and
                   python/review.py alike). Pass or fail, no limit.
  crosshair        the crosshair is not a target. On the static recordings of eval_moving.py: the frames where the
                   room moved 0.5 degrees or more since the frame before, and at least half that either side (the
                   camera's turn, read from the video by review.camera_motion; a one-frame jump is a shot's flash), with
                   a box on the fixed map's crosshair that stayed put (moved under half the room's move, and no box of
                   the frame before moved there with the room). A static target moves with the room; only something
                   fixed to the screen stays. A frame where most boxes stayed put is left out: the reading is wrong.
  screen_fixed     the same on the fixed map's other parts in the area the review reads: the HUD and overlay are left
                   out. (A box elsewhere that seems to stay put is mostly a tiled wall's seam meeting another seam's
                   place; that is the wall, not the screen, and the acceptance checks judge it.)
  boxes_per_frame  at most 50 boxes in a frame on those recordings: link() compares every pair of boxes in two frames,
                   up to 2,500.
  box_fit          the boxes fit the targets: centre error (px), and width and height against the val labels (the
                   target's area, pi/4 x w x h, gives the review's target radius; the box, its on-target test in
                   tracking); centre error against the held-out hand labels (the user's clicks: true ground truth).
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
import review  # noqa: E402

REPORTS = HERE / "reports"
CACHE = Path("test_out/vod_model/contract")          # camera readings, fixed maps, and each export's peaks
HAND = ("test_out/vod_model/hand_data", "test_out/vod_model/hand_data2")   # their test splits: no model trained on them
KILLS = "data_kills4"                                # the kill-moment crops, among calibrate.VAL
TURN = 0.5                                           # degrees the room moves between two frames for a turning pair
NEAR = 24                                            # px from the crosshair's centre where its fixed-map parts start
MAX_BOXES = 50                                       # link(): 2,500 pairings at most
BANDS = (0.4, 0.5, 0.6, 0.7, 0.8)                    # the calibration's bands over the threshold (and 1.0)
DRAWS = 400                                          # draws of the val scenarios for full_v3's spread


def recordings():
    """The static recordings of eval_moving.py: the four eval_vods.py runs, the valorant run and three 1wall 6targets
    extra small runs."""
    import eval_moving
    return list(eval_moving.STATIC)


# ---- the model with its settings -------------------------------------------------------------------------------------
def load_settings(model, given=None):
    """The model's settings: the file given, else exports/detector_<name>.json beside its export, else what
    calibrate.py gives (marked as such)."""
    name = calibrate.model_name(model)
    p = Path(given) if given else calibrate.u8in_of(model).parent / f"detector_{name}.json"
    if p.is_file():
        s = json.loads(p.read_text())
        return s.get("settings", s), str(p)                # a calibration report holds them under "settings"
    return calibrate.calibrate(model)[0], "calibrate.py (no settings file yet)"


def settings_problems(s, name):
    """What the pipeline would refuse in a settings file (src/model.rs, MODEL_FILE.md), or a wrong name or reference."""
    bad = []
    if s.get("format") != 1:
        bad.append("format is not 1")
    if s.get("name") != name:
        bad.append(f"name {s.get('name')!r} is not {name!r}")
    if s.get("reference") != infer.BEST:
        bad.append(f"reference {s.get('reference')!r} is not {infer.BEST!r}")
    t = s.get("threshold")
    if not isinstance(t, (int, float)) or not 0 <= t <= 1:
        bad.append("threshold is not a number from 0 to 1")
    m = s.get("score_map")
    if m is not None:
        a = np.array(m, np.float64) if isinstance(m, list) else np.zeros((0, 0))
        if a.ndim != 2 or a.shape[0] < 2 or a.shape[1] != 2:
            bad.append("score_map is not 2 or more [raw, mapped] points")
        elif (a < 0).any() or (a > 1).any() or (np.diff(a, axis=0) <= 0).any():
            bad.append("score_map has a value outside 0 to 1, or a raw or mapped value that does not rise")
    return bad


class Model:
    """The _u8in export with its settings: boxes (cx, cy, w, h, mapped score) over the threshold."""

    def __init__(self, model, s):
        self.path = calibrate.u8in_of(model)
        self.sess = calibrate.session(self.path)
        self.thr, self.map = float(s["threshold"]), s["score_map"]

    def mapped(self, d, thr=None):
        """Peaks with their scores mapped, over the threshold (or `thr`)."""
        d = d.copy()
        d[:, 4] = calibrate.apply_map(self.map, d[:, 4]).astype(np.float32)
        return d[d[:, 4] > (self.thr if thr is None else thr)]

    def raw(self, rgb, fixed):
        """rgb (n, 720, 1280, 3) uint8 and one fixed map (720, 1280) -> per frame, its peaks over calibrate.FLOOR with
        their raw scores."""
        if len(rgb) > 1 and self.sess.get_inputs()[0].shape[0] == 1:      # an export traced for one frame
            return [d for f in rgb for d in self.raw(f[None], fixed)]
        n = len(rgb)
        score, reg = self.sess.run(None, {"rgb": rgb, "fixed": np.broadcast_to(fixed, (n, *fixed.shape)).copy()})
        return [infer.decode_np(score[k:k + 1], reg[k:k + 1], calibrate.FLOOR) for k in range(n)]

    def crops(self, scored, thr=None):
        """A calibrate.Scored's crops with the mapped scores, only the boxes over the threshold (or `thr`), as a new
        Scored."""
        return calibrate.Scored([self.mapped(d, thr) for d in scored.dets], scored.gts, scored.files)


# ---- 1. the export's form -------------------------------------------------------------------------------------------
def check_export(m, s, name):
    sess = m.sess
    ins = {i.name: i for i in sess.get_inputs()}
    outs = {o.name: o for o in sess.get_outputs()}
    bad = settings_problems(s, name)
    if set(ins) != {"rgb", "fixed"} or set(outs) != {"score", "reg"}:
        bad.append(f"inputs {sorted(ins)} and outputs {sorted(outs)}: want rgb, fixed and score, reg")
        return dict(passed=False, problems=bad)
    if ins["rgb"].type != "tensor(uint8)" or ins["fixed"].type != "tensor(uint8)" or len(ins["rgb"].shape) != 4 \
            or len(ins["fixed"].shape) != 3:
        bad.append("rgb and fixed are not uint8 (n, h, w, 3) and (n, h, w)")
    if isinstance(ins["rgb"].shape[0], int):
        bad.append(f"the batch axis is fixed at {ins['rgb'].shape[0]} (export.py --u8in gives a free one)")
    import bench
    rgb, fixed = bench.sample()                            # a real 1280 x 720 KovOBS frame
    score, reg = sess.run(None, {"rgb": rgb[None], "fixed": fixed[None].astype(np.uint8)})
    if score.shape != (1, 1, 180, 320) or reg.shape != (1, 4, 180, 320):
        bad.append(f"outputs {score.shape} and {reg.shape} for a 720 x 1280 frame: want cells of 4 px")
    if not (np.isfinite(score).all() and np.isfinite(reg).all()) or score.min() < 0 or score.max() > 1:
        bad.append("scores outside 0 to 1, or values that are not finite")
    s0 = score[0, 0]
    pad = np.pad(s0, 1)
    around = np.max([pad[1 + dy:181 + dy, 1 + dx:321 + dx] for dy in (-1, 0, 1) for dx in (-1, 0, 1)], axis=0)
    if ((s0 > 0) & (s0 < around)).any():
        bad.append("the score map holds cells that are not peaks (the 3 x 3 peak finding is not in the graph)")
    batch = None
    if not isinstance(ins["rgb"].shape[0], int):
        four = np.stack([np.roll(rgb, 37 * k, axis=1) if k != 1 else rgb for k in range(4)])
        sb, rb = sess.run(None, {"rgb": four, "fixed": np.repeat(fixed[None].astype(np.uint8), 4, axis=0)})
        batch = float(max(np.abs(sb[1] - score[0]).max(), np.abs(rb[1] - reg[0]).max()))
        if batch > 1e-4:
            bad.append(f"a frame in a batch of 4 differs from the same frame alone by {batch:.1e}")
    src = (ROOT / "src" / "fixed.rs").read_text(encoding="utf-8")
    diff = re.search(r"pub const DIFF: f32 = ([\d.]+);", src)
    share = re.search(r"pub const SHARE: f64 = ([\d.]+);", src)
    fixed_map = dict(core_diff=float(diff[1]) if diff else None, core_share=float(share[1]) if share else None,
                     python_diff=review.DIFF)
    if fixed_map["core_diff"] != 30 or fixed_map["core_share"] != 0.8 or review.DIFF != 30:
        bad.append(f"the fixed map is not made as the training crops' was (DIFF 30, SHARE 0.8): {fixed_map}")
    return dict(passed=not bad, problems=bad, batch_of_4_largest_difference=batch, fixed_map=fixed_map,
                peaks_on_sample_frame=int((s0 > m.thr).sum()))


# ---- 2 to 4. recordings: the crosshair, other screen-fixed boxes, boxes per frame -----------------------------------
def camera(video):
    """The recording's fixed map (review.fixed_map) and the room's move on screen per frame (review.camera_motion,
    degrees; NaN where it has no reading), cached in test_out/vod_model/contract/."""
    st = Path(video).stat()
    p = CACHE / f"{hashlib.md5(str(video).encode()).hexdigest()[:10]}.npz"
    if p.is_file():
        z = np.load(p)
        if int(z["size"]) == st.st_size and float(z["mtime"]) == st.st_mtime:
            return z["fixed"], z["room"]
    fixed = review.fixed_map(list(review._frames(video, keyframes=True)))
    cam = review.camera_motion(video, [], mask=review.MASK, fixed=fixed)
    room = np.array([(c[0], c[1]) if c else (np.nan, np.nan) for c in cam], np.float64).reshape(-1, 2)
    CACHE.mkdir(parents=True, exist_ok=True)
    np.savez_compressed(p, fixed=fixed, room=room, size=st.st_size, mtime=st.st_mtime)
    return fixed, room


def crosshair_area(fixed):
    """Where the crosshair is: the fixed map's parts within NEAR px of the crosshair's centre, whole, grown by 2 px,
    and a disc of 3 px at the centre (in case the fixed map misses a dot)."""
    lab, _ = ndimage.label(fixed)
    cx, cy = int(round(review.CX)), int(round(review.CY))
    ids = np.unique(lab[cy - NEAR:cy + NEAR + 1, cx - NEAR:cx + NEAR + 1])
    area = np.isin(lab, ids[ids > 0])
    yy, xx = np.mgrid[0:review.H, 0:review.W]
    return ndimage.binary_dilation(area, iterations=2) | ((xx - review.CX) ** 2 + (yy - review.CY) ** 2 <= 9)


def peaks_on(m, video, need, batch=4):
    """The model's raw peaks on the frames `need` of a recording, cached in test_out/vod_model/contract/ per export
    (its size and time) and recording."""
    st, ex = Path(video).stat(), m.path.stat()
    p = CACHE / f"peaks_{m.path.stem}_{hashlib.md5(str(video).encode()).hexdigest()[:10]}.npz"
    key = np.array([st.st_size, st.st_mtime, ex.st_size, ex.st_mtime])
    if p.is_file():
        z = np.load(p)
        if np.array_equal(z["key"], key) and need <= set(z["frames"].tolist()):
            rows = z["rows"]
            return {int(i): rows[rows[:, 0] == i, 1:] for i in z["frames"]}
    dets, buf, idx = {}, [], []
    fixed = camera(video)[0].astype(np.uint8)
    for i, f in enumerate(review.rgb_frames(video)):
        if i in need:
            buf.append(f)
            idx.append(i)
        if len(buf) == batch:
            dets.update(zip(idx, m.raw(np.stack(buf), fixed)))
            buf, idx = [], []
    if buf:
        dets.update(zip(idx, m.raw(np.stack(buf), fixed)))
    rows = np.concatenate([np.c_[np.full(len(d), i), d] for i, d in dets.items()] + [np.zeros((0, 6))])
    CACHE.mkdir(parents=True, exist_ok=True)
    np.savez_compressed(p, key=key, frames=np.array(sorted(dets)), rows=rows)
    return dets


def moved_with_room(prev, room):
    """Where the boxes of the frame before would be now if they were static targets: moved by the room's turn."""
    out = [review.to_px(*(np.array(review.to_deg(float(b[0]), float(b[1]))) + room)) for b in prev]
    return np.array(out, np.float64).reshape(-1, 2)


def recording(m, video):
    fixed, room = camera(video)
    deg = np.hypot(room[:, 0], room[:, 1])
    # a turn, not a one-frame jump in the reading (a shot's flash can give one): half as much either side
    pairs = [i for i in range(1, len(deg) - 1) if deg[i] >= TURN and deg[i - 1] >= TURN / 2 and deg[i + 1] >= TURN / 2]
    raw = peaks_on(m, video, set(pairs) | {i - 1 for i in pairs})
    cross = crosshair_area(fixed)
    hud = ndimage.binary_dilation(fixed, iterations=2) & ~cross     # the fixed map's other parts: HUD, overlay

    def boxes(i):                                      # over the threshold, centre where the review reads (KovOBS's
        d = m.mapped(raw[i])                           # layout)
        return d[[bool(review.MASK[min(review.H - 1, max(0, int(b[1]))), min(review.W - 1, max(0, int(b[0])))])
                  for b in d]] if len(d) else d

    on_cross = elsewhere = any_cross = wrong = 0
    examples = []
    for i in pairs:
        now, prev = boxes(i), boxes(i - 1)
        if not len(now):
            continue
        x, y = review.to_px(*room[i])
        tol = math.hypot(x - review.CX, y - review.CY) / 2     # half the room's move on screen
        moved = moved_with_room(prev, room[i])
        fixed_box = []                                 # stayed put, and no static target of the frame before lands there
        for b in now:
            stay = len(prev) and np.hypot(prev[:, 0] - b[0], prev[:, 1] - b[1]).min() < tol
            static = len(prev) and np.hypot(moved[:, 0] - b[0], moved[:, 1] - b[1]).min() < tol
            fixed_box.append(bool(stay and not static))
        at = [bool(cross[min(review.H - 1, int(b[1])), min(review.W - 1, int(b[0]))]) for b in now]
        others = [f for f, a in zip(fixed_box, at) if not a]
        if others and sum(others) > len(others) / 2:   # most targets stayed put: the camera's reading is wrong here
            wrong += 1
            continue
        on_hud = [bool(hud[min(review.H - 1, int(b[1])), min(review.W - 1, int(b[0]))]) for b in now]
        on_cross += any(f and a for f, a in zip(fixed_box, at))
        elsewhere += any(f and h for f, h in zip(fixed_box, on_hud))
        any_cross += any(at)
        if any(f and (a or h) for f, a, h in zip(fixed_box, at, on_hud)) and len(examples) < 5:
            examples.append(dict(frame=i, room_deg=[round(float(v), 3) for v in room[i]],
                                 boxes=[[round(float(v), 2) for v in b] for b, f, a, h in zip(now, fixed_box, at, on_hud)
                                        if f and (a or h)]))
    counts = np.array([len(boxes(i)) for i in raw])
    n = len(pairs) - wrong
    return dict(video=Path(video).name, frames=len(deg), turning_pairs=n, camera_contradicted=wrong,
                crosshair_pixels=int(cross.sum()),
                crosshair_pairs=on_cross, crosshair_share=round(on_cross / max(1, n), 4),
                screen_fixed_pairs=elsewhere, screen_fixed_share=round(elsewhere / max(1, n), 4),
                any_box_on_crosshair_share=round(any_cross / max(1, n), 4),
                boxes_max=int(counts.max()) if len(counts) else 0,
                boxes_p99=float(np.percentile(counts, 99)) if len(counts) else 0.0, screen_fixed_examples=examples)


# ---- 5 to 10. crops -------------------------------------------------------------------------------------------------
# Each number can be weighted per crop (`cw`): a draw of the scenarios weights each crop by how often its scenario was
# drawn, which gives full_v3's spread.
def wq(v, w, p):
    """The weighted p-th percentile of the values v (NaN left out)."""
    ok = ~np.isnan(v) & (w > 0)
    if not ok.any():
        return None
    v, w = v[ok], w[ok]
    o = np.argsort(v)
    c = np.cumsum(w[o])
    return float(v[o][min(len(v) - 1, np.searchsorted(c, p / 100 * c[-1]))])


def box_fit(sc, cw=None):
    """Over the threshold, the boxes that took a label: centre error (px), IoU, and width and height over the
    label's."""
    w = (np.ones(len(sc.dets)) if cw is None else cw)[sc.crop] * sc.hit

    def q(v, p):
        x = wq(v, w, p)
        return None if x is None else round(x, 4)
    return dict(boxes=int(sc.hit.sum()), centre_error_median=q(sc.err, 50), centre_error_p90=q(sc.err, 90),
                iou_median=q(sc.iou, 50), iou_p10=q(sc.iou, 10), width_ratio_median=q(sc.w_ratio, 50),
                height_ratio_median=q(sc.h_ratio, 50))


def per_crop(sc):
    """Per crop: targets found, second boxes (boxes that took no label with their centre inside a label another box
    took), and on the kill-moment crops the labels under the crosshair (touching the fixed map) and how many of those
    were found; and the crop's kind (KINDS)."""
    found, extra, under, under_found = (np.zeros(len(sc.dets)) for _ in range(4))
    for c, (d, g, f) in enumerate(zip(sc.dets, sc.gts, sc.files)):
        took = calibrate.match(d, g)
        found[c] = (took >= 0).sum()
        taken = g[took[took >= 0]]
        for b in d[took < 0]:
            extra[c] += bool(len(taken) and ((np.abs(taken[:, 0] - b[0]) <= taken[:, 2] / 2) &
                                             (np.abs(taken[:, 1] - b[1]) <= taken[:, 3] / 2)).any())
        if KILLS in str(f) and len(g):
            fixed = np.load(f)["fixed"]
            for j, b in enumerate(g):
                x0, x1 = int(max(0, b[0] - b[2] / 2)), int(min(256, math.ceil(b[0] + b[2] / 2)))
                y0, y1 = int(max(0, b[1] - b[3] / 2)), int(min(256, math.ceil(b[1] + b[3] / 2)))
                if fixed[y0:y1, x0:x1].any():
                    under[c] += 1
                    under_found[c] += j in took
    return dict(found=found, extra=extra, under=under, under_found=under_found,
                kind=np.array([1 if KILLS in str(f) else 2 if "moving" in str(f) else 0 for f in sc.files]))


KINDS = ("static", "kill_moments", "moving")


def bands(thr):
    return [thr] + [b for b in BANDS if b > thr] + [1.0]


def crop_checks(sc, pc, edges, cw=None):
    """The val numbers of the checks box_fit, one_box, under_crosshair, calibrated and targets_found."""
    cw = np.ones(len(sc.dets)) if cw is None else cw

    def ratio(a, b):
        return float((cw * a).sum() / max(1e-9, (cw * b).sum()))
    w = cw[sc.crop]
    precision = []
    for a, b in zip(edges[:-1], edges[1:]):
        m = (sc.score > a) & (sc.score <= b)
        precision.append(float((w * m * sc.hit).sum() / max(1e-9, (w * m).sum())))
    return dict(box_fit=box_fit(sc, cw), one_box=ratio(pc["extra"], pc["found"]),
                under_crosshair=ratio(pc["under_found"], pc["under"]), band_precision=precision,
                recall_by_kind={k: ratio(pc["found"] * (pc["kind"] == i), sc.crop_labels * (pc["kind"] == i))
                                for i, k in enumerate(KINDS)})


def flat(d, pre=""):
    """A nested dict of numbers as {"a.b": value} (a list's items by index)."""
    out = {}
    for k, v in (d.items() if isinstance(d, dict) else enumerate(d)):
        if isinstance(v, (dict, list)):
            out.update(flat(v, f"{pre}{k}."))
        else:
            out[f"{pre}{k}"] = v
    return out


def spread(sc, pc, hand, edges, draws=DRAWS, seed=0):
    """full_v3's standard deviation over draws of the val scenarios (for the hand crops: draws of the crops) of each
    number with a relative limit."""
    rng = np.random.default_rng(seed)
    rows, hrows = [], []
    for _ in range(draws):
        draw = np.bincount(rng.integers(0, sc.units, sc.units), minlength=sc.units).astype(float)
        rows.append(flat(crop_checks(sc, pc, edges, draw[sc.unit])))
        hdraw = np.bincount(rng.integers(0, len(hand.dets), len(hand.dets)), minlength=len(hand.dets)).astype(float)
        hrows.append(box_fit(hand, hdraw)["centre_error_median"])
    sd = {k: round(float(np.std([r[k] for r in rows if r.get(k) is not None])), 4)
          for k in rows[0] if isinstance(rows[0][k], float)}
    sd["hand.centre_error_median"] = round(float(np.std([v for v in hrows if v is not None])), 4)
    return sd


def crop_numbers(m, val, hand):
    """The model's crop numbers (mapped scores, boxes over the threshold), and what the spread needs."""
    v, hd = m.crops(val), m.crops(hand)
    edges = bands(m.thr)
    pc = per_crop(v)
    nums = crop_checks(v, pc, edges)
    nums["band_boxes"] = [int(((v.score > a) & (v.score <= b)).sum()) for a, b in zip(edges[:-1], edges[1:])]
    nums["hand"] = dict(box_fit(hd), targets=hd.labels, found=int(hd.hit.sum()), false_boxes=int((~hd.hit).sum()))
    nums["bands"], nums["at_threshold"] = edges, v.at(m.thr)
    return nums, (v, pc, hd, edges)


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


def judge(rep, ref, sd, ref_rec):
    """Pass or fail per check: `ref` holds full_v3's numbers on the same crops, `sd` their spread, `ref_rec` its
    results on the same recordings."""
    out = {}
    rec = rep["recordings"]

    def share(rows, key):
        return sum(r[f"{key}_pairs"] for r in rows) / max(1, sum(r["turning_pairs"] for r in rows))
    for key in ("crosshair", "screen_fixed"):
        per = [dict(video=r["video"][:48], pairs=r["turning_pairs"], value=r[f"{key}_share"],
                    reference=f[f"{key}_share"], passed=bool(r[f"{key}_share"] - f[f"{key}_share"] <
                                                             max(SCREEN_ONE, 2 / max(1, r["turning_pairs"]))))
               for r, f in zip(rec, ref_rec)]
        # the pairs more than full_v3's, recording by recording (fewer on one recording does not make up for more on
        # another), over all the pairs
        more = sum(max(0.0, r[f"{key}_share"] - f[f"{key}_share"]) * r["turning_pairs"]
                   for r, f in zip(rec, ref_rec)) / max(1, sum(r["turning_pairs"] for r in rec))
        out[key] = dict(value=round(share(rec, key), 4), reference=round(share(ref_rec, key), 4),
                        more_than_reference=round(more, 4), allowed_gap=SCREEN_ALL,
                        allowed_gap_one_recording=SCREEN_ONE, recordings=per,
                        passed=bool(more <= SCREEN_ALL and all(p["passed"] for p in per)))
    most = max(r["boxes_max"] for r in rec)
    out["boxes_per_frame"] = dict(value=most, limit=MAX_BOXES, passed=most <= MAX_BOXES)
    fr = flat(ref)

    def rel(key, value, worse):
        """value against full_v3's: worse +1 when larger is worse, -1 when smaller is, 0 either way."""
        r, gap = fr[key], max(SDS * sd[key], SHARE_FLOOR if key.split(".")[0] in SHARES else 0.0)
        if value is None:
            return dict(value=None, reference=r, allowed_gap=gap, passed=False)
        d = value - r if worse > 0 else r - value if worse < 0 else abs(value - r)
        return dict(value=round(value, 4), reference=round(r, 4), allowed_gap=round(gap, 4), passed=bool(d <= gap))

    v = rep["box_fit"]
    rows = {k: rel(f"box_fit.{k}", v[k], w) for k, w in (("centre_error_median", 1), ("centre_error_p90", 1),
                                                          ("width_ratio_median", 0), ("height_ratio_median", 0))}
    rows["hand_centre_error_median"] = rel("hand.centre_error_median", rep["hand"]["centre_error_median"], 1)
    out["box_fit"] = dict(rows, passed=all(x["passed"] for x in rows.values()))
    out["one_box"] = rel("one_box", rep["one_box"], 1)
    out["under_crosshair"] = rel("under_crosshair", rep["under_crosshair"], -1)
    rows = []
    for k, (a, b) in enumerate(zip(rep["bands"][:-1], rep["bands"][1:])):
        x = dict(band=[a, b], **rel(f"band_precision.{k}", rep["band_precision"][k], 0), boxes=rep["band_boxes"][k],
                 reference_boxes=ref["band_boxes"][k])
        x["passed"] = x["passed"] and x["boxes"] >= BAND_BOXES
        rows.append(x)
    out["calibrated"] = dict(bands=rows, passed=all(x["passed"] for x in rows))
    rows = {k: rel(f"recall_by_kind.{k}", rep["recall_by_kind"][k], -1) for k in KINDS}
    out["targets_found"] = dict(rows, passed=all(x["passed"] for x in rows.values()))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("model")
    ap.add_argument("--settings", help="a settings file or calibration report [exports/detector_<name>.json beside "
                    "the export]")
    ap.add_argument("--report", help="the JSON report [python/model/reports/contract_<name>.json]")
    ap.add_argument("--vods", nargs="*", help="static recordings for the screen checks [eval_moving.py's static ones]")
    a = ap.parse_args()
    name = calibrate.model_name(a.model)
    s, source = load_settings(a.model, a.settings)
    m = Model(a.model, s)
    ref_m = m if name == infer.BEST else Model(infer.BEST, load_settings(infer.BEST)[0])
    rep = dict(model=name, export_file=str(m.path), settings=s, settings_from=source)
    print(f"{name}: settings from {source}: threshold {m.thr}, score map {m.map}", flush=True)
    rep["export"] = check_export(m, s, name)
    print(f"  export: {'pass' if rep['export']['passed'] else 'FAIL: ' + '; '.join(rep['export']['problems'])}", flush=True)

    files, hand_files = calibrate.crop_files(calibrate.VAL, "val"), calibrate.crop_files(HAND, "test")
    val = calibrate.Scored(*calibrate.peaks(m.sess, files), files)
    hand = calibrate.Scored(*calibrate.peaks(m.sess, hand_files), hand_files)
    nums, (_, _, _, edges) = crop_numbers(m, val, hand)
    rep.update(nums)
    if ref_m is not m:
        val = calibrate.Scored(*calibrate.peaks(ref_m.sess, files), files)
        hand = calibrate.Scored(*calibrate.peaks(ref_m.sess, hand_files), hand_files)
    ref, (_, _, rhand, _) = crop_numbers(ref_m, val, hand)
    # full_v3's bands are those of the model's threshold, from the lower of the two thresholds
    rb = ref_m.crops(val, min(m.thr, ref_m.thr))
    rpc = per_crop(rb)
    ref["band_precision"] = crop_checks(rb, rpc, edges)["band_precision"]
    ref["band_boxes"] = [int(((rb.score > a) & (rb.score <= b)).sum()) for a, b in zip(edges[:-1], edges[1:])]
    sd = spread(rb, rpc, rhand, edges)
    print("  crops done", flush=True)

    rep["recordings"], ref_rec = [], []
    for v in a.vods or recordings():
        r = recording(m, v)
        rep["recordings"].append(r)
        ref_rec.append(r if ref_m is m else recording(ref_m, v))
        print(f"  {r['video'][:50]}: {r['turning_pairs']} turning pairs, crosshair {r['crosshair_share']:.2%} "
              f"(full_v3 {ref_rec[-1]['crosshair_share']:.2%}), elsewhere {r['screen_fixed_share']:.2%}, "
              f"boxes at most {r['boxes_max']}", flush=True)
    rep["reference"] = dict(model=infer.BEST, **ref, spread=sd,
                            recordings=[{k: x[k] for k in ("video", "turning_pairs", "crosshair_share",
                                                           "screen_fixed_share", "boxes_max")} for x in ref_rec])
    rep["checks"] = dict(export=dict(passed=rep["export"]["passed"], problems=rep["export"]["problems"]),
                         **judge(rep, ref, sd, ref_rec))
    rep["passed"] = all(c["passed"] for c in rep["checks"].values())
    out = Path(a.report) if a.report else REPORTS / f"contract_{name}.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    json.dump(rep, open(out, "w"), indent=1, default=float)
    for k, c in rep["checks"].items():
        print(f"  {k}: {'pass' if c['passed'] else 'FAIL'}")
    print(f"{name}: {'meets the contract' if rep['passed'] else 'FAILS the contract'}; report {out}")
    sys.exit(0 if rep["passed"] else 1)


if __name__ == "__main__":
    main()
