"""Labels a folder of crops with Grounding DINO before the user checks them on the Crops page (crop_check/README.md):
the user reviews a pretrained detector's boxes instead of drawing every target. Each crop file's boxes become the
teacher's (the boxes it had are kept as model_boxes and model_scores), with a pill-shaped target mask (the Crops page
shows a box as a pill), and why says which teacher and prompt. Run make_page.py on the folder after it.

Each box is snapped to the target's pixels (`snap`): Grounding DINO's boxes are loose. On the 16 Centering crops whose
box the user fixed (2026-10-06), its width was 3.4 px too wide (the user's: 0.61 of it) and its height 1.7 px too
tall; snapped, 0.2 and 0.3 px off. A box whose target does not stand out from the wall round it keeps its size.

The prompt and threshold depend on the targets: pick them on a sample first. Grounding DINO base (2026-10-05): robots
"humanoid robot . person ." at 0.5; the thin black pill bots of the Centering scenarios "black pole . black stick ."
at 0.3 (all 12 of a sample boxed whole, where "capsule . cylinder ." found 8 and the robot prompt none). It is far
worse than our own model on small targets (spheres, tiles): there, the user labels first.

Usage: python python/model/teacher_label.py <crop folder> --prompt "black pole . black stick ." [--threshold 0.3]
"""
import argparse
from pathlib import Path

import numpy as np
import torch
from PIL import Image

TEACHER = "IDEA-Research/grounding-dino-base"
UPSCALE = 3                     # crops are 256 px; the detector works at 800 px and more
SAME_TARGET = 0.5               # two boxes whose centers are within half the other's size are one target (each
                                # phrase of the prompt finds it)
SNAP_MARGIN_PX = 3              # the snap looks this far past the box's top and bottom, and a box's width either side
WALL_COLUMNS = 2                # the wall's color: the window's outermost columns either side
TARGET_SHARE = 0.3              # a pixel is the target's where it differs from the wall by this share of the most
COVERAGE_FLOOR = 0.05           # under this share a pixel is the wall's texture, not a target's edge
MIN_CONTRAST = 40               # the target must differ from the wall by this much (RGB distance) to be snapped
MIN_SNAPPED_PX = 2.0


def snap(rgb, box):
    """A box (cx, cy, w, h) moved and sized to its target's pixels: within a window a box's width either side and
    SNAP_MARGIN_PX past its ends, each pixel's coverage is its color distance from the wall (the window's outer
    columns) over the most (under COVERAGE_FLOOR: none). The target's rows are those with a pixel over TARGET_SHARE,
    its height theirs and its middle between them; its width is the coverage summed over the columns per row (an
    anti-aliased edge counts by its share), its middle the coverage's centroid. The box as it was when the target does
    not stand out (MIN_CONTRAST) or the snapped box would be under MIN_SNAPPED_PX."""
    cx, cy, width, height = box
    x0, x1 = int(max(0, cx - width)), int(min(rgb.shape[1], cx + width + 1))
    y0 = int(max(0, cy - height / 2 - SNAP_MARGIN_PX))
    y1 = int(min(rgb.shape[0], cy + height / 2 + SNAP_MARGIN_PX + 1))
    window = rgb[y0:y1, x0:x1].astype(np.float32)
    if window.shape[0] < MIN_SNAPPED_PX + 1 or window.shape[1] < 2 * WALL_COLUMNS + 1:
        return box
    wall = np.median(np.concatenate([window[:, :WALL_COLUMNS], window[:, -WALL_COLUMNS:]], 1).reshape(-1, 3), 0)
    contrast = np.linalg.norm(window - wall, axis=2)
    if contrast.max() < MIN_CONTRAST:
        return box
    coverage = contrast / contrast.max()
    coverage[coverage < COVERAGE_FLOOR] = 0
    rows = np.nonzero((coverage > TARGET_SHARE).any(1))[0]
    snapped_h = float(rows.max() - rows.min() + 1)
    snapped_w = float(coverage[rows.min():rows.max() + 1].sum() / snapped_h)
    if min(snapped_w, snapped_h) < MIN_SNAPPED_PX:
        return box
    columns = np.arange(x0, x1) + 0.5
    middle_x = float((coverage.sum(0) * columns).sum() / coverage.sum())
    return [middle_x, y0 + (rows.min() + rows.max() + 1) / 2, snapped_w, snapped_h]


class Teacher:
    def __init__(self, prompt, threshold, device):
        from transformers import AutoModelForZeroShotObjectDetection, AutoProcessor
        self.processor = AutoProcessor.from_pretrained(TEACHER)
        self.model = AutoModelForZeroShotObjectDetection.from_pretrained(TEACHER).to(device).eval()
        self.prompt, self.threshold, self.device = prompt, threshold, device

    @torch.no_grad()
    def boxes(self, rgb):
        """A crop's boxes (cx, cy, w, h in crop pixels) and scores, one per target, surest first."""
        size = rgb.shape[1] * UPSCALE, rgb.shape[0] * UPSCALE
        image = Image.fromarray(rgb).resize(size, Image.Resampling.BICUBIC)
        inputs = self.processor(images=image, text=self.prompt, return_tensors="pt").to(self.device)
        result = self.processor.post_process_grounded_object_detection(
            self.model(**inputs), inputs.input_ids, threshold=self.threshold, text_threshold=self.threshold,
            target_sizes=[size[::-1]])[0]
        kept = []
        for score, (x0, y0, x1, y1) in sorted(zip(result["scores"].tolist(), result["boxes"].tolist()),
                                              key=lambda pair: -pair[0]):
            box = [(x0 + x1) / 2 / UPSCALE, (y0 + y1) / 2 / UPSCALE, (x1 - x0) / UPSCALE, (y1 - y0) / UPSCALE]
            if all(abs(box[0] - other[0]) > SAME_TARGET * other[2] or abs(box[1] - other[1]) > SAME_TARGET * other[3]
                   for _, other in kept):
                kept.append((score, box))
        return [box for _, box in kept], [score for score, _ in kept]


def pill_mask(boxes, size):
    """The pixels of each box's pill (a stadium: its shorter side the width, round ends), as the Crops page draws a
    box."""
    mask = np.zeros(size, np.uint8)
    yy, xx = np.mgrid[0:size[0], 0:size[1]]
    for cx, cy, width, height in boxes:
        radius = min(width, height) / 2
        reach_x, reach_y = max(0.0, width / 2 - radius), max(0.0, height / 2 - radius)
        gap_x = np.maximum(np.abs(xx - cx) - reach_x, 0)
        gap_y = np.maximum(np.abs(yy - cy) - reach_y, 0)
        mask[gap_x ** 2 + gap_y ** 2 <= radius ** 2] = 1
    return mask


def label(path, teacher, note):
    """One crop file relabelled by the teacher; whether it boxed anything."""
    crop = dict(np.load(path, allow_pickle=True))
    boxes, scores = teacher.boxes(crop["rgb"])
    boxes = [snap(crop["rgb"], box) for box in boxes]
    if "model_boxes" not in crop:
        crop["model_boxes"], crop["model_scores"] = crop["boxes"], crop.get("scores", np.zeros(0, np.float32))
    crop["boxes"] = np.array(boxes, np.float32).reshape(-1, 4)
    crop["scores"] = np.array(scores, np.float32)
    crop["tmask"] = pill_mask(boxes, crop["rgb"].shape[:2])
    crop["why"] = np.str_(f"{note}; {crop['why']}" if "why" in crop else note)
    np.savez_compressed(path, **crop)
    return bool(boxes)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("folder", type=Path)
    parser.add_argument("--prompt", required=True)
    parser.add_argument("--threshold", type=float, default=0.3)
    args = parser.parse_args()
    teacher = Teacher(args.prompt, args.threshold, "cuda" if torch.cuda.is_available() else "cpu")
    note = f"Grounding DINO: \"{args.prompt}\" at {args.threshold}"
    files = sorted(args.folder.glob("**/*.npz"))
    boxed = sum(label(path, teacher, note) for path in files)
    print(f"{boxed} of {len(files)} crops boxed by {note}")


if __name__ == "__main__":
    main()
