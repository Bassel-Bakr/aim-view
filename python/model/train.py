"""Train the target detector (REPRODUCE.md step 2) from a JSON config in python/model/configs/.

Data: the crops from build_data.py (real frames, automatic labels). Augmentation on the GPU turns them into many looks
the library hardly has (synthetic, recorded as such in MODEL_STATUS.md): any wall color, any target color, wall
texture, blur and noise, outlines round the targets, and synthetic crosshairs (marked in the fixed map), sometimes drawn
over a target.
Writes runs/<name>/: config.json, metrics.jsonl (one line an epoch), best.pt (by validation F1), last.pt, and the
whole state to resume or fork from (state.pt, snapshots/; main says how).
Usage: python python/model/train.py python/model/configs/small.json [--epochs N] [--out test_out/vod_model/runs]
       [--data <dataset>] [--extra <dataset> ...] [--repeat <file> --times N] [--init <checkpoint>]
       [--gpu-share 0.5] [--save-every 200]
       python python/model/train.py --resume <run folder>
       python python/model/train.py <config> --fork <snapshot or run folder>
"""
import argparse
import json
import math
import os
import random
import shutil
import sys
import time
from pathlib import Path

import numpy as np
import torch
import torch.nn.functional as F
from torch.utils.data import Dataset

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import local_config  # noqa: E402
import net  # noqa: E402
from crop_pack import PackedCrops, PackLoader, read_crop  # noqa: E402
HALF = 0.5                      # a coin toss; and a mask's 0/1 split

# recolour (the recolor step): the wall moved to a random color, the targets painted one that stands out
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
TEXTURE_LOW, TEXTURE_HIGH = 0.06, 0.03   # the coarse and the fine texture's largest strength
OUTLINE_MIN_PX, OUTLINE_MAX_PX = 1, 3    # a bot outline's width
GLOW_SHARE = 0.3                # a glow: a little wider, with soft edges
LOUD_SHARE = 0.6                # outlines are often loud: one or two channels full
LOUD_HIGH, LOUD_LOW = 0.9, 0.1  # a loud color: a channel over half becomes 0.9 plus a tenth of it, the others a tenth
OUTLINE_ALPHA_MIN, OUTLINE_ALPHA_RANGE = 0.7, 0.3   # an outline's opacity: 0.7 to 1
REAL_VISIBLE = 0.3              # a real crosshair's pixel shows in the fixed map from this alpha up
CROSSHAIR_EDGE_PX = 8           # a crosshair off the targets keeps this far from the crop's edge
REAL_MIN_PX, REAL_MAX_PX = 5, 40    # a KovaaK's crosshair image's larger side once scaled
TINT_FLOOR = 1e-3               # the smallest channel a tint is divided by
DOT_RADIUS_PX = (0.8, 3.0)      # a drawn crosshair's sizes, each from the first to the second
PLUS_ARM_PX, PLUS_THICKNESS_PX, PLUS_GAP_PX = (3, 9), (0.6, 1.6), (0, 3)
RING_RADIUS_PX, RING_THICKNESS_PX = (3, 8), 0.8
OUTLINE_SIZES = (3, 5)          # a drawn crosshair's outline: the max-pool window that grows it
DARK_OUTLINE_SHARE, DARK_OUTLINE = 0.7, 0.2   # most outlines are dark: their color scaled to a fifth
DECODER_GAIN, DECODER_OFFSET = 0.04, 0.02     # the width of a channel's random gain (round 1) and offset (round 0)
BLUR_SIGMA_PX = (0.3, 1.0)
NOISE = 0.02                    # the noise's largest strength

# targets and loss
SIGMA_SIZE, MIN_SIGMA = 0.15, 0.6   # a center's Gaussian: 0.15 of its larger side (cells), 0.6 cells at least
SCORE_CLAMP = 1e-4              # scores kept this far from 0 and 1, so the log stays finite
FOCAL_POWER_HIT, FOCAL_POWER_NEAR = 2, 4   # the focal loss's powers (CenterNet's alpha and beta)
WARM_UP_STEPS = 300             # the learning rate's linear warm-up
CLIP_NORM = 5.0                 # the gradient's norm is clipped to this
MATCH_MIN_PX = 2.0              # a prediction is a hit within max(2 px, half the target's smaller side)
FAR = 1e9                       # a distance no match takes
TINY = 1e-9                     # the smallest denominator
RECOLOR_SEEDS = 10 ** 6         # each batch's recoloring seed is drawn below this
SEED_STRIDE = 100003            # an epoch's order is seeded by seed * this + epoch
VAL_BATCH = 64


class Crops(Dataset):
    """The crop files of one or more folders, each read as crop_pack.read_crop reads it (eval.py; training reads the
    packs, crop_pack.PackedCrops, in the same order)."""

    def __init__(self, folders, repeat=(), times=1):
        """folders: one folder of crops or several; repeat: crop name prefixes (a VOD's 10-character hash, or
        "hand_crop_" for hand-checked crops) whose crops appear `times` times, or a dict of prefix: times."""
        folders = [folders] if isinstance(folders, (str, Path)) else folders
        self.files = sorted(file for folder in folders for file in Path(folder).glob("*.npz"))
        repeats = repeat if isinstance(repeat, dict) else {prefix: times for prefix in repeat}
        self.files += [file for file in self.files for _ in range(repeats.get(file.name[:10], 1) - 1)]

    def __len__(self):
        """The crop count, repeats included."""
        return len(self.files)

    def __getitem__(self, i):
        """Crop i's arrays (crop_pack.read_crop)."""
        return read_crop(self.files[i])


# ---- augmentation (batched, on the GPU) -----------------------------------------------------------------------------
def lum(image):
    """The luminance (B, 1, H, W) of RGB images (B, 3, H, W) with values 0 to 1 (BT.601 weights)."""
    return 0.299 * image[:, 0:1] + 0.587 * image[:, 1:2] + 0.114 * image[:, 2:3]


def flip_rot(image, fixed, target_mask, boxes, counts):
    """Random flips and quarter turns per batch (the same for the whole batch; boxes follow)."""
    size = image.shape[-1]
    turns = random.randint(0, 3)
    # positions are pixel indices (0 to size - 1), so a flip maps x to size - 1 - x
    if random.random() < HALF:
        image, fixed, target_mask = image.flip(-1), fixed.flip(-1), target_mask.flip(-1)
        boxes[..., 0] = size - 1 - boxes[..., 0]
    # rotate 90 deg counter-clockwise: (x, y) goes to (y, S - 1 - x)
    for _ in range(turns):
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
    """A share of the crops get a random texture on the wall (off the targets): coarse noise per TEXTURE_CELLS and
    fine noise per pixel, each of a random strength."""
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
    width = random.randint(OUTLINE_MIN_PX, OUTLINE_MAX_PX)
    soft = random.random() < GLOW_SHARE                         # a glow: a little wider, with soft edges
    return fast(outline_ring)(image, target_mask, fixed, share, width, soft)


def outline_ring(image, target_mask, fixed, share, width, soft):
    """outlines()' drawing for one width and style (compiled once for each)."""
    batch = image.shape[0]
    device = image.device
    pick = (torch.rand(batch, 1, 1, 1, device=device) < share).float()
    targets = ((target_mask > HALF) & (fixed < HALF)).float()
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


KOVAAKS_CROSSHAIRS = local_config.kovaak("crosshairs")   # KovaaK's crosshair images, or None without Steam's folder
_REAL = {}                                              # real_crosshairs' images, by folder and device


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


DOT, PLUS, RING = 0, 1, 2       # a drawn crosshair's kinds
_SCALED = {}                    # scaled_picture's images, by picture list, index and size


def scaled_picture(pictures, index, height, width):
    """One of KovaaK's crosshair images (4 x h x w) at a size, made once (bilinear; the size alone sets the result)."""
    key = (id(pictures), index, height, width)
    if key not in _SCALED:
        _SCALED[key] = F.interpolate(pictures[index][None], size=(height, width), mode="bilinear",
                                     align_corners=False)[0].clamp(0, 1)
    return _SCALED[key]


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


def drawn_plan():
    """A synthetic crosshair's kind and sizes (px): a dot (its radius), a plus (its arm, thickness and gap) or a ring
    (its radius)."""
    kind = random.choice((DOT, DOT, PLUS, RING))
    if kind == DOT:
        return DOT, random.uniform(*DOT_RADIUS_PX), 0.0, 0.0
    if kind == PLUS:
        return PLUS, random.uniform(*PLUS_ARM_PX), random.uniform(*PLUS_THICKNESS_PX), random.uniform(*PLUS_GAP_PX)
    return RING, random.uniform(*RING_RADIUS_PX), 0.0, 0.0


def crosshair_plans(boxes, counts, size, shares, pictures):
    """Each crop's crosshair, chosen on the CPU: (crop, cx, cy, picture) for one of KovaaK's images (its index, height,
    width and whether it is tinted), else (crop, cx, cy, shape, outline) for a drawn one (drawn_plan(), and the
    outline's width and whether it is dark, or None)."""
    share, on_target, jitter, outline, real = shares
    pictured, drawn = [], []
    for crop in range(boxes.shape[0]):
        if random.random() > share:
            continue
        cx, cy = crosshair_center(boxes, counts, crop, size, on_target, jitter)
        if pictures and random.random() < real:
            index = random.randrange(len(pictures))
            height, width = pictures[index].shape[1:]
            scale = random.uniform(REAL_MIN_PX, REAL_MAX_PX) / max(height, width)
            pictured.append((crop, cx, cy, (index, max(1, round(height * scale)), max(1, round(width * scale)),
                                            random.random() < HALF)))
            continue
        shape = drawn_plan()
        edge = (random.choice(OUTLINE_SIZES), random.random() < DARK_OUTLINE_SHARE) \
            if random.random() < outline else None
        drawn.append((crop, cx, cy, shape, edge))
    return pictured, drawn


def paste_pictures(image, fixed, plans, pictures):
    """KovaaK's crosshair images alpha-blended into their crops, centered on their spots, half of them tinted, and
    their visible pixels marked in the fixed map, as the key frames mark the game's crosshair: placed in one overlay,
    then blended for the whole batch at once (alpha 0 leaves a crop as it was)."""
    if not plans:
        return image, fixed
    batch, _, size, _ = image.shape
    device = image.device
    overlay = torch.zeros(batch, 4, size, size, device=device)
    for crop, cx, cy, (index, height, width, _) in plans:
        picture = scaled_picture(pictures, index, height, width)
        x0, y0 = int(round(cx - width / 2)), int(round(cy - height / 2))
        left, top, right, bottom = max(0, x0), max(0, y0), min(size, x0 + width), min(size, y0 + height)
        if left < right and top < bottom:
            overlay[crop, :, top:bottom, left:right] = picture[:, top - y0:bottom - y0, left - x0:right - x0]
    gain = torch.ones(batch, 3, 1, 1, device=device)
    tinted = [crop for crop, _, _, (_, _, _, tint) in plans if tint]
    if tinted:
        tint = torch.rand(len(tinted), 3, 1, 1, device=device)
        gain.index_copy_(0, torch.tensor(tinted).to(device, non_blocking=True),
                         tint / tint.amax(1, keepdim=True).clamp(min=TINT_FLOOR))
    alpha = overlay[:, 3:]
    image = image * (1 - alpha) + overlay[:, :3] * gain * alpha
    return image, torch.maximum(fixed, (alpha > REAL_VISIBLE).to(fixed.dtype))


def drawn_masks(plans, size, device):
    """The pixels of each drawn crosshair (n, S, S) and of its outline (none without one), all at once."""
    rows = torch.tensor([[cx, cy, *shape, (edge or (0, False))[0]] for _, cx, cy, shape, edge in plans],
                        dtype=torch.float32).to(device, non_blocking=True)
    cx, cy, kind, first, second, third, grow = (rows[:, k, None, None] for k in range(7))
    pixels = torch.arange(size, device=device, dtype=torch.float32)
    dx, dy = pixels[None, None, :] - cx, pixels[None, :, None] - cy
    distance2 = dx ** 2 + dy ** 2
    plus = ((dx.abs() <= second) & (dy.abs() <= first) & (dy.abs() >= third)) | \
        ((dy.abs() <= second) & (dx.abs() <= first) & (dx.abs() >= third))
    drawn = torch.where(kind == DOT, distance2 <= first ** 2,
                        torch.where(kind == PLUS, plus, (distance2.sqrt() - first).abs() <= RING_THICKNESS_PX))
    edge = torch.zeros_like(drawn)
    for grow_px in OUTLINE_SIZES:
        edge |= (F.max_pool2d(drawn[:, None].float(), grow_px, 1, grow_px // 2)[:, 0] > 0) & (grow == grow_px)
    return drawn, edge & ~drawn


def draw_crosshairs(image, fixed, plans):
    """The drawn crosshairs (dot, plus or ring, any color) and their outlines (mostly dark) painted into their crops
    and marked in the fixed map, all at once."""
    if not plans:
        return image, fixed
    device = image.device
    crops = torch.tensor([plan[0] for plan in plans]).to(device, non_blocking=True)
    dark = torch.tensor([bool(edge and edge[1]) for *_, edge in plans]).to(device, non_blocking=True)
    drawn, edge = drawn_masks(plans, image.shape[-1], device)
    color = torch.rand(len(plans), 3, 1, 1, device=device)
    edge_color = torch.rand(len(plans), 3, 1, 1, device=device) \
        * torch.where(dark, DARK_OUTLINE, 1.0)[:, None, None, None]
    picked = torch.where(edge[:, None], edge_color, torch.where(drawn[:, None], color, image.index_select(0, crops)))
    marked = fixed.index_select(0, crops).masked_fill((drawn | edge)[:, None], 1)
    return image.index_copy(0, crops, picked), fixed.index_copy(0, crops, marked)


def crosshairs(image, fixed, boxes, counts, share=0.5, on_target=0.5, jitter=0.0, outline=0.0, real=0.0,
               folder=KOVAAKS_CROSSHAIRS):
    """Synthetic crosshairs (dot, plus or ring, any color) drawn into the image and the fixed map. A share of them
    (on_target) sit on a target, which stays a target: the model must see a target through a crosshair, as in hold-fire
    runs. jitter moves that crosshair off the target's center by up to jitter times the target's size, as when a player
    holds slightly off (v2; v1 drew it dead center). A share (outline) get a 1 or 2 px edge, mostly dark, as Aim Lab's
    red cross has: every model up to small_v6 took that crosshair for a target. A share (real) are KovaaK's own
    crosshair images instead (real_crosshairs()), 5 to 40 px across, half of them tinted, as the game's crosshair color
    does: small_v10 took the user's and other players' crosshairs for targets again.
    A crop has one crosshair at most, so each is chosen on the CPU and all are drawn for the whole batch at once: drawn
    a crop at a time they took about half of a training step's time on the CPU, in hundreds of small GPU calls."""
    pictures = real_crosshairs(folder, image.device) if real else []
    pictured, drawn = crosshair_plans(boxes.cpu(), counts, image.shape[-1], (share, on_target, jitter, outline, real),
                                      pictures)
    image, fixed = paste_pictures(image, fixed, pictured, pictures)
    return draw_crosshairs(image, fixed, drawn)


def decoder(image, share=0.0):
    """Another decoder's colors: the chroma re-sampled at half resolution (nearest or smooth), and small per-channel
    gains and offsets, as a different YUV-to-RGB conversion gives (browser, GPU, other recorders). A share of the
    crops, done for the whole batch at once."""
    if share <= 0:
        return image
    return fast(decoder_colors)(image, share, random.choice(("nearest", "bilinear")))


def decoder_colors(image, share, mode):
    """decoder()'s colors for one way of re-sampling the chroma (compiled once for each)."""
    batch = image.shape[0]
    device = image.device
    pick = (torch.rand(batch, 1, 1, 1, device=device) < share).float()
    luma = lum(image)
    chroma = F.interpolate(F.avg_pool2d(image - luma, 2), scale_factor=2, mode=mode)
    gain = 1 + (torch.rand(batch, 3, 1, 1, device=device) - HALF) * DECODER_GAIN
    offset = (torch.rand(batch, 3, 1, 1, device=device) - HALF) * DECODER_OFFSET
    out = ((luma + chroma) * gain + offset).clamp(0, 1)
    return pick * out + (1 - pick) * image


def blur_noise(image, share=0.5):
    """A Gaussian blur on the whole batch a share of the time (one sigma for it), then noise on every crop, of a
    random strength per crop."""
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


_COMPILED = {}      # a step's fused work, compiled, by name (compile_fused)


def fast(augmentation):
    """A step's work compiled when compile_fused() made it so, else as it is."""
    return _COMPILED.get(augmentation.__name__, augmentation)


def compile_fused(device):
    """torch.compile for what a step does to the whole batch in long chains of small operations (the augmentations
    and the target heatmap), where it can run (CUDA and Triton: triton-windows on Windows, with its own C compiler
    when no other is set). Each chain becomes a few fused kernels: recoloring takes 2.5 ms a batch on the GPU
    instead of 8.1. The model gains nothing (cuDNN's convolutions set its pace). Returns whether it could."""
    if device.type != "cuda":
        return False
    try:
        import triton
    except ImportError:
        return False
    compiler = Path(triton.__file__).parent / "runtime" / "tcc" / "tcc.exe"
    if os.name == "nt" and "CC" not in os.environ and compiler.is_file():
        os.environ["CC"] = str(compiler)    # Triton looks for it in the system's packages, not the user's
    for augmentation in (recolour, texture, outline_ring, decoder_colors, gaussian_heatmap):
        _COMPILED[augmentation.__name__] = torch.compile(augmentation, dynamic=False)
    return True


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
    # the flips and turns leave 8 memory layouts, and a compiled augmentation is made again for each one it meets
    image, fixed, target_mask = (part.contiguous() for part in (image, fixed, target_mask))
    shares = config["augment"]
    image = fast(recolour)(image, target_mask, shares["theme"], shares["target"])
    image = fast(texture)(image, target_mask, shares["texture"])
    image = outlines(image, target_mask, fixed, shares.get("outline", 0.0))
    image, fixed = crosshairs(image, fixed, boxes, counts, shares["crosshair"], shares.get("crosshair_on_target", 0.5),
                              shares.get("crosshair_jitter", 0.0), shares.get("crosshair_outline", 0.0),
                              shares.get("crosshair_real", 0.0))
    image = decoder(image, shares.get("decoder", 0.0))
    image = blur_noise(image, shares["blur"])
    return (torch.cat([image, fixed], 1), boxes) + (() if ignore is None else (ignore,))


# ---- targets and loss -----------------------------------------------------------------------------------------------
def gaussian_heatmap(boxes, valid, cells):
    """(B, 1, G, G): a Gaussian at every valid box's center (cells), its spread from the box's size."""
    centers = boxes[..., :2] / net.STRIDE
    sides = boxes[..., 2:].clamp(min=1.0)
    sigma = (SIGMA_SIZE * sides.max(-1).values / net.STRIDE).clamp(min=MIN_SIGMA)
    yy, xx = torch.meshgrid(torch.arange(cells, device=boxes.device), torch.arange(cells, device=boxes.device),
                            indexing="ij")
    distance2 = (xx[None, None] + 0.5 - centers[..., 0, None, None]) ** 2 + \
        (yy[None, None] + 0.5 - centers[..., 1, None, None]) ** 2
    gaussians = torch.exp(-distance2 / (2 * sigma[..., None, None] ** 2)) * valid[..., None, None]
    return gaussians.max(1).values[:, None]


def targets(boxes, counts, size, device=None):
    """Heatmap (B, 1, S/4, S/4) with a Gaussian at every center, and the regression targets at center cells, on
    `device` (the boxes' own by default). With the boxes and counts on the CPU nothing here waits for the GPU."""
    batch, box_slots = boxes.shape[:2]
    cells = size // net.STRIDE
    device = device or boxes.device
    boxes = boxes.cpu()
    valid = torch.arange(box_slots)[None] < counts.cpu()[:, None]
    # every box's cell at once, found on the CPU; where two boxes share a cell the later one's values stand, as
    # writing them box by box left them
    crops, slots = valid.nonzero(as_tuple=True)
    cells_xy = (boxes[..., :2] / net.STRIDE).floor().long().clamp(0, cells - 1)[crops, slots]
    keys = ((crops * cells + cells_xy[:, 1]) * cells + cells_xy[:, 0]).tolist()
    last = sorted({key: i for i, key in enumerate(keys)}.values())
    kept = torch.stack([crops[last], slots[last], cells_xy[last, 0], cells_xy[last, 1]], 1)
    # one pinned upload for all of it (the indices are small enough to be exact as floats)
    sent = torch.cat([boxes.reshape(-1), valid.reshape(-1).float(), kept.reshape(-1).float()]).pin_memory()
    sent = sent.to(device, non_blocking=True)
    box_values = boxes.numel()
    boxes = sent[:box_values].view(batch, box_slots, 4)
    valid = sent[box_values:box_values + valid.numel()].view(batch, box_slots) > 0
    crops, slots, x, y = sent[box_values + valid.numel():].view(-1, 4).long().unbind(1)
    heatmap = fast(gaussian_heatmap)(boxes, valid, cells)
    centers, sides = boxes[..., :2] / net.STRIDE, boxes[..., 2:].clamp(min=1.0)
    peak = torch.zeros(batch, 1, cells, cells, device=device)
    reg = torch.zeros(batch, 4, cells, cells, device=device)
    peak[:, 0][crops, y, x] = torch.ones((), device=device)
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
    """The loss of a batch's output against its targets: the focal loss on the heatmap plus the config's weights times
    the offset's and size's L1 loss at the center cells; and the three parts, detached. keep (kept_cells): the cells
    outside it add nothing, to the heatmap's loss or the regression."""
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
def batch_detections(out, threshold):
    """net.decode(out, threshold) for a batch, the peaks found for all its images at once and read back to the CPU
    in one copy (decoding image by image waited for the GPU 64 times a batch): the same values, as the same
    arithmetic runs on the same device."""
    heat = torch.sigmoid(out[:, 0:1].float())
    peak = (heat == F.max_pool2d(heat, net.PEAK_WINDOW, 1, 1)) & (heat > threshold)
    images, ys, xs = torch.nonzero(peak[:, 0], as_tuple=True)
    cells = out[images, :, ys, xs].float().T
    found = torch.stack([(xs.float() + cells[1]) * net.STRIDE, (ys.float() + cells[2]) * net.STRIDE, cells[3].exp(),
                         cells[4].exp(), heat[images, 0, ys, xs]], 1).cpu()
    images = images.cpu()
    detections = []
    for image in range(out.shape[0]):
        rows = found[images == image]
        if len(rows) > net.MAX_DETECTIONS:
            rows = rows[rows[:, 4].topk(net.MAX_DETECTIONS).indices]
        detections.append(rows)
    return detections


def match(predictions, truth, counts):
    """Greedy matching by score: a prediction is a hit when its center is within max(2 px, half the target's size).
    Equal scores (common: the scores come from bfloat16) keep the detections' order, on any device."""
    true_pos = false_pos = false_neg = 0
    errors = []
    for crop in range(len(predictions)):
        labels = truth[crop, :int(counts[crop])]
        used = torch.zeros(len(labels), dtype=torch.bool)
        for found in predictions[crop][predictions[crop][:, 4].argsort(descending=True, stable=True)].cpu():
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
    """Precision, recall, F1, the hits' center error (median and 90th percentile, px) and the counts, from match's
    tally."""
    precision =true_pos / max(1, true_pos + false_pos)
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
            out = model(inputs.contiguous(memory_format=torch.channels_last))
        hits, misses, missed, found_errors = match(batch_detections(out, threshold), boxes.cpu(), counts)
        true_pos, false_pos, false_neg, errors = true_pos + hits, false_pos + misses, false_neg + missed, \
            errors + found_errors
    model.train()
    return summarize(true_pos, false_pos, false_neg, errors)


class EpochOrder(torch.utils.data.Sampler):
    """The crops in a fixed order per epoch (seeded by the run's seed and the epoch), starting at any batch: a resumed
    run goes on exactly where it stopped."""

    def __init__(self, n, seed, batch):
        """n: the crop count; seed: the run's; batch: the batch size."""
        self.n, self.seed, self.batch, self.epoch, self.start = n, seed, batch, 0, 0

    def set_epoch(self, epoch, start_batch=0):
        """The epoch to draw the order of, and the batch to start at."""
        self.epoch, self.start = epoch, start_batch

    def __iter__(self):
        """The epoch's crop indices from the start batch on."""
        order = torch.Generator().manual_seed(self.seed * SEED_STRIDE + self.epoch)
        return iter(torch.randperm(self.n, generator=order)[self.start * self.batch:].tolist())

    def __len__(self):
        """The crops left in the epoch from the start batch on."""
        return self.n - self.start * self.batch


# The least share of the time --gpu-share lets the GPU train: below it a run would barely move.
MIN_GPU_SHARE = 0.05


def parse_args():
    """The parser (for its errors) and the command line's options."""
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
    parser.add_argument("--gpu-share", type=float, default=1.0,
                        help="the share of the time the GPU trains (0 to 1): after each step the run rests so the GPU "
                        "is free the rest of the time, for a game beside it; the weights come out the same")
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

    gpu_share = 1.0                 # --gpu-share: the share of the time the GPU trains

    def __init__(self, config, args, run, device):
        """Opens the train and val packs of the dataset and its --extra ones, and builds the model; start() then loads
        weights and makes the optimizer. run: the run's folder."""
        self.config, self.args, self.run, self.device = config, args, run, device
        train = config["train"]
        sets = [Path(args["data"])] + [Path(extra) for extra in args.get("extra", [])]
        # the crops' packs (crop_pack.py), read on a thread: worker processes reading the crop files were slower
        self.train_crops = PackedCrops([folder / "train" for folder in sets], repeats(args), args.get("times", 1))
        self.order = EpochOrder(len(self.train_crops), config["seed"], train["batch"])
        self.train_loader = PackLoader(self.train_crops, train["batch"], sampler=self.order, drop_last=True,
                                       pin_memory=True)
        self.val_loader = PackLoader(PackedCrops([folder / "val" for folder in sets]), VAL_BATCH)
        per_epoch = len(self.train_crops) // train["batch"]
        # channels last: cuDNN's depthwise convolutions run 1.7 times as fast on it as on the default layout
        self.model = net.build(config).to(device, memory_format=torch.channels_last)
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
        rgb, fixed, target_mask = (batch[part].to(self.device, non_blocking=True) for part in (0, 1, 2))
        # the boxes, their counts and the ignore boxes stay on the CPU, where the crosshairs, the targets and the
        # ignored cells read them: nothing in the step waits for the GPU, so the next batch is prepared while it works
        boxes, counts, ignore = batch[3], batch[4], batch[5]
        inputs, boxes, ignore = augment(rgb, fixed, target_mask, boxes.clone(), counts, self.config, ignore)
        heatmap, peak, reg = targets(boxes, counts, inputs.shape[-1], self.device)
        keep = kept_cells(ignore, heatmap.shape[-1])
        with torch.autocast("cuda", dtype=torch.bfloat16, enabled=self.device.type == "cuda"):
            out = self.model(inputs.contiguous(memory_format=torch.channels_last))
        loss, _ = loss_fn(out, heatmap, peak, reg, self.config,
                          None if keep is None else keep.to(self.device, non_blocking=True))
        self.optimizer.zero_grad(set_to_none=True)
        loss.backward()
        torch.nn.utils.clip_grad_norm_(self.model.parameters(), CLIP_NORM)
        self.optimizer.step()
        self.scheduler.step()
        # left on the GPU, in float64 so the epoch's sum equals Python's: reading it each step would wait for the GPU
        return loss.detach().double()

    def rest(self, step_started):
        """After a step, rests so the GPU trains only --gpu-share of the time: the step's time (waited for on the GPU)
        times the rest's share over the work's."""
        if self.gpu_share >= 1:
            return
        if self.device.type == "cuda":
            torch.cuda.synchronize()
        busy = time.perf_counter() - step_started
        time.sleep(busy * (1 / max(self.gpu_share, MIN_GPU_SHARE) - 1))

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
                step_started = time.perf_counter()
                self.total += self.step(batch)
                self.rest(step_started)
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
    torch.backends.cudnn.benchmark = True       # the fastest convolution for the crops' one size, found once
    compiled = compile_fused(device)
    run = Path(cli.out) / config["name"]
    if not cli.resume and (run / "state.pt").exists():
        raise SystemExit(f"{run} already has a run: --resume it, or give the config another name")
    (run / "snapshots").mkdir(parents=True, exist_ok=True)
    json.dump(config, open(run / "config.json", "w"), indent=1)
    train = config["train"]
    trainer = Trainer(config, args, run, device)
    trainer.gpu_share = cli.gpu_share
    trainer.start(state, fork)
    print(f"{config['name']}: {trainer.params} parameters; {len(trainer.train_crops)} train crops, "
          f"{len(trainer.val_loader.dataset)} val crops; {train['epochs']} epochs on {device}"
          + (f"; resuming at epoch {trainer.epoch0 + 1}, batch {trainer.batch0}" if state else "")
          + (f"; forked from {config['forked_from']}" if fork else "")
          + ("; compiled" if compiled else ""), flush=True)
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
