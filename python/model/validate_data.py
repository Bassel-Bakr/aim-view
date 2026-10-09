"""Check the dataset (REPRODUCE.md step 1b): counts per split, label statistics, files that fail to load, scenario
leakage between splits, and a contact sheet of random crops with their boxes for a visual check.
Usage: python python/model/validate_data.py [--data test_out/vod_model/data] [--sheet test_out/vod_model/sheet.png]
Writes validation.json into the dataset folder, and the contact sheet.
"""
import argparse
import json
import random
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from local_config import folder  # noqa: E402

CROP_PX = 256
BOX_VALUES = 4                      # cx, cy, w, h
SHEET_COLUMNS, SHEET_ROWS = 8, 6
# magenta: the contact sheet shows the fixed map's pixels in it
FIXED_TINT = np.array([255, 0, 255])
FIXED_KEEP, FIXED_TINT_SHARE = 0.4, 0.6   # a fixed pixel's own color and the tint, mixed
BOX_COLOR = (0, 255, 0)
BOX_MARGIN_PX = 2


def leaks(rows):
    """The scenario folders of kept VODs that are in more than one split."""
    folders = {}
    for row in rows:
        if row["kept"]:
            folders.setdefault(row["folder"], set()).add(row["split"])
    return [folder for folder, splits in folders.items() if len(splits) > 1]


def split_report(files):
    """A split's crop count, unreadable files, box count and sizes, crops without targets and fixed-map share."""
    box_counts, sizes, empty, bad, fixed_share = [], [], 0, 0, []
    for path in files:
        try:
            crop = np.load(path)
            boxes = crop["boxes"]
            assert crop["rgb"].shape == (CROP_PX, CROP_PX, 3) and crop["fixed"].shape == (CROP_PX, CROP_PX) \
                and boxes.shape[1] == BOX_VALUES
        except Exception:
            bad += 1
            continue
        box_counts.append(len(boxes))
        sizes += list(np.maximum(boxes[:, 2], boxes[:, 3]))
        empty += len(boxes) == 0
        fixed_share.append(float(crop["fixed"].mean()))
    sizes = np.array(sizes)
    return dict(crops=len(files), unreadable=bad, boxes=int(sum(box_counts)), crops_without_targets=empty,
                box_px_p5_p50_p95=[float(np.percentile(sizes, q)) for q in (5, 50, 95)] if len(sizes) else None,
                fixed_share_mean=round(float(np.mean(fixed_share)), 4) if fixed_share else None)


def contact_sheet(files, path):
    """48 random train crops with their boxes, the fixed map tinted magenta."""
    pick = random.Random(0).sample(files, min(SHEET_COLUMNS * SHEET_ROWS, len(files)))
    sheet = Image.new("RGB", (SHEET_COLUMNS * CROP_PX, SHEET_ROWS * CROP_PX))
    for k, file in enumerate(pick):
        crop = np.load(file)
        pixels = np.asarray(Image.fromarray(crop["rgb"])).copy()
        fixed = crop["fixed"].astype(bool)
        pixels[fixed] = (pixels[fixed] * FIXED_KEEP + FIXED_TINT * FIXED_TINT_SHARE).astype(np.uint8)
        image = Image.fromarray(pixels)
        draw = ImageDraw.Draw(image)
        for cx, cy, width, height in crop["boxes"]:
            draw.rectangle([cx - width / 2 - BOX_MARGIN_PX, cy - height / 2 - BOX_MARGIN_PX,
                            cx + width / 2 + BOX_MARGIN_PX, cy + height / 2 + BOX_MARGIN_PX], outline=BOX_COLOR)
        sheet.paste(image, ((k % SHEET_COLUMNS) * CROP_PX, (k // SHEET_COLUMNS) * CROP_PX))
    sheet.save(path)


def main():
    """Prints the leaks and each split's report, writes them to <data>/validation.json, and saves the contact sheet."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--data", default=str(folder("data") / "vod_model" / "data"))
    parser.add_argument("--sheet", default=str(folder("data") / "vod_model" / "sheet.png"))
    args = parser.parse_args()
    data = Path(args.data)
    rows = [json.loads(line) for line in open(data / "manifest.jsonl", encoding="utf-8")]
    print("scenario folders in more than one split:", leaks(rows) or "none")
    report = {}
    for split in ("train", "val", "test"):
        report[split] = split_report(sorted((data / split).glob("*.npz")))
        print(split, report[split])
    json.dump(report, open(data / "validation.json", "w"), indent=1)
    contact_sheet(sorted((data / "train").glob("*.npz")), args.sheet)
    print("contact sheet:", args.sheet)


if __name__ == "__main__":
    main()
