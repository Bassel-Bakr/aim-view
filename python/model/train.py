"""Train the target detector (REPRODUCE.md step 2) from a JSON config in python/model/configs/.

Data: the crops from build_data.py (real frames, automatic labels). Augmentation on the GPU turns them into many looks
the library hardly has (synthetic, recorded as such in MODEL_STATUS.md): any wall colour, any target colour, wall
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


class Crops(Dataset):
    def __init__(self, folders, repeat=(), times=1):
        """folders: one folder of crops or several; repeat: crop name prefixes (a VOD's 10-character hash, or
        "hand_crop_" for hand-checked crops) whose crops appear `times` times, or a dict of prefix: times."""
        folders = [folders] if isinstance(folders, (str, Path)) else folders
        self.files = sorted(f for d in folders for f in Path(d).glob("*.npz"))
        rep = repeat if isinstance(repeat, dict) else {r: times for r in repeat}
        self.files += [f for f in self.files for _ in range(rep.get(f.name[:10], 1) - 1)]

    def __len__(self):
        return len(self.files)

    def __getitem__(self, i):
        z = np.load(self.files[i])
        keep = []                       # the labeller can report one target twice (overlapping search windows)
        for box in z["boxes"]:
            if all(np.hypot(box[0] - k[0], box[1] - k[1]) > 1.5 for k in keep):
                keep.append(box)
        b = np.zeros((MAXBOX, 4), np.float32)
        n = min(MAXBOX, len(keep))
        if n:
            b[:n] = np.array(keep[:n])
        return z["rgb"], z["fixed"], z["tmask"], b, n


# ---- augmentation (batched, on the GPU) -----------------------------------------------------------------------------
def lum(x):
    return 0.299 * x[:, 0:1] + 0.587 * x[:, 1:2] + 0.114 * x[:, 2:3]


def flip_rot(img, fixed, tmask, boxes, n):
    """Random flips and quarter turns per batch (the same for the whole batch; boxes follow)."""
    S = img.shape[-1]
    k = random.randint(0, 3)
    # positions are pixel indices (0 to S - 1), so a flip maps x to S - 1 - x
    if random.random() < 0.5:
        img, fixed, tmask = img.flip(-1), fixed.flip(-1), tmask.flip(-1)
        boxes[..., 0] = S - 1 - boxes[..., 0]
    for _ in range(k):                                       # rotate 90 deg counter-clockwise: (x, y) -> (y, S - 1 - x)
        img, fixed, tmask = img.rot90(1, (-2, -1)), fixed.rot90(1, (-2, -1)), tmask.rot90(1, (-2, -1))
        x, y, w, h = boxes.unbind(-1)
        boxes = torch.stack([y, S - 1 - x, h, w], -1)
    return img, fixed, tmask, boxes


def recolour(img, tmask, p_theme=0.7, p_target=0.8):
    """A new theme: the wall's colour moved to a random colour (texture and edges kept, relative to it), and the
    targets painted a random colour that still stands out, with their anti-aliased edges blended again."""
    B = img.shape[0]
    dev = img.device
    t = tmask.float()
    wallmask = (F.max_pool2d(t, 5, 1, 2) == 0).float()
    wall = (img * wallmask).sum((2, 3), keepdim=True) / wallmask.sum((2, 3), keepdim=True).clamp(min=1)
    # local background: the image with the targets cut out, averaged nearby
    num = F.avg_pool2d(img * wallmask, 9, 1, 4)
    den = F.avg_pool2d(wallmask, 9, 1, 4)
    bg = torch.where(den > 0.05, num / den.clamp(min=1e-3), wall.expand_as(img))
    tcol = (img * t).sum((2, 3), keepdim=True) / t.sum((2, 3), keepdim=True).clamp(min=1)
    # how much of each pixel near a target is target (anti-aliasing), from its luminance between wall and target
    near = F.max_pool2d(t, 3, 1, 1)
    a = ((lum(bg) - lum(img)) / (lum(bg) - lum(tcol)).abs().clamp(min=0.05) * torch.sign(lum(bg) - lum(tcol)))
    alpha = (a.clamp(0, 1) * near).clamp(0, 1)
    alpha = torch.maximum(alpha, t)
    theme = (torch.rand(B, 1, 1, 1, device=dev) < p_theme).float()
    new_wall = torch.rand(B, 3, 1, 1, device=dev) * (0.15 + 0.85 * torch.rand(B, 1, 1, 1, device=dev))
    gain = 0.6 + 0.8 * torch.rand(B, 1, 1, 1, device=dev)
    img_w = theme * (new_wall + (img - wall) * gain) + (1 - theme) * img
    bg_w = theme * (new_wall + (bg - wall) * gain) + (1 - theme) * bg
    # target colour: random, at least 0.35 (sum of |RGB| differences) away from the wall
    cand = torch.rand(B, 6, 3, device=dev)
    wcol = (theme * new_wall + (1 - theme) * wall).view(B, 1, 3)
    far = (cand - wcol).abs().sum(-1) > 0.35
    idx = torch.where(far.any(1), far.float().argmax(1), torch.zeros(B, dtype=torch.long, device=dev))
    newt = cand[torch.arange(B, device=dev), idx].view(B, 3, 1, 1)
    paint = (torch.rand(B, 1, 1, 1, device=dev) < p_target).float()
    shade = 1 + 0.15 * (lum(img) - lum(tcol))                # keep a little of the original shading
    tgt = paint * (newt * shade).clamp(0, 1) + (1 - paint) * img
    out = alpha * tgt + (1 - alpha) * img_w
    out = torch.where(alpha > 0, alpha * tgt + (1 - alpha) * bg_w, img_w)
    return out.clamp(0, 1)


def texture(img, tmask, p=0.3):
    B, _, S, _ = img.shape
    dev = img.device
    on = (torch.rand(B, 1, 1, 1, device=dev) < p).float()
    lo = F.interpolate(torch.randn(B, 3, S // 16, S // 16, device=dev), size=(S, S), mode="bilinear", align_corners=False)
    hi = torch.randn(B, 1, S, S, device=dev)
    amp_lo = 0.06 * torch.rand(B, 1, 1, 1, device=dev)
    amp_hi = 0.03 * torch.rand(B, 1, 1, 1, device=dev)
    wall = (F.max_pool2d(tmask.float(), 5, 1, 2) == 0).float()
    return (img + on * wall * (amp_lo * lo + amp_hi * hi)).clamp(0, 1)


def outlines(img, tmask, fixed, p=0.0):
    """KovaaK's can draw an outline round each bot: a ring of one colour, 1 to 3 px wide at 720p, sometimes soft like a
    glow. Drawn round the labelled targets of a share p of the crops, following their shape (a round dilation); the
    crosshair (the fixed map) is never outlined and stays on top, as in the game. The labels stay the targets.
    One width and style per batch, a colour per crop: done for the whole batch at once."""
    if p <= 0:
        return img
    B = img.shape[0]
    dev = img.device
    pick = (torch.rand(B, 1, 1, 1, device=dev) < p).float()
    t = ((tmask > 0.5) & (fixed < 0.5)).float()
    r = random.randint(1, 3)
    soft = random.random() < 0.3                                # a glow: a little wider, with soft edges
    R = r + 1 if soft else r
    yy, xx = torch.meshgrid(torch.arange(-R, R + 1, device=dev), torch.arange(-R, R + 1, device=dev), indexing="ij")
    disk = ((xx ** 2 + yy ** 2) <= R * R + 0.5).float()[None, None]
    ring = (F.conv2d(t, disk, padding=R) > 0).float() * (1 - t)
    if soft:
        ring = F.avg_pool2d(ring, 3, 1, 1) * (1 - t)
    col = torch.rand(B, 3, 1, 1, device=dev)
    loud = (torch.rand(B, 1, 1, 1, device=dev) < 0.6).float()  # outlines are often loud: one or two channels full
    col = loud * ((col > 0.5).float() * 0.9 + 0.1 * col) + (1 - loud) * col
    alpha = (0.7 + 0.3 * torch.rand(B, 1, 1, 1, device=dev)) * ring * (1 - fixed) * pick
    return img * (1 - alpha) + col * alpha


KOVAAKS_CROSSHAIRS = r"C:\Program Files (x86)\Steam\steamapps\common\FPSAimTrainer\FPSAimTrainer\crosshairs"
_REAL = {}


def real_crosshairs(folder):
    """The crosshair images a KovaaK's install has (RGBA PNGs: 45 here, from small dots to big circles), as float
    tensors 4 x h x w, loaded once. Empty when the folder is missing."""
    if folder not in _REAL:
        from PIL import Image
        files = sorted(Path(folder).glob("*.png")) if folder and Path(folder).is_dir() else []
        _REAL[folder] = [torch.from_numpy(np.asarray(Image.open(f).convert("RGBA"), np.float32) / 255).permute(2, 0, 1)
                         for f in files]
    return _REAL[folder]


def paste_real(img, fixed, b, cx, cy, x):
    """One real crosshair image x (4 x h x w) alpha-blended into crop b, centred on (cx, cy), and its visible pixels
    marked in the fixed map, as the key frames mark the game's crosshair."""
    S = img.shape[-1]
    h, w = x.shape[1:]
    x0, y0 = int(round(cx - w / 2)), int(round(cy - h / 2))
    xa, ya, xb, yb = max(0, x0), max(0, y0), min(S, x0 + w), min(S, y0 + h)
    if xa >= xb or ya >= yb:
        return
    part = x[:, ya - y0:yb - y0, xa - x0:xb - x0]
    a = part[3:4]
    img[b, :, ya:yb, xa:xb] = img[b, :, ya:yb, xa:xb] * (1 - a) + part[:3] * a
    fixed[b, 0, ya:yb, xa:xb] = torch.maximum(fixed[b, 0, ya:yb, xa:xb], (a[0] > 0.3).to(fixed.dtype))


def crosshairs(img, fixed, boxes, n, p=0.5, on_target=0.5, jitter=0.0, outline=0.0, real=0.0, folder=KOVAAKS_CROSSHAIRS):
    """Synthetic crosshairs (dot, plus or ring, any colour) drawn into the image and the fixed map. A share of them
    (on_target) sit on a target, which stays a target: the model must see a target through a crosshair, as in hold-fire
    runs. jitter moves that crosshair off the target's centre by up to jitter times the target's size, as when a player
    holds slightly off (v2; v1 drew it dead centre). A share (outline) get a 1 or 2 px edge, mostly dark, as Aim Lab's
    red cross has: every model up to small_v6 took that crosshair for a target. A share (real) are KovaaK's own
    crosshair images instead (real_crosshairs()), 5 to 40 px across, half of them tinted, as the game's crosshair colour
    does: small_v10 took the user's and other players' crosshairs for targets again."""
    B, _, S, _ = img.shape
    dev = img.device
    yy, xx = torch.meshgrid(torch.arange(S, device=dev), torch.arange(S, device=dev), indexing="ij")
    for b in range(B):
        if random.random() > p:
            continue
        if n[b] > 0 and random.random() < on_target:
            j = random.randrange(int(n[b]))
            cx, cy = float(boxes[b, j, 0]), float(boxes[b, j, 1])
            r = jitter * float(boxes[b, j, 2:].max())
            cx, cy = cx + random.uniform(-r, r), cy + random.uniform(-r, r)
        else:
            cx, cy = random.uniform(8, S - 8), random.uniform(8, S - 8)
        pics = real_crosshairs(folder) if real else []
        if pics and random.random() < real:
            x = random.choice(pics).to(dev)
            k = random.uniform(5, 40) / max(x.shape[1:])
            x = F.interpolate(x[None], size=(max(1, round(x.shape[1] * k)), max(1, round(x.shape[2] * k))),
                              mode="bilinear", align_corners=False)[0].clamp(0, 1)
            if random.random() < 0.5:
                col = torch.rand(3, 1, 1, device=dev)
                x = torch.cat([x[:3] * col / col.max().clamp(min=1e-3), x[3:]])
            paste_real(img, fixed, b, cx, cy, x)
            continue
        kind = random.choice(("dot", "dot", "plus", "ring"))
        dx, dy = xx - cx, yy - cy
        if kind == "dot":
            m = dx ** 2 + dy ** 2 <= random.uniform(0.8, 3.0) ** 2
        elif kind == "plus":
            arm, th, gap = random.uniform(3, 9), random.uniform(0.6, 1.6), random.uniform(0, 3)
            m = ((dx.abs() <= th) & (dy.abs() <= arm) & (dy.abs() >= gap)) | \
                ((dy.abs() <= th) & (dx.abs() <= arm) & (dx.abs() >= gap))
        else:
            r = random.uniform(3, 8)
            m = ((dx ** 2 + dy ** 2).sqrt() - r).abs() <= 0.8
        col = torch.rand(3, 1, device=dev)
        img[b][:, m] = col
        fixed[b, 0][m] = 1
        if random.random() < outline:
            k = random.choice((3, 5))
            edge = (F.max_pool2d(m[None, None].float(), k, 1, k // 2)[0, 0] > 0) & ~m
            dark = random.random() < 0.7
            img[b][:, edge] = torch.rand(3, 1, device=dev) * (0.2 if dark else 1.0)
            fixed[b, 0][edge] = 1
    return img, fixed


def decoder(img, p=0.0):
    """Another decoder's colours: the chroma re-sampled at half resolution (nearest or smooth), and small per-channel
    gains and offsets, as a different YUV-to-RGB conversion gives (browser, GPU, other recorders). A share p of the
    crops, done for the whole batch at once."""
    if p <= 0:
        return img
    B = img.shape[0]
    dev = img.device
    pick = (torch.rand(B, 1, 1, 1, device=dev) < p).float()
    y = lum(img)
    c = F.interpolate(F.avg_pool2d(img - y, 2), scale_factor=2, mode=random.choice(("nearest", "bilinear")))
    g = 1 + (torch.rand(B, 3, 1, 1, device=dev) - 0.5) * 0.04
    o = (torch.rand(B, 3, 1, 1, device=dev) - 0.5) * 0.02
    out = ((y + c) * g + o).clamp(0, 1)
    return pick * out + (1 - pick) * img


def blur_noise(img, p=0.5):
    B = img.shape[0]
    dev = img.device
    if random.random() < p:
        s = random.uniform(0.3, 1.0)
        k = torch.exp(-torch.arange(-2, 3, device=dev, dtype=torch.float32) ** 2 / (2 * s * s))
        k = k / k.sum()
        img = F.conv2d(img.reshape(-1, 1, *img.shape[2:]), k.view(1, 1, 1, 5), padding=(0, 2))
        img = F.conv2d(img, k.view(1, 1, 5, 1), padding=(2, 0)).reshape(B, 3, *img.shape[2:])
    img = img + torch.randn_like(img) * 0.02 * torch.rand(B, 1, 1, 1, device=dev)
    return img.clamp(0, 1)


def augment(rgb, fixed, tmask, boxes, n, cfg):
    img = rgb.permute(0, 3, 1, 2).float() / 255.0
    fixed = fixed[:, None].float()
    tmask = tmask[:, None].float()
    img, fixed, tmask, boxes = flip_rot(img, fixed, tmask, boxes, n)
    a = cfg["augment"]
    img = recolour(img, tmask, a["theme"], a["target"])
    img = texture(img, tmask, a["texture"])
    img = outlines(img, tmask, fixed, a.get("outline", 0.0))
    img, fixed = crosshairs(img, fixed, boxes, n, a["crosshair"], a.get("crosshair_on_target", 0.5),
                            a.get("crosshair_jitter", 0.0), a.get("crosshair_outline", 0.0), a.get("crosshair_real", 0.0))
    img = decoder(img, a.get("decoder", 0.0))
    img = blur_noise(img, a["blur"])
    return torch.cat([img, fixed], 1), boxes


# ---- targets and loss -----------------------------------------------------------------------------------------------
def targets(boxes, n, S):
    """Heatmap (B, 1, S/4, S/4) with a Gaussian at every centre, and the regression targets at centre cells."""
    B = boxes.shape[0]
    G = S // net.STRIDE
    dev = boxes.device
    valid = torch.arange(boxes.shape[1], device=dev)[None] < n[:, None]
    c = boxes[..., :2] / net.STRIDE
    size = boxes[..., 2:].clamp(min=1.0)
    sigma = (0.15 * size.max(-1).values / net.STRIDE).clamp(min=0.6)
    yy, xx = torch.meshgrid(torch.arange(G, device=dev), torch.arange(G, device=dev), indexing="ij")
    d2 = (xx[None, None] + 0.5 - c[..., 0, None, None]) ** 2 + (yy[None, None] + 0.5 - c[..., 1, None, None]) ** 2
    g = torch.exp(-d2 / (2 * sigma[..., None, None] ** 2)) * valid[..., None, None]
    hm = g.max(1).values[:, None]
    ci = c.floor().long().clamp(0, G - 1)
    peak = torch.zeros(B, 1, G, G, device=dev)
    reg = torch.zeros(B, 4, G, G, device=dev)
    for b in range(B):
        for j in range(int(n[b])):
            x, y = ci[b, j]
            peak[b, 0, y, x] = 1
            reg[b, :, y, x] = torch.stack([c[b, j, 0] - x, c[b, j, 1] - y, size[b, j, 0].log(), size[b, j, 1].log()])
    hm = torch.maximum(hm, peak)
    return hm, peak, reg


def loss_fn(out, hm, peak, reg, cfg):
    p = torch.sigmoid(out[:, 0:1].float()).clamp(1e-4, 1 - 1e-4)
    pos = peak
    neg = 1 - pos
    lp = -((1 - p) ** 2 * torch.log(p) * pos).sum()
    ln = -((1 - hm) ** 4 * p ** 2 * torch.log(1 - p) * neg).sum()
    npos = pos.sum().clamp(min=1)
    focal = (lp + ln) / npos
    m = pos.expand(-1, 4, -1, -1)
    off = (F.l1_loss(out[:, 1:3].float(), reg[:, 0:2], reduction="none") * m[:, :2]).sum() / npos
    size = (F.l1_loss(out[:, 3:5].float(), reg[:, 2:4], reduction="none") * m[:, 2:]).sum() / npos
    w = cfg["loss"]
    return focal + w["offset"] * off + w["size"] * size, dict(focal=focal.item(), offset=off.item(), size=size.item())


# ---- metrics --------------------------------------------------------------------------------------------------------
def match(pred, gt, n):
    """Greedy matching by score: a prediction is a hit when its centre is within max(2 px, half the target's size)."""
    tp = fp = fn = 0
    err = []
    for b in range(len(pred)):
        g = gt[b, :int(n[b])]
        used = torch.zeros(len(g), dtype=torch.bool)
        for d in pred[b][pred[b][:, 4].argsort(descending=True)].cpu():
            if len(g) == 0:
                fp += 1
                continue
            dist = ((g[:, :2] - d[:2]) ** 2).sum(1).sqrt()
            tol = torch.clamp(0.5 * g[:, 2:].min(1).values, min=2.0)
            ok = (dist <= tol) & ~used
            if ok.any():
                j = int(torch.where(ok, dist, torch.full_like(dist, 1e9)).argmin())
                used[j] = True
                tp += 1
                err.append(float(dist[j]))
            else:
                fp += 1
        fn += int((~used).sum())
    return tp, fp, fn, err


def summarize(tp, fp, fn, err):
    prec = tp / max(1, tp + fp)
    rec = tp / max(1, tp + fn)
    return dict(precision=round(prec, 4), recall=round(rec, 4), f1=round(2 * prec * rec / max(1e-9, prec + rec), 4),
                loc_err_median_px=round(float(np.median(err)), 3) if err else None,
                loc_err_p90_px=round(float(np.percentile(err, 90)), 3) if err else None, tp=tp, fp=fp, fn=fn)


@torch.no_grad()
def evaluate(model, loader, dev, recolour_test=False, thr=0.3):
    """Detection metrics on a split, against its automatic labels. recolour_test: recolour every crop first with a
    fixed seed (a synthetic check that colour does not matter)."""
    model.eval()
    g = torch.Generator(device="cpu").manual_seed(0)
    tp = fp = fn = 0
    err = []
    for rgb, fixed, tmask, boxes, n in loader:
        rgb, fixed, tmask, boxes = rgb.to(dev), fixed.to(dev), tmask.to(dev), boxes.to(dev)
        if recolour_test:
            torch.manual_seed(int(torch.randint(0, 10 ** 6, (1,), generator=g)))
            img = recolour(rgb.permute(0, 3, 1, 2).float() / 255.0, tmask[:, None].float(), 1.0, 1.0)
            x = torch.cat([img, fixed[:, None].float()], 1)
        else:
            x = net.prepare(rgb, fixed)
        with torch.autocast("cuda", dtype=torch.bfloat16, enabled=dev.type == "cuda"):
            out = model(x)
        a, b, c, e = match(net.decode(out, thr), boxes.cpu(), n)
        tp, fp, fn, err = tp + a, fp + b, fn + c, err + e
    model.train()
    return summarize(tp, fp, fn, err)


class EpochOrder(torch.utils.data.Sampler):
    """The crops in a fixed order per epoch (seeded by the run's seed and the epoch), starting at any batch: a resumed
    run goes on exactly where it stopped."""

    def __init__(self, n, seed, batch):
        self.n, self.seed, self.batch, self.epoch, self.start = n, seed, batch, 0, 0

    def set_epoch(self, epoch, start_batch=0):
        self.epoch, self.start = epoch, start_batch

    def __iter__(self):
        g = torch.Generator().manual_seed(self.seed * 100003 + self.epoch)
        return iter(torch.randperm(self.n, generator=g)[self.start * self.batch:].tolist())

    def __len__(self):
        return self.n - self.start * self.batch


def main():
    """Training that can be paused, resumed and forked at any point. The full state (weights, optimizer, schedule, the
    position in the epoch, the random states) is saved every --save-every steps and at each epoch's end, to
    runs/<name>/state.pt (the latest) and runs/<name>/snapshots/eEE_bBBBBB.pt (all of them).
      pause:  create runs/<name>/PAUSE (or press Ctrl+C): the state is saved at the current step and the run stops;
      resume: train.py --resume runs/<name>;
      fork:   train.py <new config> --fork runs/<name>/snapshots/e03_b00200.pt: a new run (the config's name) starting
              from that snapshot's weights and optimizer memory, on the new config's schedule."""
    ap = argparse.ArgumentParser()
    ap.add_argument("config", nargs="?")
    ap.add_argument("--epochs", type=int)
    ap.add_argument("--out", default="test_out/vod_model/runs")
    ap.add_argument("--data", default="test_out/vod_model/data")
    ap.add_argument("--init", help="start from this checkpoint's weights only")
    ap.add_argument("--repeat", help="a file of VOD hashes (one a line) whose crops count --times times; a line can "
                    "give its own count after the hash (\"hand_crop_ 20\")")
    ap.add_argument("--times", type=int, default=1)
    ap.add_argument("--resume", help="a run folder to go on with")
    ap.add_argument("--fork", help="a snapshot (or a run folder: its latest state) to branch from")
    ap.add_argument("--save-every", type=int, default=200)
    ap.add_argument("--extra", action="append", default=[], help="another dataset folder (its train and val splits are added)")
    a = ap.parse_args()
    st = None
    if a.resume:
        st = torch.load(Path(a.resume) / "state.pt", map_location="cpu", weights_only=False)
        cfg, args = st["config"], st["args"]
    else:
        if not a.config:
            ap.error("a config, or --resume")
        cfg = json.load(open(a.config))
        if a.epochs:
            cfg["train"]["epochs"] = a.epochs
        args = dict(data=a.data, repeat=a.repeat, times=a.times, save_every=a.save_every, init=a.init, extra=a.extra)
    fork = None
    if a.fork:
        f = Path(a.fork)
        fork = torch.load(f / "state.pt" if f.is_dir() else f, map_location="cpu", weights_only=False)
        cfg["forked_from"] = dict(run=fork["config"]["name"], snapshot=str(f), epoch=fork["epoch"], batch=fork["batch"])
    random.seed(cfg["seed"])
    np.random.seed(cfg["seed"])
    torch.manual_seed(cfg["seed"])
    dev = torch.device("cuda" if torch.cuda.is_available() else "cpu")
    run = Path(a.out) / cfg["name"]
    if not a.resume and (run / "state.pt").exists():
        raise SystemExit(f"{run} already has a run: --resume it, or give the config another name")
    (run / "snapshots").mkdir(parents=True, exist_ok=True)
    json.dump(cfg, open(run / "config.json", "w"), indent=1)
    t = cfg["train"]
    rep = {p[0]: int(p[1]) if len(p) > 1 else args.get("times", 1)
           for p in (line.split() for line in Path(args["repeat"]).read_text().splitlines()) if p}         if args.get("repeat") else {}
    sets = [Path(args["data"])] + [Path(e) for e in args.get("extra", [])]
    train_ds = Crops([d / "train" for d in sets], rep, args.get("times", 1))
    order = EpochOrder(len(train_ds), cfg["seed"], t["batch"])
    train_dl = DataLoader(train_ds, t["batch"], sampler=order, num_workers=t["workers"], drop_last=True,
                          persistent_workers=True, pin_memory=True)
    val_dl = DataLoader(Crops([d / "val" for d in sets]), 64, num_workers=4, persistent_workers=True)
    per_epoch = len(train_ds) // t["batch"]
    model = net.build(cfg).to(dev)
    if args.get("init") and not st and not fork:
        model.load_state_dict(torch.load(args["init"], map_location=dev, weights_only=False)["model"])
    params = sum(p.numel() for p in model.parameters())
    opt = torch.optim.AdamW(model.parameters(), lr=t["lr"], weight_decay=t["weight_decay"])
    steps = t["epochs"] * per_epoch
    sched = torch.optim.lr_scheduler.LambdaLR(opt, lambda s: min(1.0, (s + 1) / 300) * 0.5 * (1 + math.cos(math.pi * s / steps)))
    epoch0, batch0, best, tot = 0, 0, -1, 0.0
    if fork:
        model.load_state_dict(fork["model"])
        opt.load_state_dict(fork["opt"])
        for g in opt.param_groups:                          # the new config's learning rate and schedule
            g["lr"] = g["initial_lr"] = t["lr"]
    if st:
        model.load_state_dict(st["model"])
        opt.load_state_dict(st["opt"])
        sched.load_state_dict(st["sched"])
        epoch0, batch0, best, tot = st["epoch"], st["batch"], st["best"], st["tot"]
        random.setstate(st["rng"]["python"])
        np.random.set_state(st["rng"]["numpy"])
        torch.set_rng_state(st["rng"]["torch"])
        if torch.cuda.is_available() and st["rng"]["cuda"]:
            torch.cuda.set_rng_state_all(st["rng"]["cuda"])
    print(f"{cfg['name']}: {params} parameters; {len(train_ds)} train crops, {len(val_dl.dataset)} val crops; "
          f"{t['epochs']} epochs on {dev}" + (f"; resuming at epoch {epoch0 + 1}, batch {batch0}" if st else "")
          + (f"; forked from {cfg['forked_from']}" if fork else ""), flush=True)

    def save(epoch, batch):
        """The whole state, at a batch boundary (batch = batches done in this epoch)."""
        state = dict(model=model.state_dict(), opt=opt.state_dict(), sched=sched.state_dict(), config=cfg, args=args,
                     epoch=epoch, batch=batch, best=best, tot=tot, params=params,
                     rng=dict(python=random.getstate(), numpy=np.random.get_state(), torch=torch.get_rng_state(),
                              cuda=torch.cuda.get_rng_state_all() if torch.cuda.is_available() else None))
        tmp = run / "state.tmp"
        torch.save(state, tmp)
        shutil.copyfile(tmp, run / "snapshots" / f"e{epoch + 1:02d}_b{batch:05d}.pt")
        tmp.replace(run / "state.pt")

    log = open(run / "metrics.jsonl", "a")
    for ep in range(epoch0, t["epochs"]):
        t0 = time.time()
        start = batch0 if ep == epoch0 else 0
        if start == 0:
            tot = 0.0
        order.set_epoch(ep, start)
        done = start
        try:
            for rgb, fixed, tmask, boxes, n in train_dl:
                rgb, fixed, tmask, boxes, n = (v.to(dev, non_blocking=True) for v in (rgb, fixed, tmask, boxes, n))
                x, boxes = augment(rgb, fixed, tmask, boxes.clone(), n, cfg)
                hm, peak, reg = targets(boxes, n, x.shape[-1])
                with torch.autocast("cuda", dtype=torch.bfloat16, enabled=dev.type == "cuda"):
                    out = model(x)
                loss, parts = loss_fn(out, hm, peak, reg, cfg)
                opt.zero_grad(set_to_none=True)
                loss.backward()
                torch.nn.utils.clip_grad_norm_(model.parameters(), 5.0)
                opt.step()
                sched.step()
                tot += loss.item()
                done += 1
                if done % args.get("save_every", 200) == 0:
                    save(ep, done)
                if (run / "PAUSE").exists():
                    save(ep, done)
                    (run / "PAUSE").unlink()
                    print(f"paused at epoch {ep + 1}, batch {done}; go on with: --resume {run}", flush=True)
                    return
        except KeyboardInterrupt:
            save(ep, done)
            print(f"stopped at epoch {ep + 1}, batch {done}; go on with: --resume {run}", flush=True)
            return
        val = evaluate(model, val_dl, dev)
        row = dict(epoch=ep + 1, loss=round(tot / max(1, done), 4), seconds=round(time.time() - t0, 1), **val)
        log.write(json.dumps(row) + "\n")
        log.flush()
        print(row, flush=True)
        state = dict(model=model.state_dict(), config=cfg, epoch=ep + 1, val=val, params=params)
        torch.save(state, run / "last.pt")
        if val["f1"] > best:
            best = val["f1"]
            torch.save(state, run / "best.pt")
        save(ep + 1, 0)
    print(f"best val F1 {best:.4f}; checkpoints in {run}")


if __name__ == "__main__":
    main()
