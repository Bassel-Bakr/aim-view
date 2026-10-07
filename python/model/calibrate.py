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
sys.path.insert(0, str(HERE.parent))
import infer  # noqa: E402
import local_config  # noqa: E402

FORMAT = 1
REFERENCE = infer.BEST                      # the scale every model's scores are put on
EXPORTS = HERE / "exports"
VODS = local_config.folder("vods")                     # crop names start with the md5 of the recording's path here (build_data)
# the map: the val splits full_v3 and small_v13 were picked on (every scenario kind)
VAL = ("test_out/vod_model/data_v3", "test_out/vod_model/data_kills4", "test_out/vod_model/data_moving_dark")
SWEEP_DATA = "test_out/vod_model/data"      # the threshold: eval.py's default, where infer.THRESHOLD was picked
FLOOR = 0.05                                # peaks scoring under this are not collected
SWEEP = (0.2, 0.3, 0.4, 0.5, 0.6)           # eval.py's threshold sweep
KNOTS = np.round(np.arange(0.05, 0.951, 0.05), 2)   # reference scores the map gets a point at
BINS = np.round(np.arange(FLOOR, 1.0001, 0.01), 2)  # the precision fit's bins
DRAWS = 400                                 # bootstrap draws of the scenario folders
MIN_MOVE = 0.01                             # smaller moves are left out of a map
SAME_TARGET_PX = 1.5                        # two labels this close are one target reported twice
MATCH_MIN_PX = 2.0                          # a peak takes a label within this, or half the label's smaller side
FAR = 1e9                                   # a distance no match takes
TINY = 1e-9                                 # the smallest denominator
STEM_CHARS = 10                             # a crop name starts with this many characters of its VOD's md5
RANGE_PERCENTILES = (2.5, 97.5)             # the draws' 95% range
UNREACHED_SHARE = 0.05                      # a knot that more of the draws do not reach does not move
RISE = 1e-4                                 # a map point's raw score must pass the one before by this
BETTER_IN = 0.95                            # a threshold other than the reference's must be better on this share
MAP_STEPS = 1001                            # scores the largest move is measured on


def model_name(model):
    """The model's name from a name or an export's path (detector_<name>[_u8in|_fp32|...].onnx|.pt|.json)."""
    name = re.match(r"detector_(.+?)(_u8in|_fp32|_fp16|_int8|_embed)?\.(pt|onnx|json)$", Path(model).name)
    return name[1] if name else str(model)


def u8in_of(model):
    """The model's _u8in export (raw uint8 frames in, the score and box maps out): the graph the app runs."""
    path = Path(model)
    if path.suffix and not path.is_file():
        raise SystemExit(f"{model}: no such file")
    folder = path.parent if path.suffix else EXPORTS
    u8 = folder / f"detector_{model_name(model)}_u8in.onnx"
    if not u8.is_file():
        raise SystemExit(f"{u8}: not found (python/model/export.py <checkpoint> writes it)")
    return u8


def session(path, gpu=False):
    """The export's ONNX Runtime session, on the CPU; with gpu, on CUDA where ONNX Runtime's CUDA build is installed
    (onnxruntime-gpu, on the CUDA and cuDNN libraries PyTorch brings, loaded with it first), about 3 ms a frame against
    25 on the CPU, with cuDNN's default algorithms so a rerun gives the same numbers."""
    import onnxruntime as ort
    cuda = []
    if gpu and "CUDAExecutionProvider" in ort.get_available_providers():
        import torch  # noqa: F401 (its CUDA libraries, which ONNX Runtime's CUDA build loads)
        cuda = [("CUDAExecutionProvider", {"cudnn_conv_algo_search": "HEURISTIC", "use_tf32": "0"})]
    return ort.InferenceSession(str(path), providers=[*cuda, "CPUExecutionProvider"])


def crop_files(folders, split):
    return [file for folder in folders for file in sorted((Path(folder) / split).glob("*.npz"))]


def scenarios(files):
    """Each crop's scenario folder, as an index (the manifest's folder of the recording it was cut from; a crop of no
    known recording counts as its own)."""
    folder = {}
    for dataset in {file.parent.parent for file in files}:
        if (dataset / "manifest.jsonl").is_file():
            for line in open(dataset / "manifest.jsonl", encoding="utf-8"):
                row = json.loads(line)
                video = str(Path(VODS) / row["folder"] / row["file"])
                folder[hashlib.md5(video.encode()).hexdigest()[:STEM_CHARS]] = row["folder"]
    names = [folder.get(file.name[:STEM_CHARS], file.name) for file in files]
    index = {name: i for i, name in enumerate(sorted(set(names)))}
    return np.array([index[name] for name in names]), len(index)


def labels(boxes):
    """A crop's labelled boxes (cx, cy, w, h), as train.Crops reads them: one target reported twice (within 1.5 px)
    counts once."""
    keep = []
    for box in boxes:
        if all(np.hypot(box[0] - kept[0], box[1] - kept[1]) > SAME_TARGET_PX for kept in keep):
            keep.append(box)
    return np.array(keep, np.float32).reshape(-1, 4)


def peaks(onnx_session, files, floor=FLOOR, batch=16):
    """Every crop's peaks scoring over `floor`, as (n, 5) arrays of cx, cy, w, h, raw score; and its labels."""
    dets, gts = [], []
    if onnx_session.get_inputs()[0].shape[0] == 1:  # an older export, traced for one frame at a time
        batch = 1
    for i in range(0, len(files), batch):
        crops = [np.load(file) for file in files[i:i + batch]]
        rgb = np.stack([crop["rgb"] for crop in crops])
        fixed = np.stack([crop["fixed"] for crop in crops]).astype(np.uint8)
        score, reg = onnx_session.run(None, {"rgb": rgb, "fixed": fixed})
        for k, crop in enumerate(crops):
            dets.append(infer.decode_np(score[k:k + 1], reg[k:k + 1], floor))
            gts.append(labels(crop["boxes"]))
    return dets, gts


def iou(a, b):
    """Intersection over union of two boxes (cx, cy, w, h)."""
    overlap_x = max(0.0, min(a[0] + a[2] / 2, b[0] + b[2] / 2) - max(a[0] - a[2] / 2, b[0] - b[2] / 2))
    overlap_y = max(0.0, min(a[1] + a[3] / 2, b[1] + b[3] / 2) - max(a[1] - a[3] / 2, b[1] - b[3] / 2))
    inter = overlap_x * overlap_y
    return inter / max(TINY, a[2] * a[3] + b[2] * b[3] - inter)


def match(found, truth):
    """train.match for one crop: peaks taken best first, each takes the nearest free label within max(2 px, half the
    label's smaller side). Returns, per peak, the label it took (-1: none). Peaks over any threshold take the same
    labels as here, so one matching serves every threshold."""
    took = np.full(len(found), -1)
    used = np.zeros(len(truth), bool)
    if not len(truth):
        return took
    tolerance = np.maximum(0.5 * truth[:, 2:].min(axis=1), MATCH_MIN_PX)
    for k in np.argsort(-found[:, 4], kind="stable"):
        distance = np.hypot(truth[:, 0] - found[k, 0], truth[:, 1] - found[k, 1])
        free = (distance <= tolerance) & ~used
        if free.any():
            j = int(np.where(free, distance, FAR).argmin())
            used[j] = True
            took[k] = j
    return took


class Scored:
    """A model's peaks on a set of crops, matched to the labels. Per peak: its raw score, whether it took a label, its
    center error (px), box IoU and width and height ratios against that label, and its crop. Per crop: its label count
    and scenario. The crops' peaks and labels stay in `dets` and `gts`."""

    def __init__(self, dets, gts, files):
        self.dets, self.gts, self.files = dets, gts, files
        rows = []
        for crop, (found, truth) in enumerate(zip(dets, gts)):
            for k, j in enumerate(match(found, truth)):
                hit = j >= 0
                rows.append((found[k, 4], hit, np.hypot(*(found[k, :2] - truth[j, :2])) if hit else np.nan,
                             iou(found[k, :4], truth[j]) if hit else np.nan, crop,
                             found[k, 2] / truth[j, 2] if hit else np.nan, found[k, 3] / truth[j, 3] if hit else np.nan))
        table = np.array(rows, np.float64).reshape(-1, 7)
        self.score, self.hit, self.err, self.iou = table[:, 0], table[:, 1] > 0, table[:, 2], table[:, 3]
        self.crop = table[:, 4].astype(int)
        self.w_ratio, self.h_ratio = table[:, 5], table[:, 6]
        self.crop_labels = np.array([len(truth) for truth in gts])
        self.labels = int(self.crop_labels.sum())
        self.unit, self.units = scenarios(files)

    def weights(self, draw):
        """Per peak and per crop, the weight of its scenario in a draw (how often the draw took that scenario)."""
        per_crop = draw[self.unit]
        return per_crop[self.crop], per_crop

    def at(self, threshold, score=None, draw=None):
        """Precision, recall and F1 over a threshold (score > threshold), as train.summarize gives them (weighted by a
        draw's scenario counts when given)."""
        scores = self.score if score is None else score
        peak_weight, crop_weight = (self.weights(draw) if draw is not None
                                    else (np.ones(len(scores)), np.ones(len(self.crop_labels))))
        over = scores > threshold
        true_pos = (peak_weight * (over & self.hit)).sum()
        false_pos = (peak_weight * (over & ~self.hit)).sum()
        targets = (crop_weight * self.crop_labels).sum()
        precision, recall = true_pos / max(TINY, true_pos + false_pos), true_pos / max(TINY, targets)
        return dict(precision=round(precision, 4), recall=round(recall, 4),
                    f1=round(2 * precision * recall / max(TINY, precision + recall), 4),
                    tp=round(float(true_pos)), fp=round(float(false_pos)), fn=round(float(targets - true_pos)))


def score(model, folders=VAL, split="val"):
    files = crop_files(folders, split)
    if not files:
        raise SystemExit(f"no crops in {', '.join(str(Path(folder) / split) for folder in folders)}")
    return Scored(*peaks(session(u8in_of(model)), files), files)


def apply_map(points, scores):
    """A score map ([raw, mapped] points, or None) applied to raw scores: piecewise linear, clamped at the ends."""
    if points is None:
        return np.asarray(scores, np.float64)
    table = np.asarray(points, np.float64)
    return np.interp(scores, table[:, 0], table[:, 1])


def precision_curve(score, hit, weight=None):
    """The share of peaks that are targets, fitted rising with the score: isotonic regression (pool adjacent
    violators) on 0.01-wide bins. Returns the pooled blocks' mean scores and precisions, both strictly rising, so the
    curve (linear between them) can be read both ways."""
    weight = np.ones(len(score)) if weight is None else weight
    bins = np.clip(np.searchsorted(BINS, score) - 1, 0, len(BINS) - 2)
    count, hits, score_sum = (np.bincount(bins, values, len(BINS) - 1) for values in (weight, weight * hit,
                                                                                         weight * score))
    blocks = []                                     # [weight, hits, score sum], pooled while they do not rise
    for k in np.nonzero(count > 0)[0]:
        blocks.append([count[k], hits[k], score_sum[k]])
        while len(blocks) > 1 and blocks[-2][1] / blocks[-2][0] >= blocks[-1][1] / blocks[-1][0]:
            last = blocks.pop()
            blocks[-1] = [a + b for a, b in zip(blocks[-1], last)]
    pooled = np.array(blocks)
    return pooled[:, 2] / pooled[:, 0], pooled[:, 1] / pooled[:, 0]


def raw_for(new, ref, new_weight=None, ref_weight=None):
    """For each knot (a reference score), the raw score of `new` with the same fitted precision; NaN where either
    curve does not reach (past the reference's first or last block, or a precision `new` never has)."""
    ref_scores, ref_precision = precision_curve(ref.score, ref.hit, ref_weight)
    new_scores, new_precision = precision_curve(new.score, new.hit, new_weight)
    precision = np.interp(KNOTS, ref_scores, ref_precision)
    reached = ((KNOTS >= ref_scores[0]) & (KNOTS <= ref_scores[-1]) & (precision >= new_precision[0])
               & (precision <= new_precision[-1]))
    return np.where(reached, np.interp(precision, new_precision, new_scores), np.nan)


def draw_of(rng, units):
    """A bootstrap draw of the scenarios: how often it took each."""
    return np.bincount(rng.integers(0, units, units), minlength=units).astype(float)


def rounded(value):
    return None if np.isnan(value) else round(float(value), 4)


def map_points(used):
    """The map's points from the raw score each knot uses: both rising strictly (the settings file's rule), and a
    point on the line through its neighbors left out (it says nothing)."""
    points = [[0.0, 0.0]]
    for knot, raw in zip(KNOTS, used):
        if raw > points[-1][0] + RISE and knot > points[-1][1]:
            points.append([round(float(raw), 4), float(knot)])
    points.append([1.0, 1.0])
    out = points[:1]
    for k in range(1, len(points) - 1):
        (x0, y0), (x1, y1), (x2, y2) = out[-1], points[k], points[k + 1]
        if abs((y1 - y0) * (x2 - x1) - (y2 - y1) * (x1 - x0)) > TINY:
            out.append(points[k])
    return out + [points[-1]]


def fit_map(new, ref, draws=DRAWS, seed=0):
    """The score map from `new` onto the reference's scale (see the top of the file), or None, and per knot: the
    fitted raw score, the 95% range of the draws, and the raw score the map uses."""
    fitted = raw_for(new, ref)
    rng = np.random.default_rng(seed)
    boot = []
    for _ in range(draws):
        draw = draw_of(rng, new.units)
        boot.append(raw_for(new, ref, new.weights(draw)[0], ref.weights(draw)[0]))
    boot = np.array(boot)
    with warnings.catch_warnings():                 # a knot no draw reaches: NaN, which means no move
        warnings.simplefilter("ignore", RuntimeWarning)
        low, high = (np.nanpercentile(boot, percentile, axis=0) for percentile in RANGE_PERCENTILES)
    used = np.clip(KNOTS, low, high)                # the point of the range nearest to no move
    # no move where the fit or a twentieth of the draws cannot say (the curves do not reach), or under MIN_MOVE
    used = np.where(np.isnan(fitted) | (np.isnan(boot).mean(axis=0) > UNREACHED_SHARE)
                    | (np.abs(used - KNOTS) < MIN_MOVE), KNOTS, used)
    knots = [dict(reference=float(knot), fitted=rounded(fit), low=rounded(lo), high=rounded(hi), raw=rounded(raw))
             for knot, fit, lo, hi, raw in zip(KNOTS, fitted, low, high, used)]
    if np.all(used == KNOTS):
        return None, knots
    return map_points(used), knots


def pick_threshold(new, points, draws=DRAWS, seed=0):
    """eval.py's sweep on the mapped scale: the best F1 over SWEEP, taken over the reference's threshold only when it
    is better on 95% of the scenario draws. Returns the threshold and the sweep."""
    mapped = apply_map(points, new.score)
    sweep = {threshold: new.at(threshold, mapped) for threshold in SWEEP}
    best = max(sweep, key=lambda threshold: sweep[threshold]["f1"])
    gain = None
    if best != infer.THRESHOLD:
        rng = np.random.default_rng(seed)
        diffs = []
        for _ in range(draws):
            draw = draw_of(rng, new.units)
            diffs.append(new.at(best, mapped, draw)["f1"] - new.at(infer.THRESHOLD, mapped, draw)["f1"])
        gain = dict(best=best, over=infer.THRESHOLD,
                    f1_gain=round(float(sweep[best]["f1"] - sweep[infer.THRESHOLD]["f1"]), 4),
                    better_in=round(float(np.mean(np.array(diffs) > 0)), 3))
        if gain["better_in"] < BETTER_IN:
            best = infer.THRESHOLD
    return best, sweep, gain


def settings(name, threshold, points):
    return {"format": FORMAT, "name": name, "threshold": threshold, "score_map": points, "reference": REFERENCE}


def calibrate(model, val=VAL, sweep_data=SWEEP_DATA, ref_scored=None):
    """The settings for a model, and the numbers behind them."""
    name = model_name(model)
    if name == REFERENCE:
        return settings(name, infer.THRESHOLD, None), dict(note="the reference model: its own scale")
    new = score(model, val)
    ref = ref_scored or score(REFERENCE, val)
    points, knots = fit_map(new, ref)
    swept = score(model, (sweep_data,))
    threshold, sweep, gain = pick_threshold(swept, points)
    scores = np.linspace(0, 1, MAP_STEPS)
    report = dict(
        val=list(map(str, val)), crops=len(new.files), labels=new.labels, scenarios=new.units,
        largest_move=round(float(np.abs(apply_map(points, scores) - scores).max()), 4), knots=knots,
        sweep_data=str(sweep_data), best_over_reference_threshold=gain,
        sweep={str(tried): numbers for tried, numbers in sweep.items()})
    return settings(name, threshold, points), report


def write_settings(model_settings, folder=EXPORTS):
    """exports/detector_<name>.json, only when it does not exist yet (a file there is kept as it is)."""
    path = Path(folder) / f"detector_{model_settings['name']}.json"
    if path.exists():
        old = json.loads(path.read_text())
        print(f"{path}: kept as it is" + ("" if old == model_settings
                                          else f"; calibration gives {json.dumps(model_settings)}"))
        return path
    path.write_text(json.dumps(model_settings) + "\n")
    print(f"{path}: written")
    return path


def print_report(report):
    moved = [knot for knot in report["knots"] if knot["raw"] != knot["reference"]]
    print(f"  largest move {report['largest_move']}; knots moved: " +
          (", ".join(f"raw {knot['raw']} -> {knot['reference']} (fit {knot['fitted']}, 95% {knot['low']} to "
                     f"{knot['high']})" for knot in moved) or "none"))
    print(f"  threshold sweep on {report['sweep_data']}: " +
          ", ".join(f"{tried}: F1 {numbers['f1']}" for tried, numbers in report["sweep"].items()) +
          (f"; best over {infer.THRESHOLD}: {report['best_over_reference_threshold']}"
           if report["best_over_reference_threshold"] else ""))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("model")
    parser.add_argument("--val", action="append", help="a dataset folder whose val split fits the map (repeat it) "
                        f"[{', '.join(VAL)}]")
    parser.add_argument("--data", default=SWEEP_DATA, help="the dataset whose val split picks the threshold")
    parser.add_argument("--report", help="save the settings and the numbers behind them here (JSON)")
    parser.add_argument("--write", action="store_true", help="write exports/detector_<name>.json if it does not exist")
    args = parser.parse_args()
    model_settings, report = calibrate(args.model, tuple(args.val or VAL), args.data)
    print(json.dumps(model_settings))
    if "knots" in report:
        print_report(report)
    if args.report:
        Path(args.report).parent.mkdir(parents=True, exist_ok=True)
        json.dump(dict(settings=model_settings, **report), open(args.report, "w"), indent=1)
    if args.write:
        write_settings(model_settings)


if __name__ == "__main__":
    main()
