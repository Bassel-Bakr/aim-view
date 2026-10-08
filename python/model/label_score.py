"""Scores automatic labellers against the crops the user checked by eye: before a labeller's boxes are trusted (or
sent to the user to check), it has to find the targets the user boxed on crops it never saw.

In: the checked sets in the data folder's vod_model/ (each a folder with checked_phone.jsonl: per crop its file, the
user's boxes, cx cy w h in crop pixels, and the model's boxes then; the crops are .npz files with rgb and fixed).
Out: a table per set (and per crop source) of each labeller's recall, precision and box error against the user's
boxes, printed and written to <out>/label_score.json.

The labellers:
- model: the boxes of the model that made the crops (its boxes when they were checked).
- pixels: build_auto_labels.py's pixel rules with the target's color learned from the recording, as a labeller run
  over a recording's frames would learn it from its stats file's kills. Here the color is the median of the user's
  boxes' colors on the recording's other checked crops (the crop's own left out), so the score is the labeller's on
  a crop it learned nothing from. Each part of that color with a target's shape (`build_auto_labels.shape_of`), off
  the crop's edge and at least MIN_AREA_PX, is a box. A crop whose recording has no other checked target is left out
  of this labeller's counts.
- agree: the pixels' boxes that a model box matches (MATCH_IOU), as the pixels drew them: the boxes two labellers
  agree on, which a review could check by sample only; the others go to the user whole.

A box finds a user's box when their overlap (IoU) is MATCH_IOU or more, matched greedily by overlap. The box error is
the median of |dx|, |dy|, |dw|, |dh| over the matched pairs, in pixels.

Usage: python python/model/label_score.py [--sets data_moving_themes,data_mined,hand_small,teacher_robots]
       [--out test_out/label_score]
"""
import argparse
import json
import re
import sys
from collections import defaultdict
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build_auto_labels as auto  # noqa: E402  the pixel rules
from local_config import folder  # noqa: E402

SETS = ("data_moving_themes", "data_mined", "hand_small", "teacher_robots")
CHECKED = "checked_phone.jsonl"     # each set's crops as the user checked them
MATCH_IOU = 0.5                     # a box and the user's box are the same target at this overlap or more
CORE_SHARE = 0.35                   # a box's color: its pixels within this share of its width and height of its center
MIN_CORE_PX = 3                     # a box gives a color only with at least this many such pixels off the fixed map
HASH_NAME = re.compile(r"^[0-9a-f]{10}_")   # a crop named after its recording's hash (data_moving_themes, data_mined)


def recording_of(file):
    """The recording a crop came from, from its file name: the hash a hashed name starts with, else the name without
    its last part (the crop's number)."""
    name = Path(file).stem
    return name[:10] if HASH_NAME.match(name) else name.rsplit("_", 1)[0]


def iou(a, b):
    """The overlap of two boxes (cx, cy, w, h) over their union."""
    ax0, ay0, ax1, ay1 = a[0] - a[2] / 2, a[1] - a[3] / 2, a[0] + a[2] / 2, a[1] + a[3] / 2
    bx0, by0, bx1, by1 = b[0] - b[2] / 2, b[1] - b[3] / 2, b[0] + b[2] / 2, b[1] + b[3] / 2
    inter = max(0.0, min(ax1, bx1) - max(ax0, bx0)) * max(0.0, min(ay1, by1) - max(ay0, by0))
    union = a[2] * a[3] + b[2] * b[3] - inter
    return inter / union if union > 0 else 0.0


def matched(found, truth):
    """The pairs (found index, truth index) matched greedily by overlap, MATCH_IOU or more."""
    pairs = sorted(((iou(f, t), i, j) for i, f in enumerate(found) for j, t in enumerate(truth)), reverse=True)
    used_found, used_truth, out = set(), set(), []
    for overlap, i, j in pairs:
        if overlap < MATCH_IOU:
            break
        if i not in used_found and j not in used_truth:
            used_found.add(i)
            used_truth.add(j)
            out.append((i, j))
    return out


def box_color(rgb, fixed, box):
    """The median color (RGB) of a box's core, its pixels off the fixed map; None with fewer than MIN_CORE_PX."""
    cx, cy, w, h = box
    rows, columns = np.mgrid[0:rgb.shape[0], 0:rgb.shape[1]]
    core = ((columns - cx) / max(CORE_SHARE * w, 0.5)) ** 2 + ((rows - cy) / max(CORE_SHARE * h, 0.5)) ** 2 <= 1
    core &= ~fixed
    return np.median(rgb[core].astype(np.float32), 0) if core.sum() >= MIN_CORE_PX else None


def pixel_boxes(rgb, fixed, color):
    """The boxes the pixel rules give for a target of `color`: each part of its color with a target's shape, off the
    crop's edge and at least MIN_AREA_PX."""
    found = auto.parts(rgb, fixed, None, color)
    if found is None:
        return []
    labelled, count = found
    boxes = []
    for number in range(1, count + 1):
        box, area, solid, edge = auto.shape_of(labelled == number)
        if solid and not edge and area >= auto.MIN_AREA_PX:
            boxes.append(box)
    return boxes


class Tally:
    """A labeller's counts on a group of crops: the user's targets found, its boxes right, and the matched pairs' box
    errors."""

    def __init__(self):
        """Empty counts."""
        self.crops = self.truth = self.found = self.right = 0
        self.errors = []

    def add(self, found, truth):
        """Counts one crop's boxes against the user's."""
        pairs = matched(found, truth)
        self.crops += 1
        self.truth += len(truth)
        self.found += len(found)
        self.right += len(pairs)
        for i, j in pairs:
            self.errors.append([abs(found[i][k] - truth[j][k]) for k in range(4)])

    def summary(self):
        """Recall, precision and the median box error (dx, dy, dw, dh in pixels), with the counts."""
        error = np.median(np.array(self.errors), 0).round(2).tolist() if self.errors else None
        return {
            "crops": self.crops, "targets": self.truth, "boxes": self.found, "right": self.right,
            "recall": round(self.right / self.truth, 3) if self.truth else None,
            "precision": round(self.right / self.found, 3) if self.found else None,
            "error_px": error,
        }


def load_set(folder):
    """A set's checked crops: (row, rgb, fixed) each, crops whose file is missing left out."""
    out = []
    for line in open(folder / CHECKED, encoding="utf-8"):
        row = json.loads(line)
        path = folder / row["file"]
        if not path.exists():
            continue
        crop = np.load(path)
        out.append((row, crop["rgb"], crop["fixed"].astype(bool)))
    return out


def recording_colors(crops):
    """Each crop's index's recording and each recording's target colors: [(crop index, color)] from its user boxes."""
    colors = defaultdict(list)
    for index, (row, rgb, fixed) in enumerate(crops):
        for box in row["boxes"]:
            color = box_color(rgb, fixed, box)
            if color is not None:
                colors[recording_of(row["file"])].append((index, color))
    return colors


def score_set(folder):
    """Each labeller's tallies on a set, by crop source and in all."""
    crops = load_set(folder)
    colors = recording_colors(crops)
    tallies = defaultdict(Tally)
    for index, (row, rgb, fixed) in enumerate(crops):
        truth = row["boxes"]
        for group in ("all", row["source"]):
            tallies[("model", group)].add(row["model"], truth)
        others = [color for at, color in colors[recording_of(row["file"])] if at != index]
        if others:
            found = pixel_boxes(rgb, fixed, np.median(np.array(others), 0))
            agreed = [found[i] for i, _ in matched(found, row["model"])]
            for group in ("all", row["source"]):
                tallies[("pixels", group)].add(found, truth)
                tallies[("agree", group)].add(agreed, truth)
    return {f"{labeller} {group}": tally.summary() for (labeller, group), tally in sorted(tallies.items())}


def main():
    """Scores the labellers on each set asked for and prints and writes the table."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--sets", default=",".join(SETS))
    parser.add_argument("--out", default=str(folder("data") / "label_score"))
    args = parser.parse_args()
    root = folder("data") / "vod_model"
    results = {}
    for name in args.sets.split(","):
        results[name] = score_set(root / name)
        print(f"\n{name}")
        for key, summary in results[name].items():
            print(f"  {key:32} {json.dumps(summary)}")
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    (out / "label_score.json").write_text(json.dumps(results, indent=1), encoding="utf-8")


if __name__ == "__main__":
    main()
