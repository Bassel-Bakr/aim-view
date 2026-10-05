"""Train the target detector (REPRODUCE.md step 2) from a JSON config in python/model/configs/.

Data: the crops from build_data.py (real frames, automatic labels). Augmentation on the GPU turns them into many looks
the library hardly has (synthetic, recorded as such in MODEL_STATUS.md): any wall color, any target color, wall
texture, blur and noise, outlines round the targets, and synthetic crosshairs (marked in the fixed map), sometimes drawn
over a target.
Writes runs/<name>/: config.json, metrics.jsonl (one line an epoch), best.pt (by validation F1), last.pt.
Usage: python python/model/train.py python/model/configs/small.json [--epochs N] [--out test_out/vod_model/runs]
"""
import argparse
import json
import math
import random
import shutil
import sys
import time
from pathlib import Path

import numpy as np
import torch
import torch.nn.functional as F
from torch.utils.data import DataLoader, Dataset

sys.path.insert(0, str(Path(__file__).resolve().parent))
import net  # noqa: E402

MAXBOX = 40
MAXIGNORE = 8
SAME_TARGET_PX = 1.5            # the labeller can report one target twice: two labels this close are one
HALF = 0.5                      # a coin toss; and a mask's 0/1 split

# recolour: the wall moved to a random color, the targets painted one that stands out
WALL_GROW, LOCAL_WINDOW = 5, 9  # the wall: 5 px from a target; the local background: averaged 9 px round
LOCAL_SHARE = 0.05              # where less of the window is wall, the wall's mean color stands in
MIN_CONTRAST = 0.05             # the luminance gap between wall and target taken as this at least
WALL_MIN_BRIGHTNESS, WALL_BRIGHTNESS_RANGE = 0.15, 0.85   # a new wall's color is scaled by 0.15 to 1
GAIN_MIN, GAIN_RANGE = 0.6, 0.8 # the wall's texture scaled by 0.6 to 1.4
TARGET_COLORS = 6               # candidate target colors drawn per crop
TARGET_FAR = 0.35               # a target color this far from the wall (sum of |RGB| differences) stands out
SHADING = 0.15                  # a little of the original shading kept on a painted target

# texture, outlines, crosshairs, decoder, blur and noise
TEXTURE_CELLS = 16              # the coarse texture: one random value per 16 px
TEXTURE_LOW, TEXTURE_HIGH = 0.06, 0.03
OUTLINE_MIN_PX, OUTLINE_MAX_PX = 1, 3
GLOW_SHARE = 0.3                # a glow: a little wider, with soft edges
LOUD_SHARE = 0.6                # outlines are often loud: one or two channels full
LOUD_HIGH, LOUD_LOW = 0.9, 0.1
OUTLINE_ALPHA_MIN, OUTLINE_ALPHA_RANGE = 0.7, 0.3
REAL_VISIBLE = 0.3              # a real crosshair's pixel shows in the fixed map from this alpha up
CROSSHAIR_EDGE_PX = 8           # a crosshair off the targets keeps this far from the crop's edge
REAL_MIN_PX, REAL_MAX_PX = 5, 40
TINT_FLOOR = 1e-3
DOT_RADIUS_PX = (0.8, 3.0)
PLUS_ARM_PX, PLUS_THICKNESS_PX, PLUS_GAP_PX = (3, 9), (0.6, 1.6), (0, 3)
RING_RADIUS_PX, RING_THICKNESS_PX = (3, 8), 0.8
OUTLINE_SIZES = (3, 5)
DARK_OUTLINE_SHARE, DARK_OUTLINE = 0.7, 0.2
DECODER_GAIN, DECODER_OFFSET = 0.04, 0.02
BLUR_SIGMA_PX = (0.3, 1.0)
NOISE = 0.02

# targets and loss
SIGMA_SIZE, MIN_SIGMA = 0.15, 0.6   # a center's Gaussian: 0.15 of its larger side (cells), 0.6 cells at least
SCORE_CLAMP = 1e-4
FOCAL_POWER_HIT, FOCAL_POWER_NEAR = 2, 4
WARM_UP_STEPS = 300
CLIP_NORM = 5.0
MATCH_MIN_PX = 2.0              # a prediction is a hit within max(2 px, half the target's smaller side)
FAR = 1e9
TINY = 1e-9
RECOLOR_SEEDS = 10 ** 6
SEED_STRIDE = 100003            # an epoch's order is seeded by seed * this + epoch
VAL_BATCH, VAL_WORKERS = 64, 4


class Crops(Dataset):
    def __init__(self, folders, repeat=(), times=1):
        """folders: one folder of crops or several; repeat: crop name prefixes (a VOD's 10-character hash, or
        "hand_crop_" for hand-checked crops) whose crops appear `times` times, or a dict of prefix: times."""
        folders = [folders] if isinstance(folders, (str, Path)) else folders
        self.files = sorted(file for folder in folders for file in Path(folder).glob("*.npz"))
        repeats = repeat if isinstance(repeat, dict) else {prefix: times for prefix in repeat}
        self.files += [file for file in self.files for _ in range(repeats.get(file.name[:10], 1) - 1)]

    def __len__(self):
        return len(self.files)

    def __getitem__(self, i):
        crop = np.load(self.files[i])
        keep = []                       # the labeller can report one target twice (overlapping search windows)
        for box in crop["boxes"]:
            if all(np.hypot(box[0] - kept[0], box[1] - kept[1]) > SAME_TARGET_PX for kept in keep):
                keep.append(box)
        boxes = np.zeros((MAXBOX, 4), np.float32)
        count = min(MAXBOX, len(keep))
        if count:
            boxes[:count] = np.array(keep[:count])
        ignore = np.zeros((MAXIGNORE, 4), np.float32)   # "ignore": a target there, but not one to learn (under the
        if "ignore" in crop.files:                      # crosshair); padding has width 0
            ignored = min(MAXIGNORE, len(crop["ignore"]))
            ignore[:ignored] = crop["ignore"][:ignored]
        return crop["rgb"], crop["fixed"], crop["tmask"], boxes, count, ignore


# ---- augmentation (batched, on the GPU) -----------------------------------------------------------------------------
def lum(image):
    return 0.299 * image[:, 0:1] + 0.587 * image[:, 1:2] + 0.114 * image[:, 2:3]


def flip_rot(image, fixed, target_mask, boxes, counts):
    """Random flips and quarter turns per batch (the same for the whole batch; boxes follow)."""
    size = image.shape[-1]
    turns = random.randint(0, 3)
    # positions are pixel indices (0 to size - 1), so a flip maps x to size - 1 - x
    if random.random() < HALF:
        image, fixed, target_mask = image.flip(-1), fixed.flip(-1), target_mask.flip(-1)
        boxes[..., 0] = size - 1 - boxes[..., 0]
    for _ in range(turns):                                   # rotate 90 deg counter-clockwise: (x, y) -> (y, S - 1 - x)
        image, fixed = image.rot90(1, (-2, -1)), fixed.rot90(1, (-2, -1))
        target_mask = target_mask.rot90(1, (-2, -1))
        x, y, width, height = boxes.unbind(-1)
        boxes = torch.stack([y, size - 1 - x, height, width], -1)
    return image, fixed, target_mask, boxes


def recolour(image, target_mask, theme_share=0.7, target_share=0.8):
    """A new theme: the wall's color moved to a random color (texture and edges kept, relative to it), and the
    targets painted a random color that still stands out, with their anti-aliased edges blended again."""
    batch = image.shape[0]
    device = image.device
    targets = target_mask.float()
    wall_mask = (F.max_pool2d(targets, WALL_GROW, 1, WALL_GROW // 2) == 0).float()
    wall = (image * wall_mask).sum((2, 3), keepdim=True) / wall_mask.sum((2, 3), keepdim=True).clamp(min=1)
    # local background: the image with the targets cut out, averaged nearby
    wall_sum = F.avg_pool2d(image * wall_mask, LOCAL_WINDOW, 1, LOCAL_WINDOW // 2)
    wall_share = F.avg_pool2d(wall_mask, LOCAL_WINDOW, 1, LOCAL_WINDOW // 2)
    background = torch.where(wall_share > LOCAL_SHARE, wall_sum / wall_share.clamp(min=1e-3), wall.expand_as(image))
    target_color = (image * targets).sum((2, 3), keepdim=True) / targets.sum((2, 3), keepdim=True).clamp(min=1)
    # how much of each pixel near a target is target (anti-aliasing), from its luminance between wall and target
    near = F.max_pool2d(targets, 3, 1, 1)
    gap = lum(background) - lum(target_color)
    coverage = ((lum(background) - lum(image)) / gap.abs().clamp(min=MIN_CONTRAST) * torch.sign(gap))
    alpha = (coverage.clamp(0, 1) * near).clamp(0, 1)
    alpha = torch.maximum(alpha, targets)
    theme = (torch.rand(batch, 1, 1, 1, device=device) < theme_share).float()
    new_wall = torch.rand(batch, 3, 1, 1, device=device) * (
        WALL_MIN_BRIGHTNESS + WALL_BRIGHTNESS_RANGE * torch.rand(batch, 1, 1, 1, device=device))
    gain = GAIN_MIN + GAIN_RANGE * torch.rand(batch, 1, 1, 1, device=device)
    image_on_wall = theme * (new_wall + (image - wall) * gain) + (1 - theme) * image
    background_on_wall = theme * (new_wall + (background - wall) * gain) + (1 - theme) * background
    # target color: random, at least 0.35 (sum of |RGB| differences) away from the wall
    candidates = torch.rand(batch, TARGET_COLORS, 3, device=device)
    wall_color = (theme * new_wall + (1 - theme) * wall).view(batch, 1, 3)
    far = (candidates - wall_color).abs().sum(-1) > TARGET_FAR
    chosen = torch.where(far.any(1), far.float().argmax(1), torch.zeros(batch, dtype=torch.long, device=device))
    new_target = candidates[torch.arange(batch, device=device), chosen].view(batch, 3, 1, 1)
    paint = (torch.rand(batch, 1, 1, 1, device=device) < target_share).float()
    shade = 1 + SHADING * (lum(image) - lum(target_color))   # keep a little of the original shading
    painted = paint * (new_target * shade).clamp(0, 1) + (1 - paint) * image
    out = alpha * painted + (1 - alpha) * image_on_wall
    out = torch.where(alpha > 0, alpha * painted + (1 - alpha) * background_on_wall, image_on_wall)
    return out.clamp(0, 1)


def texture(image, target_mask, share=0.3):
    batch, _, size, _ = image.shape
    device = image.device
    on = (torch.rand(batch, 1, 1, 1, device=device) < share).float()
    coarse = F.interpolate(torch.randn(batch, 3, size // TEXTURE_CELLS, size // TEXTURE_CELLS, device=device),
                           size=(size, size), mode="bilinear", align_corners=False)
    fine = torch.randn(batch, 1, size, size, device=device)
    coarse_amplitude = TEXTURE_LOW * torch.rand(batch, 1, 1, 1, device=device)
    fine_amplitude = TEXTURE_HIGH * torch.rand(batch, 1, 1, 1, device=device)
    wall = (F.max_pool2d(target_mask.float(), WALL_GROW, 1, WALL_GROW // 2) == 0).float()
    return (image + on * wall * (coarse_amplitude * coarse + fine_amplitude * fine)).clamp(0, 1)


def outlines(image, target_mask, fixed, share=0.0):
    """KovaaK's can draw an outline round each bot: a ring of one color, 1 to 3 px wide at 720p, sometimes soft like a
    glow. Drawn round the labelled targets of a share of the crops, following their shape (a round dilation); the
    crosshair (the fixed map) is never outlined and stays on top, as in the game. The labels stay the targets.
    One width and style per batch, a color per crop: done for the whole batch at once."""
    if share <= 0:
        return image
    batch = image.shape[0]
    device = image.device
    pick = (torch.rand(batch, 1, 1, 1, device=device) < share).float()
    targets = ((target_mask > HALF) & (fixed < HALF)).float()
    width = random.randint(OUTLINE_MIN_PX, OUTLINE_MAX_PX)
    soft = random.random() < GLOW_SHARE                         # a glow: a little wider, with soft edges
    reach = width + 1 if soft else width
    yy, xx = torch.meshgrid(torch.arange(-reach, reach + 1, device=device), torch.arange(-reach, reach + 1,
                                                                                         device=device), indexing="ij")
    disk = ((xx ** 2 + yy ** 2) <= reach * reach + 0.5).float()[None, None]
    ring = (F.conv2d(targets, disk, padding=reach) > 0).float() * (1 - targets)
    if soft:
        ring = F.avg_pool2d(ring, 3, 1, 1) * (1 - targets)
    color = torch.rand(batch, 3, 1, 1, device=device)
    loud = (torch.rand(batch, 1, 1, 1, device=device) < LOUD_SHARE).float()
    color = loud * ((color > HALF).float() * LOUD_HIGH + LOUD_LOW * color) + (1 - loud) * color
    alpha = (OUTLINE_ALPHA_MIN + OUTLINE_ALPHA_RANGE * torch.rand(batch, 1, 1, 1, device=device)) * ring * (1 - fixed) \
        * pick
    return image * (1 - alpha) + color * alpha


KOVAAKS_CROSSHAIRS = r"C:\Program Files (x86)\Steam\steamapps\common\FPSAimTrainer\FPSAimTrainer\crosshairs"
_REAL = {}


def real_crosshairs(folder, device="cpu"):
    """The crosshair images a KovaaK's install has (RGBA PNGs: 45 here, from small dots to big circles), as float
    tensors 4 x h x w, loaded once per device (a copy to the GPU for each crosshair drawn would wait for it). Empty
    when the folder is missing."""
    key = (folder, str(device))
    if key not in _REAL:
        from PIL import Image
        files = sorted(Path(folder).glob("*.png")) if folder and Path(folder).is_dir() else []
        _REAL[key] = [torch.from_numpy(np.asarray(Image.open(file).convert("RGBA"), np.float32) / 255)
                      .permute(2, 0, 1).to(device) for file in files]
    return _REAL[key]


def paste_real(image, fixed, crop, cx, cy, picture):
    """One real crosshair image (4 x h x w) alpha-blended into a crop, centered on (cx, cy), and its visible pixels
    marked in the fixed map, as the key frames mark the game's crosshair."""
    size = image.shape[-1]
    height, width = picture.shape[1:]
    x0, y0 = int(round(cx - width / 2)), int(round(cy - height / 2))
    left, top, right, bottom = max(0, x0), max(0, y0), min(size, x0 + width), min(size, y0 + height)
    if left >= right or top >= bottom:
        return
    part = picture[:, top - y0:bottom - y0, left - x0:right - x0]
    alpha = part[3:4]
    image[crop, :, top:bottom, left:right] = image[crop, :, top:bottom, left:right] * (1 - alpha) + part[:3] * alpha
    fixed[crop, 0, top:bottom, left:right] = torch.maximum(fixed[crop, 0, top:bottom, left:right],
                                                           (alpha[0] > REAL_VISIBLE).to(fixed.dtype))


def crosshair_center(boxes, counts, crop, size, on_target, jitter):
    """Where a crop's crosshair goes: on one of its targets (a share on_target of the time), moved off its center by up
    to jitter times its size, else anywhere."""
    if counts[crop] > 0 and random.random() < on_target:
        j = random.randrange(int(counts[crop]))
        cx, cy = float(boxes[crop, j, 0]), float(boxes[crop, j, 1])
        reach = jitter * float(boxes[crop, j, 2:].max())
        return cx + random.uniform(-reach, reach), cy + random.uniform(-reach, reach)
    return random.uniform(CROSSHAIR_EDGE_PX, size - CROSSHAIR_EDGE_PX), random.uniform(CROSSHAIR_EDGE_PX,
                                                                                          size - CROSSHAIR_EDGE_PX)


def real_picture(pictures, device):
    """One of KovaaK's crosshair images, 5 to 40 px across, half of them tinted."""
    picture = random.choice(pictures).to(device)
    scale = random.uniform(REAL_MIN_PX, REAL_MAX_PX) / max(picture.shape[1:])
    picture = F.interpolate(picture[None], size=(max(1, round(picture.shape[1] * scale)),
                                                 max(1, round(picture.shape[2] * scale))),
                            mode="bilinear", align_corners=False)[0].clamp(0, 1)
    if random.random() < HALF:
        tint = torch.rand(3, 1, 1, device=device)
        picture = torch.cat([picture[:3] * tint / tint.max().clamp(min=TINT_FLOOR), picture[3:]])
    return picture


def drawn_shape(dx, dy):
    """A synthetic crosshair's pixels: a dot, a plus or a ring, by their offsets from its center."""
    kind = random.choice(("dot", "dot", "plus", "ring"))
    if kind == "dot":
        return dx ** 2 + dy ** 2 <= random.uniform(*DOT_RADIUS_PX) ** 2
    if kind == "plus":
        arm, thickness, gap = (random.uniform(*PLUS_ARM_PX), random.uniform(*PLUS_THICKNESS_PX),
                               random.uniform(*PLUS_GAP_PX))
        return ((dx.abs() <= thickness) & (dy.abs() <= arm) & (dy.abs() >= gap)) | \
            ((dy.abs() <= thickness) & (dx.abs() <= arm) & (dx.abs() >= gap))
    radius = random.uniform(*RING_RADIUS_PX)
    return ((dx ** 2 + dy ** 2).sqrt() - radius).abs() <= RING_THICKNESS_PX


def crosshairs(image, fixed, boxes, counts, share=0.5, on_target=0.5, jitter=0.0, outline=0.0, real=0.0,
               folder=KOVAAKS_CROSSHAIRS):
    """Synthetic crosshairs (dot, plus or ring, any color) drawn into the image and the fixed map. A share of them
    (on_target) sit on a target, which stays a target: the model must see a target through a crosshair, as in hold-fire
    runs. jitter moves that crosshair off the target's center by up to jitter times the target's size, as when a player
    holds slightly off (v2; v1 drew it dead center). A share (outline) get a 1 or 2 px edge, mostly dark, as Aim Lab's
    red cross has: every model up to small_v6 took that crosshair for a target. A share (real) are KovaaK's own
    crosshair images instead (real_crosshairs()), 5 to 40 px across, half of them tinted, as the game's crosshair color
    does: small_v10 took the user's and other players' crosshairs for targets again."""
    batch, _, size, _ = image.shape
    device = image.device
    yy, xx = torch.meshgrid(torch.arange(size, device=device), torch.arange(size, device=device), indexing="ij")
    # the boxes read once on the CPU, and the pixels painted with where() rather than a mask's index: neither makes
    # the loop wait for the GPU crop by crop
    boxes = boxes.cpu()
    for crop in range(batch):
        if random.random() > share:
            continue
        cx, cy = crosshair_center(boxes, counts, crop, size, on_target, jitter)
        pictures = real_crosshairs(folder, device) if real else []
        if pictures and random.random() < real:
            paste_real(image, fixed, crop, cx, cy, real_picture(pictures, device))
            continue
        drawn = drawn_shape(xx - cx, yy - cy)
        color = torch.rand(3, 1, device=device)
        image[crop] = torch.where(drawn, color[:, :, None], image[crop])
        fixed[crop, 0].masked_fill_(drawn, 1)
        if random.random() < outline:
            grow = random.choice(OUTLINE_SIZES)
            edge = (F.max_pool2d(drawn[None, None].float(), grow, 1, grow // 2)[0, 0] > 0) & ~drawn
            dark = random.random() < DARK_OUTLINE_SHARE
            edge_color = torch.rand(3, 1, device=device) * (DARK_OUTLINE if dark else 1.0)
            image[crop] = torch.where(edge, edge_color[:, :, None], image[crop])
            fixed[crop, 0].masked_fill_(edge, 1)
    return image, fixed


def decoder(image, share=0.0):
    """Another decoder's colors: the chroma re-sampled at half resolution (nearest or smooth), and small per-channel
    gains and offsets, as a different YUV-to-RGB conversion gives (browser, GPU, other recorders). A share of the
    crops, done for the whole batch at once."""
    if share <= 0:
        return image
    batch = image.shape[0]
    device = image.device
    pick = (torch.rand(batch, 1, 1, 1, device=device) < share).float()
    luma = lum(image)
    chroma = F.interpolate(F.avg_pool2d(image - luma, 2), scale_factor=2, mode=random.choice(("nearest", "bilinear")))
    gain = 1 + (torch.rand(batch, 3, 1, 1, device=device) - HALF) * DECODER_GAIN
    offset = (torch.rand(batch, 3, 1, 1, device=device) - HALF) * DECODER_OFFSET
    out = ((luma + chroma) * gain + offset).clamp(0, 1)
    return pick * out + (1 - pick) * image


def blur_noise(image, share=0.5):
    batch = image.shape[0]
    device = image.device
    if random.random() < share:
        sigma = random.uniform(*BLUR_SIGMA_PX)
        kernel = torch.exp(-torch.arange(-2, 3, device=device, dtype=torch.float32) ** 2 / (2 * sigma * sigma))
        kernel = kernel / kernel.sum()
        image = F.conv2d(image.reshape(-1, 1, *image.shape[2:]), kernel.view(1, 1, 1, 5), padding=(0, 2))
        image = F.conv2d(image, kernel.view(1, 1, 5, 1), padding=(2, 0)).reshape(batch, 3, *image.shape[2:])
    image = image + torch.randn_like(image) * NOISE * torch.rand(batch, 1, 1, 1, device=device)
    return image.clamp(0, 1)


def augment(rgb, fixed, target_mask, boxes, counts, config, ignore=None):
    """The augmented batch and its boxes; with ignore boxes, those too (moved as the boxes are)."""
    image = rgb.permute(0, 3, 1, 2).float() / 255.0
    fixed = fixed[:, None].float()
    target_mask = target_mask[:, None].float()
    box_slots = boxes.shape[1]
    image, fixed, target_mask, boxes = flip_rot(image, fixed, target_mask,
                                                boxes if ignore is None else torch.cat([boxes, ignore], 1), counts)
    if ignore is not None:
        boxes, ignore = boxes[:, :box_slots], boxes[:, box_slots:]
    shares = config["augment"]
    image = recolour(image, target_mask, shares["theme"], shares["target"])
    image = texture(image, target_mask, shares["texture"])
    image = outlines(image, target_mask, fixed, shares.get("outline", 0.0))
    image, fixed = crosshairs(image, fixed, boxes, counts, shares["crosshair"], shares.get("crosshair_on_target", 0.5),
                              shares.get("crosshair_jitter", 0.0), shares.get("crosshair_outline", 0.0),
                              shares.get("crosshair_real", 0.0))
    image = decoder(image, shares.get("decoder", 0.0))
    image = blur_noise(image, shares["blur"])
    return (torch.cat([image, fixed], 1), boxes) + (() if ignore is None else (ignore,))


# ---- targets and loss -----------------------------------------------------------------------------------------------
def targets(boxes, counts, size):
    """Heatmap (B, 1, S/4, S/4) with a Gaussian at every center, and the regression targets at center cells."""
    batch = boxes.shape[0]
    cells = size // net.STRIDE
    device = boxes.device
    valid = torch.arange(boxes.shape[1], device=device)[None] < counts.to(device)[:, None]
    centers = boxes[..., :2] / net.STRIDE
    sides = boxes[..., 2:].clamp(min=1.0)
    sigma = (SIGMA_SIZE * sides.max(-1).values / net.STRIDE).clamp(min=MIN_SIGMA)
    yy, xx = torch.meshgrid(torch.arange(cells, device=device), torch.arange(cells, device=device), indexing="ij")
    distance2 = (xx[None, None] + 0.5 - centers[..., 0, None, None]) ** 2 + \
        (yy[None, None] + 0.5 - centers[..., 1, None, None]) ** 2
    gaussians = torch.exp(-distance2 / (2 * sigma[..., None, None] ** 2)) * valid[..., None, None]
    heatmap = gaussians.max(1).values[:, None]
    center_cells = centers.floor().long().clamp(0, cells - 1)
    peak = torch.zeros(batch, 1, cells, cells, device=device)
    reg = torch.zeros(batch, 4, cells, cells, device=device)
    # every box's cell at once, its crop and slot found on the CPU (counts is there) and the cells read back in one
    # copy; where two boxes share a cell the later one's values stand, as writing them box by box left them
    crops, slots = (torch.arange(boxes.shape[1])[None] < counts.cpu()[:, None]).nonzero(as_tuple=True)
    cells_xy = center_cells.cpu()[crops, slots]
    keys = ((crops * cells + cells_xy[:, 1]) * cells + cells_xy[:, 0]).tolist()
    last = sorted({key: i for i, key in enumerate(keys)}.values())
    crops, slots, cells_xy = crops[last].to(device), slots[last].to(device), cells_xy[last].to(device)
    x, y = cells_xy[:, 0], cells_xy[:, 1]
    peak[crops, 0, y, x] = 1
    reg[crops, :, y, x] = torch.stack([centers[crops, slots, 0] - x, centers[crops, slots, 1] - y,
                                       sides[crops, slots, 0].log(), sides[crops, slots, 1].log()], 1)
    heatmap = torch.maximum(heatmap, peak)
    return heatmap, peak, reg


def kept_cells(ignore, cells):
    """(B, 1, G, G): 0 on the grid cells an ignore box covers, with a cell of slack round it, else 1. None when the
    batch has no ignore box."""
    on = ignore[..., 2] > 0
    if not on.any():
        return None
    keep = torch.ones(ignore.shape[0], 1, cells, cells, device=ignore.device)
    for crop, j in on.nonzero().tolist():
        cx, cy, width, height = ignore[crop, j].tolist()
        x0 = max(0, math.floor((cx - width / 2) / net.STRIDE) - 1)
        x1 = min(cells - 1, math.floor((cx + width / 2) / net.STRIDE) + 1)
        y0 = max(0, math.floor((cy - height / 2) / net.STRIDE) - 1)
        y1 = min(cells - 1, math.floor((cy + height / 2) / net.STRIDE) + 1)
        keep[crop, 0, y0:y1 + 1, x0:x1 + 1] = 0
    return keep


def loss_fn(out, heatmap, peak, reg, config, keep=None):
    """keep (kept_cells): the cells outside it add nothing, to the heatmap's loss or the regression."""
    score = torch.sigmoid(out[:, 0:1].float()).clamp(SCORE_CLAMP, 1 - SCORE_CLAMP)
    positive = peak
    negative = 1 - positive
    if keep is not None:
        positive, negative = positive * keep, negative * keep
    loss_positive = -((1 - score) ** FOCAL_POWER_HIT * torch.log(score) * positive).sum()
    loss_negative = -((1 - heatmap) ** FOCAL_POWER_NEAR * score ** FOCAL_POWER_HIT * torch.log(1 - score) * negative).sum()
    positives = positive.sum().clamp(min=1)
    focal = (loss_positive + loss_negative) / positives
    at_peaks = positive.expand(-1, 4, -1, -1)
    offset = (F.l1_loss(out[:, 1:3].float(), reg[:, 0:2], reduction="none") * at_peaks[:, :2]).sum() / positives
    size = (F.l1_loss(out[:, 3:5].float(), reg[:, 2:4], reduction="none") * at_peaks[:, 2:]).sum() / positives
    weights = config["loss"]
    return focal + weights["offset"] * offset + weights["size"] * size, dict(focal=focal.detach(), offset=offset.detach(),
                                                                            size=size.detach())


# ---- metrics --------------------------------------------------------------------------------------------------------
def match(predictions, truth, counts):
    """Greedy matching by score: a prediction is a hit when its center is within max(2 px, half the target's size)."""
    true_pos = false_pos = false_neg = 0
    errors = []
    for crop in range(len(predictions)):
        labels = truth[crop, :int(counts[crop])]
        used = torch.zeros(len(labels), dtype=torch.bool)
        for found in predictions[crop][predictions[crop][:, 4].argsort(descending=True)].cpu():
            if len(labels) == 0:
                false_pos += 1
                continue
            distance = ((labels[:, :2] - found[:2]) ** 2).sum(1).sqrt()
            tolerance = torch.clamp(0.5 * labels[:, 2:].min(1).values, min=MATCH_MIN_PX)
            free = (distance <= tolerance) & ~used
            if free.any():
                j = int(torch.where(free, distance, torch.full_like(distance, FAR)).argmin())
                used[j] = True
                true_pos += 1
                errors.append(float(distance[j]))
            else:
                false_pos += 1
        false_neg += int((~used).sum())
    return true_pos, false_pos, false_neg, errors


def summarize(true_pos, false_pos, false_neg, errors):
    precision = true_pos / max(1, true_pos + false_pos)
    recall = true_pos / max(1, true_pos + false_neg)
    return dict(precision=round(precision, 4), recall=round(recall, 4),
                f1=round(2 * precision * recall / max(TINY, precision + recall), 4),
                loc_err_median_px=round(float(np.median(errors)), 3) if errors else None,
                loc_err_p90_px=round(float(np.percentile(errors, 90)), 3) if errors else None, tp=true_pos,
                fp=false_pos, fn=false_neg)


@torch.no_grad()
def evaluate(model, loader, device, recolour_test=False, threshold=0.3):
    """Detection metrics on a split, against its automatic labels. recolour_test: recolor every crop first with a
    fixed seed (a synthetic check that color does not matter)."""
    model.eval()
    seeds = torch.Generator(device="cpu").manual_seed(0)
    true_pos = false_pos = false_neg = 0
    errors = []
    for rgb, fixed, target_mask, boxes, counts, _ in loader:
        rgb, fixed, target_mask, boxes = rgb.to(device), fixed.to(device), target_mask.to(device), boxes.to(device)
        if recolour_test:
            torch.manual_seed(int(torch.randint(0, RECOLOR_SEEDS, (1,), generator=seeds)))
            image = recolour(rgb.permute(0, 3, 1, 2).float() / 255.0, target_mask[:, None].float(), 1.0, 1.0)
            inputs = torch.cat([image, fixed[:, None].float()], 1)
        else:
            inputs = net.prepare(rgb, fixed)
        with torch.autocast("cuda", dtype=torch.bfloat16, enabled=device.type == "cuda"):
            out = model(inputs)
        hits, misses, missed, found_errors = match(net.decode(out, threshold), boxes.cpu(), counts)
        true_pos, false_pos, false_neg, errors = true_pos + hits, false_pos + misses, false_neg + missed, \
            errors + found_errors
    model.train()
    return summarize(true_pos, false_pos, false_neg, errors)


class EpochOrder(torch.utils.data.Sampler):
    """The crops in a fixed order per epoch (seeded by the run's seed and the epoch), starting at any batch: a resumed
    run goes on exactly where it stopped."""

    def __init__(self, n, seed, batch):
        self.n, self.seed, self.batch, self.epoch, self.start = n, seed, batch, 0, 0

    def set_epoch(self, epoch, start_batch=0):
        self.epoch, self.start = epoch, start_batch

    def __iter__(self):
        order = torch.Generator().manual_seed(self.seed * SEED_STRIDE + self.epoch)
        return iter(torch.randperm(self.n, generator=order)[self.start * self.batch:].tolist())

    def __len__(self):
        return self.n - self.start * self.batch


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("config", nargs="?")
    parser.add_argument("--epochs", type=int)
    parser.add_argument("--out", default="test_out/vod_model/runs")
    parser.add_argument("--data", default="test_out/vod_model/data")
    parser.add_argument("--init", help="start from this checkpoint's weights only")
    parser.add_argument("--repeat", help="a file of VOD hashes (one a line) whose crops count --times times; a line can "
                        "give its own count after the hash (\"hand_crop_ 20\")")
    parser.add_argument("--times", type=int, default=1)
    parser.add_argument("--resume", help="a run folder to go on with")
    parser.add_argument("--fork", help="a snapshot (or a run folder: its latest state) to branch from")
    parser.add_argument("--save-every", type=int, default=200)
    parser.add_argument("--extra", action="append", default=[], help="another dataset folder (its train and val splits "
                        "are added)")
    return parser, parser.parse_args()


def run_settings(parser, cli):
    """The config, the run's settings and the state to resume from (None for a new run), and the snapshot to fork
    from (None without --fork)."""
    state = None
    if cli.resume:
        state = torch.load(Path(cli.resume) / "state.pt", map_location="cpu", weights_only=False)
        config, args = state["config"], state["args"]
    else:
        if not cli.config:
            parser.error("a config, or --resume")
        config = json.load(open(cli.config))
        if cli.epochs:
            config["train"]["epochs"] = cli.epochs
        args = dict(data=cli.data, repeat=cli.repeat, times=cli.times, save_every=cli.save_every, init=cli.init,
                    extra=cli.extra)
    fork = None
    if cli.fork:
        path = Path(cli.fork)
        fork = torch.load(path / "state.pt" if path.is_dir() else path, map_location="cpu", weights_only=False)
        config["forked_from"] = dict(run=fork["config"]["name"], snapshot=str(path), epoch=fork["epoch"],
                                     batch=fork["batch"])
    return config, args, state, fork


def repeats(args):
    """--repeat's file as {prefix: times}: a line's own count, else --times."""
    if not args.get("repeat"):
        return {}
    return {parts[0]: int(parts[1]) if len(parts) > 1 else args.get("times", 1)
            for parts in (line.split() for line in Path(args["repeat"]).read_text().splitlines()) if parts}


def schedule(step, steps):
    """The learning rate's factor at a step: a linear warm-up over 300 steps, then a cosine down to 0."""
    return min(1.0, (step + 1) / WARM_UP_STEPS) * 0.5 * (1 + math.cos(math.pi * step / steps))


class Trainer:
    """Training that can be paused, resumed and forked at any point (see main)."""

    def __init__(self, config, args, run, device):
        self.config, self.args, self.run, self.device = config, args, run, device
        train = config["train"]
        sets = [Path(args["data"])] + [Path(extra) for extra in args.get("extra", [])]
        self.train_crops = Crops([folder / "train" for folder in sets], repeats(args), args.get("times", 1))
        self.order = EpochOrder(len(self.train_crops), config["seed"], train["batch"])
        self.train_loader = DataLoader(self.train_crops, train["batch"], sampler=self.order,
                                       num_workers=train["workers"], drop_last=True, persistent_workers=True,
                                       pin_memory=True)
        self.val_loader = DataLoader(Crops([folder / "val" for folder in sets]), VAL_BATCH, num_workers=VAL_WORKERS,
                                     persistent_workers=True)
        per_epoch = len(self.train_crops) // train["batch"]
        self.model = net.build(config).to(device)
        self.steps = train["epochs"] * per_epoch
        self.epoch0, self.batch0, self.best, self.total = 0, 0, -1, 0.0

    def start(self, state, fork):
        """The model, optimizer and schedule: from --init's weights, the fork's weights and optimizer memory (on this
        config's learning rate and schedule), or the resumed run's whole state."""
        train = self.config["train"]
        if self.args.get("init") and not state and not fork:
            self.model.load_state_dict(torch.load(self.args["init"], map_location=self.device,
                                                  weights_only=False)["model"])
        self.params = sum(parameter.numel() for parameter in self.model.parameters())
        self.optimizer = torch.optim.AdamW(self.model.parameters(), lr=train["lr"], weight_decay=train["weight_decay"])
        self.scheduler = torch.optim.lr_scheduler.LambdaLR(self.optimizer, lambda step: schedule(step, self.steps))
        if fork:
            self.model.load_state_dict(fork["model"])
            self.optimizer.load_state_dict(fork["opt"])
            for group in self.optimizer.param_groups:       # the new config's learning rate and schedule
                group["lr"] = group["initial_lr"] = train["lr"]
        if state:
            self.model.load_state_dict(state["model"])
            self.optimizer.load_state_dict(state["opt"])
            self.scheduler.load_state_dict(state["sched"])
            self.epoch0, self.batch0, self.best, self.total = state["epoch"], state["batch"], state["best"], state["tot"]
            random.setstate(state["rng"]["python"])
            np.random.set_state(state["rng"]["numpy"])
            torch.set_rng_state(state["rng"]["torch"])
            if torch.cuda.is_available() and state["rng"]["cuda"]:
                torch.cuda.set_rng_state_all(state["rng"]["cuda"])

    def save(self, epoch, batch):
        """The whole state, at a batch boundary (batch = batches done in this epoch)."""
        state = dict(model=self.model.state_dict(), opt=self.optimizer.state_dict(), sched=self.scheduler.state_dict(),
                     config=self.config, args=self.args, epoch=epoch, batch=batch, best=self.best, tot=float(self.total),
                     params=self.params,
                     rng=dict(python=random.getstate(), numpy=np.random.get_state(), torch=torch.get_rng_state(),
                              cuda=torch.cuda.get_rng_state_all() if torch.cuda.is_available() else None))
        partial = self.run / "state.tmp"
        torch.save(state, partial)
        shutil.copyfile(partial, self.run / "snapshots" / f"e{epoch + 1:02d}_b{batch:05d}.pt")
        partial.replace(self.run / "state.pt")

    def step(self, batch):
        """One optimizer step on a batch; returns its loss."""
        rgb, fixed, target_mask, boxes, ignore = (batch[part].to(self.device, non_blocking=True) for part in (0, 1, 2, 3, 5))
        counts = batch[4]     # kept on the CPU: the loops over the crops read it without waiting for the GPU
        inputs, boxes, ignore = augment(rgb, fixed, target_mask, boxes.clone(), counts, self.config, ignore)
        heatmap, peak, reg = targets(boxes, counts, inputs.shape[-1])
        with torch.autocast("cuda", dtype=torch.bfloat16, enabled=self.device.type == "cuda"):
            out = self.model(inputs)
        loss, _ = loss_fn(out, heatmap, peak, reg, self.config, kept_cells(ignore, heatmap.shape[-1]))
        self.optimizer.zero_grad(set_to_none=True)
        loss.backward()
        torch.nn.utils.clip_grad_norm_(self.model.parameters(), CLIP_NORM)
        self.optimizer.step()
        self.scheduler.step()
        # left on the GPU, in float64 so the epoch's sum equals Python's: reading it each step would wait for the GPU
        return loss.detach().double()

    def train_epoch(self, epoch):
        """One epoch's batches, from where a resumed run stopped; the batches done, or None when paused or
        stopped."""
        start = self.batch0 if epoch == self.epoch0 else 0
        if start == 0:
            self.total = 0.0
        self.order.set_epoch(epoch, start)
        done = start
        try:
            for batch in self.train_loader:
                self.total += self.step(batch)
                done += 1
                if done % self.args.get("save_every", 200) == 0:
                    self.save(epoch, done)
                if (self.run / "PAUSE").exists():
                    self.save(epoch, done)
                    (self.run / "PAUSE").unlink()
                    print(f"paused at epoch {epoch + 1}, batch {done}; go on with: --resume {self.run}", flush=True)
                    return None
        except KeyboardInterrupt:
            self.save(epoch, done)
            print(f"stopped at epoch {epoch + 1}, batch {done}; go on with: --resume {self.run}", flush=True)
            return None
        return done

    def end_epoch(self, epoch, done, started, log):
        """The epoch's validation numbers logged and printed, its checkpoints, and the state saved."""
        val = evaluate(self.model, self.val_loader, self.device)
        row = dict(epoch=epoch + 1, loss=round(float(self.total) / max(1, done), 4), seconds=round(time.time() - started, 1),
                   **val)
        log.write(json.dumps(row) + "\n")
        log.flush()
        print(row, flush=True)
        state = dict(model=self.model.state_dict(), config=self.config, epoch=epoch + 1, val=val, params=self.params)
        torch.save(state, self.run / "last.pt")
        if val["f1"] > self.best:
            self.best = val["f1"]
            torch.save(state, self.run / "best.pt")
        self.save(epoch + 1, 0)


def main():
    """Training that can be paused, resumed and forked at any point. The full state (weights, optimizer, schedule, the
    position in the epoch, the random states) is saved every --save-every steps and at each epoch's end, to
    runs/<name>/state.pt (the latest) and runs/<name>/snapshots/eEE_bBBBBB.pt (all of them).
      pause:  create runs/<name>/PAUSE (or press Ctrl+C): the state is saved at the current step and the run stops;
      resume: train.py --resume runs/<name>;
      fork:   train.py <new config> --fork runs/<name>/snapshots/e03_b00200.pt: a new run (the config's name) starting
              from that snapshot's weights and optimizer memory, on the new config's schedule."""
    parser, cli = parse_args()
    config, args, state, fork = run_settings(parser, cli)
    random.seed(config["seed"])
    np.random.seed(config["seed"])
    torch.manual_seed(config["seed"])
    device = torch.device("cuda" if torch.cuda.is_available() else "cpu")
    run = Path(cli.out) / config["name"]
    if not cli.resume and (run / "state.pt").exists():
        raise SystemExit(f"{run} already has a run: --resume it, or give the config another name")
    (run / "snapshots").mkdir(parents=True, exist_ok=True)
    json.dump(config, open(run / "config.json", "w"), indent=1)
    train = config["train"]
    trainer = Trainer(config, args, run, device)
    trainer.start(state, fork)
    print(f"{config['name']}: {trainer.params} parameters; {len(trainer.train_crops)} train crops, "
          f"{len(trainer.val_loader.dataset)} val crops; {train['epochs']} epochs on {device}"
          + (f"; resuming at epoch {trainer.epoch0 + 1}, batch {trainer.batch0}" if state else "")
          + (f"; forked from {config['forked_from']}" if fork else ""), flush=True)
    log = open(run / "metrics.jsonl", "a")
    for epoch in range(trainer.epoch0, train["epochs"]):
        started = time.time()
        done = trainer.train_epoch(epoch)
        if done is None:
            return
        trainer.end_epoch(epoch, done, started, log)
    print(f"best val F1 {trainer.best:.4f}; checkpoints in {run}")


if __name__ == "__main__":
    main()
