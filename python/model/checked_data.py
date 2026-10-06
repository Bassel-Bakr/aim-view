"""A training set from crops checked by eye (label_check.py's checked.jsonl format, as the phone page writes it): each
crop's npz copied with its boxes replaced by the checked ones ("skip": no target there, no boxes) and its target mask
made again: a row drawn on the app's Crops page brings its targets' visible pixels ("mask", from the core: see
crop_check/labels.py), any other gets the ellipse that fills each box (as build_data.py's model labels and
build_mined.py make it).
The rest of the npz is kept as it is, and so is the crop's split (train/, val/, test/). Crops checked "unsure" are
left out, and so are the files --leave-out names (a slip on the label page). A row's "covered" boxes (a target
hidden under the crosshair) go into the npz as "ignore": train.py learns neither a target nor wall there. The
manifest is copied, so validate_data.py reads the new set.

Each crop's name gets a 10-character tag in front (--tag), so train.py --repeat can weight these crops alone: their
recordings' hashes also start other sets' crops of the same recordings.
Usage: python python/model/checked_data.py --labels <set>/checked_phone.jsonl --out <new set> --tag chk_theme_
       [--leave-out <file>,...]
"""
import argparse
import collections
import json
import shutil
from pathlib import Path

import numpy as np

CROP = 256
TAG_CHARS = 10                      # train.py --repeat reads a crop name's first 10 characters
BOX_VALUES = 4                      # cx, cy, w, h


def last_checks(labels):
    """Each crop's check, by its file: a later check of a crop wins."""
    last = {}
    for line in Path(labels).read_text(encoding="utf-8").splitlines():
        if line.strip():
            row = json.loads(line)
            last[row["file"]] = row
    return last


def ellipse_mask(boxes):
    """The target mask: the ellipse that fills each box."""
    target_mask = np.zeros((CROP, CROP), np.uint8)
    yy, xx = np.ogrid[0:CROP, 0:CROP]
    for box_x, box_y, box_w, box_h in boxes:
        target_mask[((xx - box_x) / max(1.0, box_w / 2)) ** 2 + ((yy - box_y) / max(1.0, box_h / 2)) ** 2 <= 1] = 1
    return target_mask


def mask_of(runs):
    """The target mask from run lengths over the crop's rows, the first of pixels not set (the core's shapes::runs)."""
    flat = np.zeros(CROP * CROP, np.uint8)
    start = 0
    for index, length in enumerate(runs):
        if index % 2:
            flat[start:start + length] = 1
        start += length
    return flat.reshape(CROP, CROP)


def write_crop(source, out, tag, file, row, counts, split=None):
    """One checked crop's npz in its split folder of `out` (or `split`), its boxes and mask replaced and its covered
    boxes added."""
    crop = np.load(source / file)
    boxes = np.array(row["boxes"] if row["verdict"] == "correct" else [], np.float32).reshape(-1, BOX_VALUES)
    split = split or Path(file).parent.name
    (out / split).mkdir(parents=True, exist_ok=True)
    extra = {}
    if row.get("covered"):                              # a target under the crosshair: train.py learns nothing there
        extra["ignore"] = np.array(row["covered"], np.float32).reshape(-1, BOX_VALUES)
        counts["ignore boxes"] += len(extra["ignore"])
    kept = {key: crop[key] for key in crop.files if key not in ("boxes", "tmask")}
    drawn = row.get("mask") is not None and row["verdict"] == "correct"
    tmask = mask_of(row["mask"]) if drawn else ellipse_mask(boxes)
    np.savez_compressed(out / split / f"{tag}{Path(file).name}", **kept, tmask=tmask, boxes=boxes, **extra)
    counts[split] += 1
    counts["boxes"] += len(boxes)
    counts["without targets"] += len(boxes) == 0


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--labels", required=True, help="checked.jsonl; its file paths are relative to its folder")
    parser.add_argument("--out", required=True)
    parser.add_argument("--tag", required=True, help="10 characters put before each crop's name (train.py --repeat's "
                        "key)")
    parser.add_argument("--leave-out", default="", help="comma-separated crop files (as the labels name them) to leave "
                        "out")
    parser.add_argument("--split", choices=("train", "val", "test"), help="every crop in this split, whatever its own "
                        "(crops checked to fix a model's mistakes on a scenario whose folder falls in val: the gate's "
                        "own runs stay the test)")
    args = parser.parse_args()
    if len(args.tag) != TAG_CHARS:
        parser.error("--tag must be 10 characters: train.py --repeat reads a name's first 10")
    source, out = Path(args.labels).parent, Path(args.out)
    if out.exists() and any(out.glob("*/*.npz")):
        raise SystemExit(f"{out} has crops already: give another folder")
    last = last_checks(args.labels)
    leave = {file for file in args.leave_out.split(",") if file}
    if leave - set(last):
        raise SystemExit(f"--leave-out names crops the labels do not have: {sorted(leave - set(last))}")
    counts = collections.Counter()
    for file, row in sorted(last.items()):
        if row["verdict"] == "unsure" or file in leave:
            counts["left out"] += 1
            continue
        if row["verdict"] not in ("correct", "skip"):
            raise SystemExit(f"{file}: verdict {row['verdict']!r}")
        write_crop(source, out, args.tag, file, row, counts, args.split)
    if (source / "manifest.jsonl").is_file():
        shutil.copyfile(source / "manifest.jsonl", out / "manifest.jsonl")
    print(dict(counts), "in", out)


if __name__ == "__main__":
    main()
