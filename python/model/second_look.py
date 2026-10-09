"""A second look at a label batch's agreed crops (build_label_batch.py) the user will not check by hand: their pixel
boxes corrected by what the user's fixes on the checked sample taught, then a second detector's opinion on each crop.
A crop the detector agrees with is accepted automatically (not training data yet); the rest go to the Crops page.

In: <batch>/agreed/*.npz (rgb, fixed, boxes: the pixel boxes, sources), agreed/sample.txt (the sample the user
checked), review/*.npz, and the user's merged answers <batch>/<checked> (checked_phone's lines: file, boxes).

The correction: a thin box (its long side THIN_ASPECT times its short side or more: a tracking capsule or a pole) has
its short side scaled by the mean ratio of the user's short side to the pixels' over the sample's thin boxes (paired at
label_score.MATCH_IOU); other boxes stay. A crop is right when every user box has a box at RIGHT_IOU or more (one to
one, best overlap first) and no box is left over. The rule is scored on the sample as is and leave-one-out (the ratio
learned without the crop it is tried on).

The second opinion: the detector (--model's fp32 export at its threshold) on the crop's rgb and fixed map. A crop is
accepted when every corrected box pairs with a detector box at MATCH_IOU or more and every detector box left unpaired
overlaps a corrected box by EXTRA_IOU or more (a duplicate, not a target the labeller missed); else it is disputed.
The rule is scored on the sample and, as a second check, on the review crops the user answered (their pixel boxes
corrected, the model's as they are), with a sweep of MATCH_IOU. The detector was trained on those answers, so it agrees
with the user there more than it would on crops it never saw: both scores run high.

Out:
- <batch>/agreed/auto_accepted.jsonl: the accepted crops outside the sample, checked_phone's line format (verdict
  "correct", source "auto_accepted", boxes the corrected ones, auto the pixel boxes, model the detector's).
- <batch>/agreed/second_look.json: the scores, the sweep, and the disputed crops by how strongly they disagree.
- Two sets in <page> (crop_check/make_page.py, through <batch>/<set>_picks.jsonl): SET_NAME, the disputed crops of
  MIN_PAGE_STRENGTH or more, the corrected boxes, then the detector's unpaired boxes crossed out; and ACCEPTED_SET,
  ACCEPTED_SAMPLE seeded accepted crops with their corrected boxes, whose answers score the accepted tier on crops
  the detector was not trained on. The weaker disputed crops are left out ("left_out" in second_look.json).

Usage: python python/model/second_look.py <batch> [--page <folder>] [--checked checked_phone_2.jsonl]
       [--model large_v16e4]
"""
import argparse
import json
import random
import subprocess
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import infer  # noqa: E402
import label_score  # noqa: E402
from box_overlap import iou  # noqa: E402

THIN_ASPECT = 2.5               # a box this many times as long as it is wide (or more) is thin
RIGHT_IOU = 0.85                # a box is the user's box at this overlap or more
MATCH_IOU = 0.77                # a corrected box is the detector's at this overlap or more: the least (from 0.75 up)
                                # whose accepted sample crops were MIN_ACCURACY right
EXTRA_IOU = 0.5                 # an unpaired detector box under this overlap with every corrected box is an extra
SWEEP_IOU = np.round(np.arange(0.70, 0.865, 0.01), 2)   # the MATCH_IOU values the sweep tries
MIN_ACCURACY = 0.97             # the share of accepted sample crops that must be right
MIN_PAGE_STRENGTH = 0.4         # a disputed crop this strong or more goes on the page; a weaker one (box size only)
                                # is left out, not training data
ACCEPTED_SAMPLE = 20            # accepted crops put on the page for the user to check the accepted tier by
SEED = 0                        # the accepted sample's seed
THREADS = 4                     # ONNX Runtime's threads
SET_NAME = "auto_disputed"
TITLE = "Second look: the labeller and large_v16e4 disagree"
CROSSED_OUT = "Crossed out: the detector's boxes the labeller does not have. Restore one if it is a target."
SOURCE = "auto_accepted"
ACCEPTED_SET = "auto_accepted_sample"
ACCEPTED_TITLE = "Accepted automatically: a random check"


def paired(found, truth, min_iou):
    """The pairs (found index, truth index, IoU) matched one to one, best overlap first, at `min_iou` or more."""
    pairs = sorted(((iou(f, t), i, j) for i, f in enumerate(found) for j, t in enumerate(truth)), reverse=True)
    used_found, used_truth, out = set(), set(), []
    for overlap, i, j in pairs:
        if overlap < min_iou:
            break
        if i not in used_found and j not in used_truth:
            used_found.add(i)
            used_truth.add(j)
            out.append((i, j, overlap))
    return out


def is_right(boxes, user):
    """Whether a crop's boxes are the user's: each user box has one at RIGHT_IOU or more and none is left over."""
    return len(boxes) == len(user) and len(paired(boxes, user, RIGHT_IOU)) == len(user)


def is_thin(box):
    """Whether a box [cx, cy, w, h] is thin: its long side THIN_ASPECT times its short side or more."""
    return max(box[2], box[3]) >= THIN_ASPECT * min(box[2], box[3])


def corrected(boxes, sources, scale):
    """The boxes [cx, cy, w, h] (px) with each thin pixel box's short side times `scale`; the model's boxes stay."""
    out = []
    for box, source in zip(boxes, sources):
        box = [float(value) for value in box]
        if source != "model" and is_thin(box):
            box[2 if box[2] < box[3] else 3] *= scale
        out.append(box)
    return out


def thin_ratios(crops):
    """The user's short side over the pixels' for each thin pixel box the user kept (paired at
    label_score.MATCH_IOU) in `crops`."""
    ratios = []
    for crop in crops:
        for i, j in label_score.matched(crop["boxes"], crop["user"]):
            box, user = crop["boxes"][i], crop["user"][j]
            if is_thin(box):
                short = 2 if box[2] < box[3] else 3
                ratios.append(user[short] / box[short])
    return ratios


def score_correction(sample, scale):
    """The sample's right crops with the pixel boxes, the corrected ones (`scale`, learned on the whole sample) and
    the corrected ones leave-one-out (the scale learned without the crop)."""
    loo = 0
    for k, crop in enumerate(sample):
        held_out = float(np.mean(thin_ratios(sample[:k] + sample[k + 1:])))
        loo += is_right(corrected(crop["boxes"], crop["sources"], held_out), crop["user"])
    return {"crops": len(sample), "thin_boxes": len(thin_ratios(sample)), "scale": round(scale, 3),
            "right_raw": sum(is_right(crop["boxes"], crop["user"]) for crop in sample),
            "right_corrected": sum(is_right(crop["fixed_boxes"], crop["user"]) for crop in sample),
            "right_leave_one_out": loo}


def second_opinion(boxes, found, match_iou):
    """A crop's verdict on its corrected boxes against the detector's (n, 5) boxes: (accepted, the detector boxes'
    indexes left unpaired, how strongly they disagree 0 to 1: the largest of one minus each corrected box's IoU
    with its pair, or with its nearest detector box when it has none, and each extra box's score)."""
    detector = [list(box[:4]) for box in found]
    pairs = paired(boxes, detector, match_iou)
    by_box = {i: overlap for i, _, overlap in pairs}
    used = {j for _, j, _ in pairs}
    unpaired = [j for j in range(len(detector)) if j not in used]
    extras = [j for j in unpaired if all(iou(box, detector[j]) < EXTRA_IOU for box in boxes)]
    misses = [1 - by_box[i] if i in by_box else 1 - max((iou(box, other) for other in detector), default=0.0)
              for i, box in enumerate(boxes)]
    strength = max(misses + [float(found[j][4]) for j in extras], default=0.0)
    return len(pairs) == len(boxes) and not extras, unpaired, round(strength, 3)


def acceptance(crops, match_iou):
    """The accepted crops of `crops` at `match_iou` and how many of them are right."""
    accepted = [crop for crop in crops if second_opinion(crop["fixed_boxes"], crop["found"], match_iou)[0]]
    return len(accepted), sum(is_right(crop["fixed_boxes"], crop["user"]) for crop in accepted)


def load(batch, part, names, answers, detector, threshold):
    """The crops of a part whose file name is in `names`: their file (relative to the batch), boxes, sources and
    scores, the detector's boxes (n, 5), and the user's boxes (None when not answered)."""
    crops = []
    for name in sorted(names):
        data = np.load(batch / part / name, allow_pickle=True)
        file = f"{part}/{name}"
        crops.append(dict(file=file, boxes=data["boxes"].tolist(),
                          sources=data["sources"].tolist(), scores=data["scores"].tolist(),
                          found=detector(data["rgb"], data["fixed"], threshold).tolist(),
                          user=answers[file]["boxes"] if file in answers else None))
    return crops


def accuracy(counts):
    """A (accepted, right) pair as a dict with the share right."""
    accepted, right = counts
    return {"accepted": accepted, "right": right, "share": round(right / accepted, 3) if accepted else None}


def pick_line(crop, unpaired, strength, kinds):
    """A disputed crop's picks line for make_page.py: the corrected boxes, then the detector's unpaired boxes
    crossed out."""
    shown = [crop["found"][j] for j in unpaired]
    boxes = crop["fixed_boxes"] + [box[:4] for box in shown]
    folder, kind = kinds.get(crop["file"].split("/")[1][:10], ("", ""))
    why = [f"disagreement {strength}: boxes 0 to {len(crop['fixed_boxes']) - 1} are the labeller's (thin ones "
           f"narrowed); the crossed-out ones are the detector's at no match"]
    return {"file": crop["file"], "folder": folder, "kind": kind, "why": why,
            "boxes": [[round(value, 1) for value in box] for box in boxes],
            "scores": [round(value, 2) for value in crop["scores"]] + [round(box[4], 2) for box in shown],
            **({"preset": {"remove": list(range(len(crop["fixed_boxes"]), len(boxes)))}} if shown else {})}


def make_set(args, name, picks, title, crossed_out=None):
    """Writes `picks` (picks lines) to <batch>/<name>_picks.jsonl (at the batch's root: make_page reads a pick's file
    relative to the picks' folder) and makes the page set `name` from them."""
    path = args.batch / f"{name}_picks.jsonl"
    path.write_text("".join(json.dumps(pick) + "\n" for pick in picks), encoding="utf-8")
    command = [sys.executable, str(HERE / "crop_check" / "make_page.py"), str(args.page), name, str(path),
               "--title", title]
    subprocess.run(command + (["--crossed-out", crossed_out] if crossed_out else []), check=True)


def accepted_pick(crop, kinds):
    """An accepted crop's picks line for make_page.py: its corrected boxes."""
    folder, kind = kinds.get(crop["file"].split("/")[1][:10], ("", ""))
    return {"file": crop["file"], "folder": folder, "kind": kind,
            "why": ["accepted automatically: the labeller and the detector agree (thin boxes narrowed)"],
            "boxes": [[round(value, 1) for value in box] for box in crop["fixed_boxes"]],
            "scores": [round(value, 2) for value in crop["scores"]]}


def write_outputs(args, rest, kinds, summary):
    """auto_accepted.jsonl, the page's two sets (the disputed crops of MIN_PAGE_STRENGTH or more, and a seeded sample
    of ACCEPTED_SAMPLE accepted crops), and second_look.json; the disputed crops' count."""
    accepted, disputed = [], []
    for crop in rest:
        ok, unpaired, strength = second_opinion(crop["fixed_boxes"], crop["found"], MATCH_IOU)
        (accepted if ok else disputed).append((crop, unpaired, strength))
    disputed.sort(key=lambda item: -item[2])
    lines = [{"file": crop["file"], "boxes": [[round(value, 1) for value in box] for box in crop["fixed_boxes"]],
              "verdict": "correct", "auto": crop["boxes"], "model": [[round(value, 2) for value in box]
                                                                     for box in crop["found"]], "source": SOURCE}
             for crop, _, _ in accepted]
    agreed = args.batch / "agreed"
    (agreed / "auto_accepted.jsonl").write_text("".join(json.dumps(line) + "\n" for line in lines), encoding="utf-8")
    shown = [item for item in disputed if item[2] >= MIN_PAGE_STRENGTH]
    make_set(args, SET_NAME, [pick_line(*item, kinds) for item in shown], TITLE, CROSSED_OUT)
    checked = sorted(random.Random(SEED).sample([crop for crop, _, _ in accepted], ACCEPTED_SAMPLE),
                     key=lambda crop: crop["file"])
    make_set(args, ACCEPTED_SET, [accepted_pick(crop, kinds) for crop in checked], ACCEPTED_TITLE)
    summary.update(auto_accepted=len(accepted), disputed=len(disputed), on_page=len(shown),
                   left_out=len(disputed) - len(shown), accepted_on_page=len(checked),
                   disputed_by_strength=[{"file": crop["file"], "strength": strength,
                                          "left_out": strength < MIN_PAGE_STRENGTH}
                                         for crop, _, strength in disputed])
    (agreed / "second_look.json").write_text(json.dumps(summary, indent=1), encoding="utf-8")
    return len(disputed)


def main():
    """Learns the correction, scores the acceptance rule, and writes the accepted crops and the page."""
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("batch", type=Path)
    parser.add_argument("--page", type=Path, help="the page folder (default: check_<batch's name>_rest beside it)")
    parser.add_argument("--checked", default="checked_phone_2.jsonl")
    parser.add_argument("--model", default="large_v16e4")
    args = parser.parse_args()
    args.page = args.page or args.batch.parent / f"check_{args.batch.name}_rest"
    settings = json.loads((HERE / "exports" / f"detector_{args.model}.json").read_text(encoding="utf-8"))
    detector = infer.OnnxDetector(HERE / "exports" / f"detector_{args.model}_fp32.onnx", threads=THREADS)
    answers = {row["file"]: row for row in map(json.loads, (args.batch / args.checked).open(encoding="utf-8"))}
    sample_names = set((args.batch / "agreed" / "sample.txt").read_text(encoding="utf-8").split())
    agreed_names = {path.name for path in (args.batch / "agreed").glob("*.npz")}
    review_names = {Path(file).name for file in answers if file.startswith("review/")}
    threshold = settings["threshold"]
    sample = load(args.batch, "agreed", sample_names, answers, detector, threshold)
    rest = load(args.batch, "agreed", agreed_names - sample_names, answers, detector, threshold)
    review = load(args.batch, "review", review_names, answers, detector, threshold)
    scale = float(np.mean(thin_ratios(sample)))
    for crop in sample + rest + review:
        crop["fixed_boxes"] = corrected(crop["boxes"], crop["sources"], scale)
    correction = score_correction(sample, scale)
    summary = {"model": args.model, "threshold": settings["threshold"], "correction": correction,
               "match_iou": MATCH_IOU, "extra_iou": EXTRA_IOU,
               "sample": accuracy(acceptance(sample, MATCH_IOU)), "review": accuracy(acceptance(review, MATCH_IOU)),
               "sweep": [{"match_iou": float(value), "sample": accuracy(acceptance(sample, value)),
                          "review": accuracy(acceptance(review, value)),
                          "rest_accepted": sum(second_opinion(crop["fixed_boxes"], crop["found"], value)[0]
                                               for crop in rest)} for value in SWEEP_IOU]}
    kinds = {row["stem"]: (row["folder"], row["kind"])
             for row in map(json.loads, (args.batch / "manifest.jsonl").open(encoding="utf-8"))}
    disputed = write_outputs(args, rest, kinds, summary)
    print(json.dumps({key: value for key, value in summary.items() if key not in ("sweep", "disputed_by_strength")},
                     indent=1))
    for row in summary["sweep"]:
        print(f"match IoU {row['match_iou']:.2f}: sample {row['sample']}, review {row['review']}, "
              f"rest accepted {row['rest_accepted']}")
    share = summary["sample"]["share"]
    print(f"{disputed} disputed, {summary['on_page']} on the page {args.page}, {summary['left_out']} left out, "
          f"{summary['accepted_on_page']} accepted on the page; sample accuracy {share} "
          f"({'meets' if share is not None and share >= MIN_ACCURACY else 'below'} {MIN_ACCURACY})")


if __name__ == "__main__":
    main()
