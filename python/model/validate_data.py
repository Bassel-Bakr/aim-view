"""Check the dataset (REPRODUCE.md step 1b): counts per split, label statistics, files that fail to load, scenario
leakage between splits, and a contact sheet of random crops with their boxes for a visual check.
Usage: python python/model/validate_data.py [--data test_out/vod_model/data] [--sheet test_out/vod_model/sheet.png]
"""
import argparse
import json
import random
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--data", default="test_out/vod_model/data")
    ap.add_argument("--sheet", default="test_out/vod_model/sheet.png")
    a = ap.parse_args()
    d = Path(a.data)
    rows = [json.loads(l) for l in open(d / "manifest.jsonl", encoding="utf-8")]
    folders = {}
    for r in rows:
        if r["kept"]:
            folders.setdefault(r["folder"], set()).add(r["split"])
    leaks = [f for f, s in folders.items() if len(s) > 1]
    print("scenario folders in more than one split:", leaks or "none")
    report = {}
    for split in ("train", "val", "test"):
        files = sorted((d / split).glob("*.npz"))
        n_boxes, sizes, empty, bad, fixed_share = [], [], 0, 0, []
        for f in files:
            try:
                z = np.load(f)
                b = z["boxes"]
                assert z["rgb"].shape == (256, 256, 3) and z["fixed"].shape == (256, 256) and b.shape[1] == 4
            except Exception:
                bad += 1
                continue
            n_boxes.append(len(b))
            sizes += list(np.maximum(b[:, 2], b[:, 3]))
            empty += len(b) == 0
            fixed_share.append(float(z["fixed"].mean()))
        sizes = np.array(sizes)
        report[split] = dict(crops=len(files), unreadable=bad, boxes=int(sum(n_boxes)), crops_without_targets=empty,
                             box_px_p5_p50_p95=[float(np.percentile(sizes, q)) for q in (5, 50, 95)] if len(sizes) else None,
                             fixed_share_mean=round(float(np.mean(fixed_share)), 4) if fixed_share else None)
        print(split, report[split])
    json.dump(report, open(d / "validation.json", "w"), indent=1)
    files = sorted((d / "train").glob("*.npz"))
    pick = random.Random(0).sample(files, min(48, len(files)))
    sheet = Image.new("RGB", (8 * 256, 6 * 256))
    for k, f in enumerate(pick):
        z = np.load(f)
        im = Image.fromarray(z["rgb"])
        fx = z["fixed"].astype(bool)
        arr = np.asarray(im).copy()
        arr[fx] = (arr[fx] * 0.4 + np.array([255, 0, 255]) * 0.6).astype(np.uint8)
        im = Image.fromarray(arr)
        dr = ImageDraw.Draw(im)
        for cx, cy, w, h in z["boxes"]:
            dr.rectangle([cx - w / 2 - 2, cy - h / 2 - 2, cx + w / 2 + 2, cy + h / 2 + 2], outline=(0, 255, 0))
        sheet.paste(im, ((k % 8) * 256, (k // 8) * 256))
    sheet.save(a.sheet)
    print("contact sheet:", a.sheet)


if __name__ == "__main__":
    main()
