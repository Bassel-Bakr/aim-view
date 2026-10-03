"""Put a detector's scores on the reference model's scale, and pick its threshold there (REPRODUCE.md, "After a
training run"). export.py calls it after an export.

The review keeps a cell as a target when its score passes a threshold, and reads scores elsewhere too (one box past the
scenario's target count needs 0.5; the faint-target cut-off compares tracks' scores). Those numbers were set on the
reference model, full_v3 (infer.BEST). Each model carries a settings file, python/model/exports/detector_<name>.json:
  {"format": 1, "name": ..., "threshold": 0.3, "score_map": null | [[raw, mapped], ...], "reference": "full_v3"}
score_map takes the model's raw score onto the reference's scale (piecewise linear, rising in both, clamped at the
ends); the threshold is on that scale.

The map: the same score means the same precision. On the validation crops (the val splits the models were picked on;
no model trained on them), every peak of the score map is matched to the labelled targets as eval.py matches them, and
each model's precision at each score (the share of its peaks with that score that are targets) is fitted rising with the
score (isotonic regression on 0.01-wide bins). A raw score maps to the reference score with the same fitted precision.
The fit is noisy where few peaks score (about 6% of them score from 0.3 to 0.6), and crops of one scenario look alike,
so it is redone on 400 draws of the scenarios (a bootstrap by scenario folder, both models on the same draw). A score
moves only as far as the 95% range of those draws demands: where that range holds the score itself, it stays. Moves
under 0.01 are dropped, and a model with none left needs no map (null).

Why not match the scores of true targets (the same recall at the same score)? Because a model that finds fewer targets
is then pushed down to scores where most peaks are false: small_v13 would keep peaks from raw 0.21 up (where a fifth of
them are targets) to reach full_v3's recall at 0.3, and small_v11, which never saw a moving target, would map its 0.05 to
0.35. The precision at a score is what the review's numbers rely on, and it is a property of the score; recall is a
property of the model, for the acceptance checks to judge.

The threshold: as infer.THRESHOLD was picked (eval.py: the best F1 over 0.2, 0.3, ... 0.6 on the val split of
test_out/vod_model/data, the one it was picked on; full_v3's best there is 0.3), on the mapped scale. A threshold other
than the reference's 0.3 is taken only when its F1 is better on 95% of the scenario draws: where two thresholds are
within the noise, the reference's stands. The reference's own map is null and its threshold 0.3.

Usage: python python/model/calibrate.py <model> [--report FILE] [--write]
  <model>: a name (small_v13), or an export (python/model/exports/detector_<name>_u8in.onnx, or any detector_<name>*
  file beside it). Prints the settings; --write writes them to exports/detector_<name>.json when that file does not
  exist yet (it never replaces one); --report saves them with the numbers behind them as JSON.
"""
import argparse
import hashlib
import json
import re
import sys
import warnings
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import infer  # noqa: E402

FORMAT = 1
REFERENCE = infer.BEST                      # the scale every model's scores are put on
EXPORTS = HERE / "exports"
VODS = r"E:\OBS\KovOBS"                     # crop names start with the md5 of the recording's path here (build_data)
# the map: the val splits full_v3 and small_v13 were picked on (every scenario kind)
VAL = ("test_out/vod_model/data_v3", "test_out/vod_model/data_kills4", "test_out/vod_model/data_moving_dark")
SWEEP_DATA = "test_out/vod_model/data"      # the threshold: eval.py's default, where infer.THRESHOLD was picked
FLOOR = 0.05                                # peaks scoring under this are not collected
SWEEP = (0.2, 0.3, 0.4, 0.5, 0.6)           # eval.py's threshold sweep
KNOTS = np.round(np.arange(0.05, 0.951, 0.05), 2)   # reference scores the map gets a point at
BINS = np.round(np.arange(FLOOR, 1.0001, 0.01), 2)  # the precision fit's bins
DRAWS = 400                                 # bootstrap draws of the scenario folders
MIN_MOVE = 0.01                             # smaller moves are left out of a map


def model_name(model):
    """The model's name from a name or an export's path (detector_<name>[_u8in|_fp32|...].onnx|.pt|.json)."""
    m = re.match(r"detector_(.+?)(_u8in|_fp32|_fp16|_int8|_embed)?\.(pt|onnx|json)$", Path(model).name)
    return m[1] if m else str(model)


def u8in_of(model):
    """The model's _u8in export (raw uint8 frames in, the score and box maps out): the graph the app runs."""
    p = Path(model)
    if p.suffix and not p.is_file():
        raise SystemExit(f"{model}: no such file")
    folder = p.parent if p.suffix else EXPORTS
    u8 = folder / f"detector_{model_name(model)}_u8in.onnx"
    if not u8.is_file():
        raise SystemExit(f"{u8}: not found (python/model/export.py <checkpoint> writes it)")
    return u8


def session(path):
    import onnxruntime as ort
    return ort.InferenceSession(str(path), providers=["CPUExecutionProvider"])


def crop_files(folders, split):
    return [f for d in folders for f in sorted((Path(d) / split).glob("*.npz"))]


def scenarios(files):
    """Each crop's scenario folder, as an index (the manifest's folder of the recording it was cut from; a crop of no
    known recording counts as its own)."""
    folder = {}
    for d in {f.parent.parent for f in files}:
        if (d / "manifest.jsonl").is_file():
            for line in open(d / "manifest.jsonl", encoding="utf-8"):
                r = json.loads(line)
                folder[hashlib.md5(str(Path(VODS) / r["folder"] / r["file"]).encode()).hexdigest()[:10]] = r["folder"]
    names = [folder.get(f.name[:10], f.name) for f in files]
    index = {n: i for i, n in enumerate(sorted(set(names)))}
    return np.array([index[n] for n in names]), len(index)


def labels(boxes):
    """A crop's labelled boxes (cx, cy, w, h), as train.Crops reads them: one target reported twice (within 1.5 px)
    counts once."""
    keep = []
    for b in boxes:
        if all(np.hypot(b[0] - k[0], b[1] - k[1]) > 1.5 for k in keep):
            keep.append(b)
    return np.array(keep, np.float32).reshape(-1, 4)


def peaks(sess, files, floor=FLOOR, batch=16):
    """Every crop's peaks scoring over `floor`, as (n, 5) arrays of cx, cy, w, h, raw score; and its labels."""
    dets, gts = [], []
    if sess.get_inputs()[0].shape[0] == 1:          # an older export, traced for one frame at a time
        batch = 1
    for i in range(0, len(files), batch):
        zs = [np.load(f) for f in files[i:i + batch]]
        rgb = np.stack([z["rgb"] for z in zs])
        fixed = np.stack([z["fixed"] for z in zs]).astype(np.uint8)
        score, reg = sess.run(None, {"rgb": rgb, "fixed": fixed})
        for k, z in enumerate(zs):
            dets.append(infer.decode_np(score[k:k + 1], reg[k:k + 1], floor))
            gts.append(labels(z["boxes"]))
    return dets, gts


def iou(a, b):
    """Intersection over union of two boxes (cx, cy, w, h)."""
    ix = max(0.0, min(a[0] + a[2] / 2, b[0] + b[2] / 2) - max(a[0] - a[2] / 2, b[0] - b[2] / 2))
    iy = max(0.0, min(a[1] + a[3] / 2, b[1] + b[3] / 2) - max(a[1] - a[3] / 2, b[1] - b[3] / 2))
    inter = ix * iy
    return inter / max(1e-9, a[2] * a[3] + b[2] * b[3] - inter)


def match(d, g):
    """train.match for one crop: peaks taken best first, each takes the nearest free label within max(2 px, half the
    label's smaller side). Returns, per peak, the label it took (-1: none). Peaks over any threshold take the same
    labels as here, so one matching serves every threshold."""
    took = np.full(len(d), -1)
    used = np.zeros(len(g), bool)
    if not len(g):
        return took
    tol = np.maximum(0.5 * g[:, 2:].min(axis=1), 2.0)
    for k in np.argsort(-d[:, 4], kind="stable"):
        dist = np.hypot(g[:, 0] - d[k, 0], g[:, 1] - d[k, 1])
        ok = (dist <= tol) & ~used
        if ok.any():
            j = int(np.where(ok, dist, 1e9).argmin())
            used[j] = True
            took[k] = j
    return took


class Scored:
    """A model's peaks on a set of crops, matched to the labels. Per peak: its raw score, whether it took a label, its
    centre error (px), box IoU and width and height ratios against that label, and its crop. Per crop: its label count
    and scenario. The crops' peaks and labels stay in `dets` and `gts`."""

    def __init__(self, dets, gts, files):
        self.dets, self.gts, self.files = dets, gts, files
        rows = []
        for c, (d, g) in enumerate(zip(dets, gts)):
            for k, j in enumerate(match(d, g)):
                hit = j >= 0
                rows.append((d[k, 4], hit, np.hypot(*(d[k, :2] - g[j, :2])) if hit else np.nan,
                             iou(d[k, :4], g[j]) if hit else np.nan, c,
                             d[k, 2] / g[j, 2] if hit else np.nan, d[k, 3] / g[j, 3] if hit else np.nan))
        a = np.array(rows, np.float64).reshape(-1, 7)
        self.score, self.hit, self.err, self.iou, self.crop = a[:, 0], a[:, 1] > 0, a[:, 2], a[:, 3], a[:, 4].astype(int)
        self.w_ratio, self.h_ratio = a[:, 5], a[:, 6]
        self.crop_labels = np.array([len(g) for g in gts])
        self.labels = int(self.crop_labels.sum())
        self.unit, self.units = scenarios(files)

    def weights(self, draw):
        """Per peak and per crop, the weight of its scenario in a draw (how often the draw took that scenario)."""
        w = draw[self.unit]
        return w[self.crop], w

    def at(self, thr, score=None, draw=None):
        """Precision, recall and F1 over a threshold (score > thr), as train.summarize gives them (weighted by a
        draw's scenario counts when given)."""
        s = self.score if score is None else score
        wp, wc = self.weights(draw) if draw is not None else (np.ones(len(s)), np.ones(len(self.crop_labels)))
        over = s > thr
        tp, fp, n = (wp * (over & self.hit)).sum(), (wp * (over & ~self.hit)).sum(), (wc * self.crop_labels).sum()
        p, r = tp / max(1e-9, tp + fp), tp / max(1e-9, n)
        return dict(precision=round(p, 4), recall=round(r, 4), f1=round(2 * p * r / max(1e-9, p + r), 4),
                    tp=round(float(tp)), fp=round(float(fp)), fn=round(float(n - tp)))


def score(model, folders=VAL, split="val"):
    files = crop_files(folders, split)
    if not files:
        raise SystemExit(f"no crops in {', '.join(str(Path(d) / split) for d in folders)}")
    return Scored(*peaks(session(u8in_of(model)), files), files)


def apply_map(points, s):
    """A score map ([raw, mapped] points, or None) applied to raw scores: piecewise linear, clamped at the ends."""
    if points is None:
        return np.asarray(s, np.float64)
    p = np.asarray(points, np.float64)
    return np.interp(s, p[:, 0], p[:, 1])


def precision_curve(score, hit, w=None):
    """The share of peaks that are targets, fitted rising with the score: isotonic regression (pool adjacent
    violators) on 0.01-wide bins. Returns the pooled blocks' mean scores and precisions, both strictly rising, so the
    curve (linear between them) can be read both ways."""
    w = np.ones(len(score)) if w is None else w
    b = np.clip(np.searchsorted(BINS, score) - 1, 0, len(BINS) - 2)
    n, h, s = (np.bincount(b, v, len(BINS) - 1) for v in (w, w * hit, w * score))
    blocks = []                                     # [weight, hits, score sum], pooled while they do not rise
    for k in np.nonzero(n > 0)[0]:
        blocks.append([n[k], h[k], s[k]])
        while len(blocks) > 1 and blocks[-2][1] / blocks[-2][0] >= blocks[-1][1] / blocks[-1][0]:
            last = blocks.pop()
            blocks[-1] = [a + c for a, c in zip(blocks[-1], last)]
    a = np.array(blocks)
    return a[:, 2] / a[:, 0], a[:, 1] / a[:, 0]


def raw_for(new, ref, wn=None, wr=None):
    """For each knot (a reference score), the raw score of `new` with the same fitted precision; NaN where either
    curve does not reach (past the reference's first or last block, or a precision `new` never has)."""
    xr, pr = precision_curve(ref.score, ref.hit, wr)
    xn, pn = precision_curve(new.score, new.hit, wn)
    p = np.interp(KNOTS, xr, pr)
    ok = (KNOTS >= xr[0]) & (KNOTS <= xr[-1]) & (p >= pn[0]) & (p <= pn[-1])
    return np.where(ok, np.interp(p, pn, xn), np.nan)


def fit_map(new, ref, draws=DRAWS, seed=0):
    """The score map from `new` onto the reference's scale (see the top of the file), or None, and per knot: the
    fitted raw score, the 95% range of the draws, and the raw score the map uses."""
    est = raw_for(new, ref)
    rng = np.random.default_rng(seed)
    boot = []
    for _ in range(draws):
        draw = np.bincount(rng.integers(0, new.units, new.units), minlength=new.units).astype(float)
        boot.append(raw_for(new, ref, new.weights(draw)[0], ref.weights(draw)[0]))
    boot = np.array(boot)
    with warnings.catch_warnings():                 # a knot no draw reaches: NaN, which means no move
        warnings.simplefilter("ignore", RuntimeWarning)
        lo, hi = np.nanpercentile(boot, 2.5, axis=0), np.nanpercentile(boot, 97.5, axis=0)
    use = np.clip(KNOTS, lo, hi)                    # the point of the range nearest to no move
    # no move where the fit or a twentieth of the draws cannot say (the curves do not reach), or under MIN_MOVE
    use = np.where(np.isnan(est) | (np.isnan(boot).mean(axis=0) > 0.05) | (np.abs(use - KNOTS) < MIN_MOVE), KNOTS, use)
    r4 = lambda v: None if np.isnan(v) else round(float(v), 4)  # noqa: E731
    knots = [dict(reference=float(m), fitted=r4(e), low=r4(a), high=r4(b), raw=r4(u))
             for m, e, a, b, u in zip(KNOTS, est, lo, hi, use)]
    if np.all(use == KNOTS):
        return None, knots
    pts = [[0.0, 0.0]]
    for m, u in zip(KNOTS, use):
        if u > pts[-1][0] + 1e-4 and m > pts[-1][1]:        # both rise strictly (the settings file's rule)
            pts.append([round(float(u), 4), float(m)])
    pts.append([1.0, 1.0])
    out = pts[:1]                                           # a point on the line through its neighbours says nothing
    for k in range(1, len(pts) - 1):
        (x0, y0), (x1, y1), (x2, y2) = out[-1], pts[k], pts[k + 1]
        if abs((y1 - y0) * (x2 - x1) - (y2 - y1) * (x1 - x0)) > 1e-9:
            out.append(pts[k])
    return out + [pts[-1]], knots


def pick_threshold(new, points, draws=DRAWS, seed=0):
    """eval.py's sweep on the mapped scale: the best F1 over SWEEP, taken over the reference's threshold only when it
    is better on 95% of the scenario draws. Returns the threshold and the sweep."""
    mapped = apply_map(points, new.score)
    sweep = {t: new.at(t, mapped) for t in SWEEP}
    best = max(sweep, key=lambda t: sweep[t]["f1"])
    gain = None
    if best != infer.THRESHOLD:
        rng = np.random.default_rng(seed)
        diffs = []
        for _ in range(draws):
            draw = np.bincount(rng.integers(0, new.units, new.units), minlength=new.units).astype(float)
            diffs.append(new.at(best, mapped, draw)["f1"] - new.at(infer.THRESHOLD, mapped, draw)["f1"])
        gain = dict(best=best, over=infer.THRESHOLD,
                    f1_gain=round(float(sweep[best]["f1"] - sweep[infer.THRESHOLD]["f1"]), 4),
                    better_in=round(float(np.mean(np.array(diffs) > 0)), 3))
        if gain["better_in"] < 0.95:
            best = infer.THRESHOLD
    return best, sweep, gain


def settings(name, thr, points):
    return {"format": FORMAT, "name": name, "threshold": thr, "score_map": points, "reference": REFERENCE}


def calibrate(model, val=VAL, sweep_data=SWEEP_DATA, ref_scored=None):
    """The settings for a model, and the numbers behind them."""
    name = model_name(model)
    if name == REFERENCE:
        return settings(name, infer.THRESHOLD, None), dict(note="the reference model: its own scale")
    new = score(model, val)
    ref = ref_scored or score(REFERENCE, val)
    points, knots = fit_map(new, ref)
    swept = score(model, (sweep_data,))
    thr, sweep, gain = pick_threshold(swept, points)
    s = np.linspace(0, 1, 1001)
    report = dict(
        val=list(map(str, val)), crops=len(new.files), labels=new.labels, scenarios=new.units,
        largest_move=round(float(np.abs(apply_map(points, s) - s).max()), 4), knots=knots,
        sweep_data=str(sweep_data), best_over_reference_threshold=gain,
        sweep={str(t): v for t, v in sweep.items()})
    return settings(name, thr, points), report


def write_settings(s, folder=EXPORTS):
    """exports/detector_<name>.json, only when it does not exist yet (a file there is kept as it is)."""
    p = Path(folder) / f"detector_{s['name']}.json"
    if p.exists():
        old = json.loads(p.read_text())
        print(f"{p}: kept as it is" + ("" if old == s else f"; calibration gives {json.dumps(s)}"))
        return p
    p.write_text(json.dumps(s) + "\n")
    print(f"{p}: written")
    return p


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("model")
    ap.add_argument("--val", action="append", help=f"a dataset folder whose val split fits the map (repeat it) [{', '.join(VAL)}]")
    ap.add_argument("--data", default=SWEEP_DATA, help="the dataset whose val split picks the threshold")
    ap.add_argument("--report", help="save the settings and the numbers behind them here (JSON)")
    ap.add_argument("--write", action="store_true", help="write exports/detector_<name>.json if it does not exist")
    a = ap.parse_args()
    s, report = calibrate(a.model, tuple(a.val or VAL), a.data)
    print(json.dumps(s))
    if "knots" in report:
        moved = [k for k in report["knots"] if k["raw"] != k["reference"]]
        print(f"  largest move {report['largest_move']}; knots moved: " +
              (", ".join(f"raw {k['raw']} -> {k['reference']} (fit {k['fitted']}, 95% {k['low']} to {k['high']})"
                         for k in moved) or "none"))
        print(f"  threshold sweep on {report['sweep_data']}: " +
              ", ".join(f"{t}: F1 {v['f1']}" for t, v in report["sweep"].items()) +
              (f"; best over {infer.THRESHOLD}: {report['best_over_reference_threshold']}"
               if report["best_over_reference_threshold"] else ""))
    if a.report:
        Path(a.report).parent.mkdir(parents=True, exist_ok=True)
        json.dump(dict(settings=s, **report), open(a.report, "w"), indent=1)
    if a.write:
        write_settings(s)


if __name__ == "__main__":
    main()
