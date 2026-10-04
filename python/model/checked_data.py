"""A training set from crops checked by eye (label_check.py's checked.jsonl format, as the phone page writes it): each
crop's npz copied with its boxes replaced by the checked ones ("skip": no target there, no boxes) and its target mask
made again from them (the ellipse that fills each box, as build_data.py's model labels and build_mined.py make it).
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


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--labels", required=True, help="checked.jsonl; its file paths are relative to its folder")
    ap.add_argument("--out", required=True)
    ap.add_argument("--tag", required=True, help="10 characters put before each crop's name (train.py --repeat's key)")
    ap.add_argument("--leave-out", default="", help="comma-separated crop files (as the labels name them) to leave out")
    a = ap.parse_args()
    if len(a.tag) != 10:
        ap.error("--tag must be 10 characters: train.py --repeat reads a name's first 10")
    src, out = Path(a.labels).parent, Path(a.out)
    if out.exists() and any(out.glob("*/*.npz")):
        raise SystemExit(f"{out} has crops already: give another folder")
    last = {}
    for line in Path(a.labels).read_text(encoding="utf-8").splitlines():
        if line.strip():
            r = json.loads(line)
            last[r["file"]] = r                         # a later check of a crop wins
    leave = {f for f in a.leave_out.split(",") if f}
    if leave - set(last):
        raise SystemExit(f"--leave-out names crops the labels do not have: {sorted(leave - set(last))}")
    n = collections.Counter()
    yy, xx = np.ogrid[0:CROP, 0:CROP]
    for f, r in sorted(last.items()):
        if r["verdict"] == "unsure" or f in leave:
            n["left out"] += 1
            continue
        if r["verdict"] not in ("correct", "skip"):
            raise SystemExit(f"{f}: verdict {r['verdict']!r}")
        z = np.load(src / f)
        bb = np.array(r["boxes"] if r["verdict"] == "correct" else [], np.float32).reshape(-1, 4)
        tmask = np.zeros((CROP, CROP), np.uint8)
        for bx, by, bw, bh in bb:
            tmask[((xx - bx) / max(1.0, bw / 2)) ** 2 + ((yy - by) / max(1.0, bh / 2)) ** 2 <= 1] = 1
        split = Path(f).parent.name
        (out / split).mkdir(parents=True, exist_ok=True)
        extra = {}
        if r.get("covered"):                            # a target under the crosshair: train.py learns nothing there
            extra["ignore"] = np.array(r["covered"], np.float32).reshape(-1, 4)
            n["ignore boxes"] += len(extra["ignore"])
        np.savez_compressed(out / split / f"{a.tag}{Path(f).name}", **{k: z[k] for k in z.files if k not in
                                                                        ("boxes", "tmask")}, tmask=tmask, boxes=bb,
                            **extra)
        n[split] += 1
        n["boxes"] += len(bb)
        n["without targets"] += len(bb) == 0
    if (src / "manifest.jsonl").is_file():
        shutil.copyfile(src / "manifest.jsonl", out / "manifest.jsonl")
    print(dict(n), "in", out)


if __name__ == "__main__":
    main()
