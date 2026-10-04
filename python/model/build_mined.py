"""Training crops mined from full_v3's own mistakes (REPRODUCE.md step 1). Not checked by eye and not trained on yet.

The recordings: those with a KovaaK stats file, of every scenario kind, the newest --per-folder of each scenario folder,
the stats-file checks' runs left out (build_data.check_runs). Moving kinds come first (dynamic and switching in turn,
then tracking), then static; --budget stops the native reviews after that many seconds.

Each recording is reviewed as the app reviews it (aimview-tool review: full_v3's _u8in export on DirectML, the
recording's areas, the scenario's target count), kept in <out>/reviews/<stem>/ so a rerun skips it. Its tracks and
the camera's turn (readings.json: phase correlation over the whole frame; the tracks' own shift, which a moving target
fools, only as a second opinion) give four rules, each kept only inside the run (from the stats file's start, else the
end of KovaaK's countdown, to the run's length; half a second in from each end). A chain is the tracks appearances()
joins (one target picked up again); a verified chain is a target for sure: one a clicking run's matched kills
(review.match) end, or in a tracking run one held near the crosshair a fifth of the time (a cloud, a name tag or a wall
seam the model boxes steadily is neither).
- kill: a kill of a dynamic or switching run, matched as the core matches them (review.match: the clock offset voted
  from the kills, the killed target's chain). In the third of a second before the kill (up to 2 frames before it), a
  frame where the killed target has no box (under the crosshair, faint, merged): its place comes from its own track,
  between two frames where it was seen (up to 0.06 s apart) or up to 0.05 s past the last one by its own speed over
  the world and the camera's turn, and then only when that speed leads it onto the crosshair at the kill. Only runs
  whose clock is sure: KovaaK's countdown ends within 2 frames of the kills' offset, or 8 in 10 kills are confirmed
  (the vote can line the kills up with the crosshair's own boxes after each kill: Pasu Voltaic Reload Easy, 7 frames
  late). The target must have been seen in 8 in 10 frames before it was lost.
- gap: a verified target seen in 9 in 10 frames over a quarter second either side (and 4 frames in a row either side,
  1/30 s at higher frame rates) that the model misses for 1 or 2 frames: its place filled from the frames either side,
  over the world (the camera's turn added back), when its speed and size either side agree. Not within 5 frames of a
  kill, not on a spot where the model marks the crosshair.
- false_static: a box that stays put on screen (within 0.1 deg; 0.03 deg within 1.5 deg of the crosshair) for 0.1 s or
  more while the view turns 1 deg or more (by the camera's reading and by the other targets): it moves with the
  screen, not the world. Only in a static scenario or on a spot where the model marks the crosshair: in a moving
  scenario every target strafing alike stays put on screen while the player follows one of them. The image there must
  be the same (mean difference under 10) as where the box was first and last seen, and no other box within 2 deg in
  the 5 frames either side.
- false_lone: a box in a single frame, 2 deg or more from the crosshair and 24 px or more from the excluded areas, with
  no box within 2 deg of it (on screen and after the camera's turn) in the 3 frames before and after, nor any box of
  the export run again on those frames within 24 px or 4 box sizes; a steady view (the camera's reading and the tracks'
  shift under 0.5 deg a frame, every other box continuing a track); and nothing like it in the image of those frames
  (no patch within 48 to 64 px matches it to a mean difference of 20: a flash, not a target the model saw once).
A placed box (kill, gap) must show in the image, alone: its middle differs from the wall round it by 40 or more (as
build_kills.py checks), the wall round it is plain (no other target touching it), its middle has the color the target
had where the model last saw it, and no other box of the model (the review's, or the export run again on the frame on
the CPU) is there or touching it. Its size: the median of the target's last boxes. The frames are decoded once per
recording (ffmpeg, from the start: frame numbers equal the review's), and each frame used is run through the export
again; a recording whose boxes do not line up with the review's tracks is dropped.

Each mined place gives one 256 x 256 crop round it (shifted up to 48 px at random), saved like build_kills.py's: rgb,
fixed, tmask, boxes (the review's boxes of that frame, the placed ones added and the false ones taken out), plus
scores (the model's, -1 for a placed box), mined (the rule), fix (the box placed or taken out, in the crop), frame and
why. A crop is left out when another box in it is not a verified chain's (seen steadily, not on a crosshair spot), or
when its frame has more boxes than the scenario has targets. Splits follow build_data.split_of. Incremental: a recording already in the manifest (same file and size) is skipped.
--pick N --check <folder>: copies N crops spread over the rules and recordings into <folder> (split folders and
picks.jsonl) for a check by eye with label_check.py; the dataset is left as it is.
Usage: python python/model/build_mined.py [--out test_out/vod_model/data_mined] [--budget 3600] [--per-folder 1]
       [--reviewed-only]
       python python/model/build_mined.py --pick 100 --check test_out/vod_model/check_mined
"""
import argparse
import collections
import hashlib
import json
import math
import queue
import random
import shutil
import subprocess
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import numpy as np
from scipy import ndimage

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
import aimview_tools  # noqa: E402
import build_data  # noqa: E402
import eval_moving  # noqa: E402
import infer  # noqa: E402
import review  # noqa: E402

W, H, CROP = review.W, review.H, 256
MODEL = HERE / "exports" / "detector_full_v3_u8in.onnx"
THRESHOLD = 0.3                              # full_v3's, in its settings file
RULES = ("kill", "gap", "false_static", "false_lone")
CODE = dict(kill="k", gap="g", false_static="s", false_lone="l")
PER_KILL = 4                                 # crops per kill at most
PER_RUN = dict(kill=80, gap=40, false_static=12, false_lone=20)   # crops per recording and rule at most


def px_size(x, y, wd, hd):
    """A box's width and height in pixels from its size in degrees at (x, y) (track.rs keep's inverse, to first
    order)."""
    xa, _ = review.to_px(x - wd / 2, y)
    xb, _ = review.to_px(x + wd / 2, y)
    _, ya = review.to_px(x, y + hd / 2)
    _, yb = review.to_px(x, y - hd / 2)
    return xb - xa, yb - ya


def disc(rgb, fixed, cx, cy, w, h):
    """The median color of a target's middle (a disc half its smaller side across, the fixed pixels left out), of the
    wall round it (a ring 1.6 to 2.6 times that radius), and the share of the ring within 40 of the ring's median, or
    None where too little of either shows."""
    r = max(1.5, 0.5 * min(w, h))
    R = int(math.ceil(2.6 * r)) + 1
    ya, yb = max(0, int(cy) - R), min(H, int(cy) + R + 1)
    xa, xb = max(0, int(cx) - R), min(W, int(cx) + R + 1)
    if yb - ya < 3 or xb - xa < 3:
        return None
    yy, xx = np.mgrid[ya:yb, xa:xb]
    d2 = (xx - cx) ** 2 + (yy - cy) ** 2
    free = fixed[ya:yb, xa:xb] == 0
    vis, ring = (d2 <= r * r) & free, (d2 >= (1.6 * r) ** 2) & (d2 <= (2.6 * r) ** 2) & free
    if vis.sum() < 3 or ring.sum() < 6:
        return None
    px = rgb[ya:yb, xa:xb].astype(np.float32)
    outer = np.median(px[ring], 0)
    return np.median(px[vis], 0), outer, float(np.mean(np.linalg.norm(px[ring] - outer, axis=1) < 40))


def shows(rgb, fixed, cx, cy, w, h, ref=None):
    """Does a target show at (cx, cy), alone: its middle unlike the wall round it (by 40 or more, as build_kills.py
    checks), the wall round it plain (8 in 10 of its pixels within 40 of its median: no other target touching it),
    and its middle within 45 of the color `ref` the target had where it was seen? Returns its middle's color, or
    None."""
    m = disc(rgb, fixed, cx, cy, w, h)
    if m is None or np.linalg.norm(m[0] - m[1]) < 40 or m[2] < 0.8:
        return None
    if ref is not None and np.linalg.norm(m[0] - ref) > 45:
        return None
    return m[0]


def patch(rgb, cx, cy, half):
    x, y = int(round(cx)), int(round(cy))
    if x - half < 0 or y - half < 0 or x + half + 1 > W or y + half + 1 > H:
        return None
    return rgb[y - half:y + half + 1, x - half:x + half + 1].astype(np.float32)


def best_match(tpl, rgb, cx, cy, half, reach):
    """The smallest mean absolute difference between the patch tpl and the patches of rgb centered within `reach` px
    of (cx, cy) (every second pixel), or None where none fits in the frame."""
    x, y = int(round(cx)), int(round(cy))
    region = rgb[max(0, y - reach - half):min(H, y + reach + half + 1),
                 max(0, x - reach - half):min(W, x + reach + half + 1)].astype(np.float32)
    k = 2 * half + 1
    if region.shape[0] < k or region.shape[1] < k:
        return None
    v = np.lib.stride_tricks.sliding_window_view(region, (k, k, 3))[::2, ::2, 0]
    return float(np.abs(v - tpl).mean(axis=(2, 3, 4)).min())


class Run:
    """One recording's review: its tracks as places in degrees, their chains (the tracks appearances() joins: one
    target picked up again), the camera's turn, the run's frames."""

    def __init__(self, folder, kind, stats):
        self.tr = json.loads((folder / "tracks.json").read_text(encoding="utf-8"))
        rd = json.loads((folder / "readings.json").read_text(encoding="utf-8"))
        self.fps, fr = self.tr["fps"], self.tr["frames"]
        self.n = len(fr)
        self.P = []                                  # per frame: (tid, x, y, wd, hd, score)
        self.pos = {}
        for f in fr:
            wh = f.get("wh") or [(0.0, 0.0)] * len(f["t"])
            s = f.get("s") or [1.0] * len(f["t"])
            self.P.append([(tid, x, y, a, b, sc) for (tid, x, y), (a, b), sc in zip(f["t"], wh, s)])
            for tid, x, y in f["t"]:
                self.pos.setdefault(tid, {})[f["i"]] = (x, y)
        cam = np.zeros((self.n, 2))
        self.cam_ok = np.zeros(self.n, bool)
        for i, c in enumerate((rd.get("camera") or [])[:self.n]):
            if c is not None:
                cam[i], self.cam_ok[i] = c[:2], True
        self.cam = cam
        self.shift = np.array([f.get("shift") or (0.0, 0.0) for f in fr], float).reshape(-1, 2)   # the tracks' own
        self.cum = np.cumsum(cam, axis=0)
        self.mask = review.mask_of(self.tr.get("areas") or review.OVERLAY_SHARES)
        self.far = ndimage.binary_erosion(self.mask, iterations=24)   # 24 px or more from the excluded areas
        _, self.follows = review.appearances(self.tr)
        before = {v: k for k, v in self.follows.items()}
        self.root, self.chain = {}, collections.defaultdict(set)
        for tid, p in self.pos.items():
            r = tid
            while r in before:
                r = before[r]
            self.root[tid] = r
            self.chain[r] |= set(p)
        self.span = {r: (min(c), max(c)) for r, c in self.chain.items()}
        self.spots = review.crosshair_spots(fr)      # where the model marks the crosshair itself
        self.kind, self.stats = kind, stats
        self.meta, self.rows = review.load_stats(stats)
        self.flicks, self.info = [], {}
        on = [i for i, c in enumerate(rd.get("countdown") or []) if c]
        countdown = (on[-1] + 1) / self.fps if on else None
        start, self.start_from, self.clock = None, None, False
        if kind != "tracking" and self.rows:
            self.flicks, self.info = review.match(self.tr, stats)
            i = self.info
            if i.get("offset") is not None and i["matched"] >= 0.8 * len(self.rows):
                start, self.start_from = float(i["offset"]), "kills"
                # the kills' clock is trusted only where KovaaK's countdown ends on the same frame (within 2), or
                # where 8 in 10 kills are confirmed (the target last seen at the crosshair within 2 frames of the
                # kill): the vote can line the kills up with the crosshair's own boxes, which the model marks once
                # the target is gone (Pasu Voltaic Reload Easy: 7 frames late)
                self.clock = (countdown is not None and abs(start - countdown) * self.fps <= 2.5) or \
                    i["confirmed"] >= 0.8 * i["matched"]
                if countdown is not None:
                    self.info["countdown"] = countdown
            else:
                self.flicks = []
        if start is None and countdown is not None:
            start, self.start_from = countdown, "countdown"
        length = review.stats_length(stats)
        self.window = None
        if start is not None and length:
            lo, hi = int(round((start + 0.5) * self.fps)), min(self.n - 1, int(round((start + length - 0.5) * self.fps)))
            if hi - lo > self.fps:
                self.window = (lo, hi)
        self.kill_frames = [f["stats_frame"] for f in self.flicks]
        # the chains that are targets for sure: a clicking run's killed ones, a tracking run's held near the
        # crosshair a fifth of the time or more (a cloud, a name tag or a wall seam the model boxes steadily is not)
        self.verified = set()
        at = {(i, x, y): tid for i, ps in enumerate(self.P) for tid, x, y, *_ in ps}
        for f in self.flicks:
            for i, x, y in f["traj"]:
                if (i, x, y) in at and not self.on_spot(x, y):
                    self.verified.add(self.root[at[(i, x, y)]])
        if kind == "tracking":
            pts = collections.defaultdict(list)
            for ps in self.P:
                for tid, x, y, a, b, _ in ps:
                    pts[self.root[tid]].append((x, y, a, b))
            for r, p in pts.items():
                p = np.array(p)
                size = float(np.median(np.maximum(p[:, 2], p[:, 3])))
                if len(p) >= 0.5 * self.fps and np.mean(np.hypot(p[:, 0], p[:, 1]) <= max(1.0, 0.75 * size)) >= 0.2:
                    self.verified.add(r)

    def inside(self, i):
        return self.window is not None and self.window[0] <= i <= self.window[1]

    def turned(self, a, b):
        """The camera's turn from frame a to frame b (degrees), or None where a reading is missing."""
        lo, hi = min(a, b), max(a, b)
        if hi > lo and not self.cam_ok[lo + 1:hi + 1].all():
            return None
        return self.cum[b] - self.cum[a]

    def box_of(self, i, x, y):
        """The review's box at (x, y) in frame i: (wd, hd, score)."""
        for _, px, py, a, b, s in self.P[i]:
            if px == x and py == y:
                return a, b, s
        return None

    def on_spot(self, x, y):
        return any(math.hypot(x - a, y - b) < 0.2 for a, b in self.spots)

    def near(self, i, x, y, r):
        return any(math.hypot(px - x, py - y) < r for _, px, py, *_ in self.P[i])

    def trusted(self, tid, i, x, y):
        """Is the review's box of track tid at (x, y) in frame i a target to keep as a label: a verified chain's, seen
        steadily, and not on a spot where the model marks the crosshair?"""
        return self.root[tid] in self.verified and not self.on_spot(x, y) and self.steady(tid, i)

    def steady(self, tid, i, need=0.8, half=None):
        """Is track tid's target seen steadily round frame i: its chain lasts 0.1 s or more and is seen in `need` of
        the frames it lives within `half` frames (0.125 s) of i? A box the model gives now and then (a wall seam, a
        name tag) is not."""
        r = self.root[tid]
        first, last = self.span[r]
        if last - first + 1 < round(0.1 * self.fps):
            return False
        half = half or int(round(0.125 * self.fps))
        lo, hi = max(first, i - half), min(last, i + half)
        return sum(k in self.chain[r] for k in range(lo, hi + 1)) >= need * (hi - lo + 1)

    def placeable(self, x, y):
        cx, cy = review.to_px(x, y)
        return 8 <= cx < W - 8 and 8 <= cy < H - 8 and self.mask[int(cy), int(cx)]


def size_of(run, p, frames):
    """A target's box (wd, hd in degrees; w, h in pixels): the median over the given frames of its track p."""
    got = []
    for i in frames:
        b = run.box_of(i, *p[i])
        if b and b[0] > 0:
            got.append((b[0], b[1], *px_size(p[i][0], p[i][1], b[0], b[1])))
    return tuple(np.median(np.array(got), 0)) if got else None


def kill_places(run):
    """The kill rule's places: (frame, x, y, wd, hd, w, h, ref frame, ref x, ref y, why)."""
    fps, out = run.fps, []
    L, ahead, gap = int(round(fps / 3)), max(2, int(round(0.05 * fps))), max(2, int(round(0.06 * fps)))
    if not run.clock:
        return out
    for f in run.flicks:
        kf = f["stats_frame"]
        p = {i: (x, y) for i, x, y in f["traj"] if not run.on_spot(x, y)}
        if not p or not run.inside(kf - L) or not run.inside(kf):
            continue
        seen = sorted(p)
        w0, w1 = max(kf - L, seen[0]), max([j for j in seen if j < kf - 1], default=-1)
        if w1 < w0 or sum(1 for j in seen if w0 <= j <= w1) < 0.8 * (w1 - w0 + 1):
            continue                                          # not seen steadily before it was lost
        got = []
        for i in range(max(kf - L, seen[0] + 1), kf - 1):
            if i in p:
                continue
            a = max(j for j in seen if j < i)
            after = [j for j in seen if j > i]
            b = after[0] if after else None
            size = size_of(run, p, [j for j in seen if j <= i][-5:] + (after[:3] if b is not None and b - a - 1 <= gap else []))
            if size is None:
                continue
            wd, hd, w, h = size
            ta = run.turned(a, i)
            if ta is None:
                continue
            if b is not None and b - a - 1 <= gap:
                tb = run.turned(a, b)
                if tb is None:
                    continue
                t = (i - a) / (b - a)
                wa = np.array(p[a]) - run.cum[a]
                wb = np.array(p[b]) - run.cum[b]
                x, y = wa + t * (wb - wa) + run.cum[i]
                why = f"kill {f['n']}: between frames {a} and {b}, where the model saw it"
            elif b is None and i - a <= ahead:
                back = [j for j in seen if a - 4 <= j < a]
                if len(back) < 2 or run.turned(back[0], a) is None:
                    continue
                v = ((np.array(p[a]) - run.cum[a]) - (np.array(p[back[0]]) - run.cum[back[0]])) / (a - back[0])
                # its own speed must lead it onto the crosshair at the kill (the clock: within 2 frames)
                r = 0.5 * max(wd, hd)
                ok = False
                for k in range(kf - 2, kf + 3):
                    tk = run.turned(a, min(k, run.n - 1))
                    if tk is not None and math.hypot(*(np.array(p[a]) + tk + v * (k - a))) <= r + 0.3:
                        ok = True
                if not ok:
                    continue
                x, y = np.array(p[a]) + ta + v * (i - a)
                why = f"kill {f['n']}: {i - a} frames past where the model last saw it, by its own speed"
            else:
                continue
            if run.near(i, x, y, max(0.3, 0.6 * max(wd, hd))) or not run.placeable(x, y):
                continue
            got.append((i, float(x), float(y), wd, hd, w, h, a, *p[a], why))
        if len(got) > PER_KILL:                               # spread over the third of a second
            got = [got[round(k * (len(got) - 1) / (PER_KILL - 1))] for k in range(PER_KILL)]
        out += got
    return out


def gap_places(run):
    """The gap rule's places, as kill_places gives them."""
    fps, out = run.fps, []
    S = max(4, int(round(fps / 30)))
    on_spot = run.on_spot
    for prev, nxt in run.follows.items():
        p, q = run.pos[prev], run.pos[nxt]
        e, s = max(p), min(q)
        if s - e - 1 not in (1, 2) or not run.inside(e - S) or not run.inside(s + S):
            continue
        if any(e - k not in p for k in range(S)) or any(s + k not in q for k in range(S)):
            continue
        if any(abs(k - e) <= 5 or abs(k - s) <= 5 for k in run.kill_frames):
            continue
        if run.root[prev] not in run.verified or not run.steady(prev, e, need=0.9, half=int(round(0.25 * fps))):
            continue                                          # not known for a target, or seen now and then
        if on_spot(*p[e]) or on_spot(*q[s]) or run.turned(e - S, s + S) is None:
            continue
        sb, sa = size_of(run, p, range(e - S + 1, e + 1)), size_of(run, q, range(s, s + S))
        if sb is None or sa is None or not 0.7 <= sb[2] * sb[3] / max(1e-6, sa[2] * sa[3]) <= 1.43:
            continue
        we = lambda t, j: np.array(t[j]) - run.cum[j]          # a place over the world
        vb, va = (we(p, e) - we(p, e - 3)) / 3, (we(q, s + 3) - we(q, s)) / 3
        size = [(u + v) / 2 for u, v in zip(sb, sa)]
        wd, hd, w, h = size
        tol = max(0.15, 0.35 * max(wd, hd))
        if np.hypot(*(we(p, e) + vb * (s - e) - we(q, s))) > tol or np.hypot(*(we(q, s) - va * (s - e) - we(p, e))) > tol:
            continue
        for i in range(e + 1, s):
            t = (i - e) / (s - e)
            x, y = we(p, e) + t * (we(q, s) - we(p, e)) + run.cum[i]
            if run.near(i, x, y, max(0.3, 0.6 * max(wd, hd))) or not run.placeable(x, y):
                continue
            out.append((i, float(x), float(y), wd, hd, w, h, e, *p[e],
                        f"gap: missed for {s - e - 1} frame{'s' if s - e > 2 else ''} between frames {e} and {s}"))
    return out


def static_places(run):
    """The false_static rule: (frame, x, y, wd, hd, first frame, last frame, why), up to 3 frames per spot on screen.
    Only in a static scenario (no target can move: a box that stays put on screen while the view turns is no target) or
    on a spot where the model marks the crosshair (review.crosshair_spots): in a moving one, every target strafing
    alike stays put on screen while the player follows one of them."""
    fps, out = run.fps, []
    lo, hi = run.window
    chains, open_ = [], {}
    for i in range(lo, hi + 1):                      # boxes linked frame to frame by their place on screen alone
        now = {}
        for tid, x, y, a, b, s in run.P[i]:
            best = None
            for key, c in open_.items():
                d = math.hypot(x - c[-1][1], y - c[-1][2])
                if d < 0.08 and key not in now and (best is None or d < best[0]):
                    best = (d, key)
            if best:
                open_[best[1]].append((i, x, y, a, b))
                now[best[1]] = open_[best[1]]
            else:
                key = (i, x, y)
                now[key] = [(i, x, y, a, b)]
                chains.append(now[key])
        open_ = now
    need = max(8, int(round(0.1 * fps)))
    tracks_cum = np.cumsum(run.shift, axis=0)
    spots = []
    for c in chains:
        if len(c) < need or not run.cam_ok[c[0][0] + 1:c[-1][0] + 1].all():
            continue
        pts = np.array([(x, y) for _, x, y, _, _ in c])
        med = np.median(pts, 0)
        if not (run.kind == "static" or run.on_spot(*med)):
            continue
        spread = float(np.hypot(*(pts - med).T).max())
        if spread > (0.03 if math.hypot(*med) < 1.5 else 0.1):
            continue
        ks = [i for i, *_ in c]
        moved = np.hypot(*(run.cum[ks] - run.cum[ks[0]]).T).max()
        moved_tracks = np.hypot(*(tracks_cum[ks] - tracks_cum[ks[0]]).T).max()
        if moved < 1.0 or moved_tracks < 1.0:        # the camera's reading and the other targets agree it turned
            continue
        spot = next((s for s in spots if math.hypot(*(s["at"] - med)) < 0.2), None)
        if spot is None:
            spot = dict(at=med, frames=[])
            spots.append(spot)
        spot["frames"] += [(i, x, y, a, b, ks[0], ks[-1], float(moved), len(c)) for i, x, y, a, b in c]
    for spot in spots:
        ok = []
        for i, x, y, a, b, f0, f1, moved, n in spot["frames"]:
            if i in (f0, f1) or abs(run.cam[i]).max() < 0.02:     # the camera turning at that frame
                continue
            if any(math.hypot(px - x, py - y) < 2.0
                   for j in range(max(0, i - 5), min(run.n, i + 6)) for _, px, py, *_ in run.P[j]
                   if math.hypot(px - spot["at"][0], py - spot["at"][1]) >= 0.15):
                continue
            ok.append((i, x, y, a, b, f0, f1, f"false_static: put on screen for {n} frames while the camera turned "
                                              f"{moved:.1f} deg" + (" (a spot where the model marks the crosshair)"
                                                                    if run.on_spot(x, y) else "")))
        if len(ok) > 3:
            ok = [ok[round(k * (len(ok) - 1) / 2)] for k in range(3)]
        out += ok
    return out


def lone_places(run, single):
    """The false_lone rule's candidates (the image test comes after decoding): (frame, x, y, wd, hd, why)."""
    out = []
    for tid in single:
        (i, (x, y)), = run.pos[tid].items()
        if not run.inside(i - 3) or not run.inside(i + 3) or math.hypot(x, y) < 2.0 or not run.placeable(x, y):
            continue
        cx, cy = review.to_px(x, y)
        if not run.far[int(cy), int(cx)]:                    # a target going into an excluded area shows by bits
            continue
        if not run.cam_ok[i - 2:i + 4].all() or abs(run.cam[i - 2:i + 4]).max() > 0.5:
            continue
        b = run.box_of(i, x, y)
        if b is None or b[0] <= 0:
            continue
        clear = True
        for j in range(i - 3, i + 4):
            t = run.cum[j] - run.cum[i]
            if j != i and (run.near(j, x, y, 2.0) or run.near(j, x + t[0], y + t[1], 2.0)):
                clear = False
            # a steady view: the tracks' shift small too, and every other box continuing a track (in a fast flick
            # the camera's reading fails and every target starts a new track each frame)
            before = {p[0] for p in run.P[j - 1]}
            if math.hypot(*run.shift[j]) > 0.5 or any(p[0] not in before for p in run.P[j] if p[0] != tid):
                clear = False
        if clear:
            out.append((i, x, y, b[0], b[1], "false_lone: a box in one frame, nothing near it in the 3 frames either side"))
    return out


def decode(video, frames):
    """The frames (sorted indices) as RGB 1280 x 720 (ffmpeg's area scaling, as everywhere): one pass from the start, so
    the numbering is the review's; frames between them in the same windows are decoded and dropped."""
    ranges = []
    for i in frames:
        if ranges and i - ranges[-1][1] <= 8:
            ranges[-1][1] = i
        else:
            ranges.append([i, i])
    while len(ranges) > 120:                          # ffmpeg's expressions stay short
        k = min(range(len(ranges) - 1), key=lambda k: ranges[k + 1][0] - ranges[k][1])
        ranges[k][1] = ranges.pop(k + 1)[1]
    sel = "+".join(f"between(n,{a},{b})" for a, b in ranges)
    size = W * H * 3
    p = subprocess.Popen(["ffmpeg", "-v", "error", "-i", str(video), "-vf",
                          f"select='{sel}',scale={W}:{H}:flags=area,format=rgb24", "-fps_mode", "passthrough",
                          "-f", "rawvideo", "-"], stdout=subprocess.PIPE, bufsize=0)
    want, out = set(frames), {}
    order = (i for a, b in ranges for i in range(a, b + 1))
    try:
        for i in order:
            buf = bytearray(size)
            mv, got = memoryview(buf), 0
            while got < size and (k := p.stdout.readinto(mv[got:])):
                got += k
            if got < size:
                break
            if i in want:
                out[i] = np.frombuffer(buf, np.uint8).reshape(H, W, 3)
    finally:
        p.stdout.close()
        p.wait()
    return out


def lined_up(run, boxes):
    """How well the export run again on the decoded frames finds the review's boxes, at frame offsets -1, 0 and 1:
    the share of the review's boxes with one within 0.15 deg (frames where the camera turns, so a frame off shows)."""
    out = {}
    for off in (-1, 0, 1):
        hit = tot = 0
        for i, bs in boxes.items():
            j = i + off
            if not 0 <= j < run.n or not run.P[j] or abs(run.cam[i]).max() < 0.03:
                continue
            got = [review.to_deg(b[0], b[1]) for b in bs]
            for _, x, y, *_ in run.P[j]:
                tot += 1
                hit += any(math.hypot(x - gx, y - gy) < 0.15 for gx, gy in got)
        out[off] = (hit / tot if tot else None, tot)
    return out


def mine(job, folder, det, out):
    """One reviewed recording's crops. Returns its manifest row (without the review's fields)."""
    video, stats, kind, split, stem = job["video"], job["stats"], job["kind"], job["split"], job["stem"]
    row = dict(kept=False, crops=0, rules={r: 0 for r in RULES})
    run = Run(folder, kind, stats)
    row.update(info={k: (round(float(v), 4) if isinstance(v, (float, np.floating)) else v) for k, v in run.info.items()},
               start_from=run.start_from)
    if run.window is None:
        return dict(row, reason="no run window (no kills lined up and no countdown)")
    joined = set(run.follows) | set(run.follows.values())
    single = [t for t, p in run.pos.items() if len(p) == 1 and t not in joined]
    places = dict(kill=kill_places(run) if kind in ("dynamic", "switching") else [],
                  gap=gap_places(run), false_static=static_places(run), false_lone=lone_places(run, single))
    rnd = random.Random(int(stem, 16))
    for r, cap in PER_RUN.items():                  # the image tests drop some: sample twice the cap first
        if len(places[r]) > 2 * cap:
            places[r] = sorted(rnd.sample(places[r], 2 * cap))
    need = set()
    for r, ps in places.items():
        for p in ps:
            need.add(p[0])
            if r in ("kill", "gap"):
                need.add(p[7])
            if r == "false_static":
                need |= {p[5], p[6]}
            if r == "false_lone":
                need |= set(range(p[0] - 3, p[0] + 4))
    row["candidates"] = {r: len(ps) for r, ps in places.items()}
    if not need:
        return dict(row, reason="nothing to mine")
    t0 = time.time()
    frames = decode(video, sorted(need))
    yuvs = build_data.keyframes(video, "yuv420p")
    if len(yuvs) < 3:
        return dict(row, reason="too few key frames")
    fixed = review.fixed_map(yuvs).astype(np.uint8)
    row["decode_s"] = round(time.time() - t0, 1)
    boxes = {i: det(frames[i], fixed, THRESHOLD) for i in sorted(frames)}
    line = lined_up(run, boxes)
    row["lined_up"] = {str(k): [None if v is None else round(v, 3), t] for k, (v, t) in line.items()}
    share0, tot = line[0]
    if tot >= 20 and (share0 is None or share0 < 0.8 or share0 < max((line[-1][0] or 0), (line[1][0] or 0)) + 0.05):
        return dict(row, reason="the decoded frames do not line up with the review's")
    # the rules' image tests
    drop = collections.Counter()                     # why the image tests dropped a place
    fixes = collections.defaultdict(list)            # frame: [(rule, cx, cy, w, h, why)] placed
    falses = collections.defaultdict(list)           # frame: [(rule, cx, cy, w, h, why)] taken out
    for r in ("kill", "gap"):
        for i, x, y, wd, hd, w, h, a, ax, ay, why in places[r]:
            if i not in frames or a not in frames:
                continue
            cx, cy = review.to_px(x, y)
            # the export run again on the CPU can find the target itself (a score near the threshold): its box,
            # about where and as big as the placed one, confirms the place and is left out of the boxes round it
            same = lambda b: math.hypot(b[0] - cx, b[1] - cy) < 0.3 * max(w, h) + 1 and 0.6 < b[2] * b[3] / (w * h) < 1.67
            cpu = [b for b in boxes[i] if same(b)]
            others = [b[:4] for b in boxes[i] if not same(b)] + [(*review.to_px(px, py), *px_size(px, py, a2, b2))
                                                                 for _, px, py, a2, b2, _ in run.P[i]]
            touch = lambda o: abs(o[0] - cx) < (o[2] + w) / 2 + 2 and abs(o[1] - cy) < (o[3] + h) / 2 + 2
            if any(touch(o) for o in others):
                drop[f"{r}: another box there or touching"] += 1
                continue                             # another box of the model there or touching it (a bot's head
                #                                      on its body, two targets merged): which is which is not clear
            if cpu:
                why += " (the export on the CPU finds it at " + f"{max(b[4] for b in cpu):.2f})"
            ref = shows(frames[a], fixed, *review.to_px(ax, ay), w, h)
            if ref is None or shows(frames[i], fixed, cx, cy, w, h, ref) is None:
                drop[f"{r}: does not show" if ref is not None else f"{r}: not clear where seen"] += 1
                continue
            fixes[i].append((r, cx, cy, w, h, why))
    for i, x, y, wd, hd, f0, f1, why in places["false_static"]:
        if not {i, f0, f1} <= set(frames):
            continue
        cx, cy = review.to_px(x, y)
        w, h = px_size(x, y, wd, hd)
        # the image there the same as where it was first and last seen, while the view turned: nothing in the
        # world (a target behind it, or come by) shows there
        half = int(math.ceil(0.5 * max(w, h))) + 3
        tpl = patch(frames[i], cx, cy, half)
        same = [patch(frames[j], cx, cy, half) for j in (f0, f1)]
        if tpl is None or any(p is None or np.abs(p - tpl).mean() >= 10 for p in same):
            drop["false_static: the image there changes"] += 1
            continue
        falses[i].append(("false_static", cx, cy, w, h, why))
    for i, x, y, wd, hd, why in places["false_lone"]:
        if not set(range(i - 3, i + 4)) <= set(frames):
            continue
        cx, cy = review.to_px(x, y)
        w, h = px_size(x, y, wd, hd)
        clear = True
        for j in (i - 3, i - 2, i - 1, i + 1, i + 2, i + 3):   # the export on the CPU, the excluded areas too
            t = run.cum[j] - run.cum[i]
            for px, py in ((cx, cy), review.to_px(x + t[0], y + t[1])):
                if any(math.hypot(b[0] - px, b[1] - py) < max(24.0, 4 * max(w, h)) for b in boxes[j]):
                    clear = False
        if not clear:
            drop["false_lone: the export finds a box near"] += 1
            continue
        half = max(4, int(math.ceil(0.5 * max(w, h))) + 2)
        tpl = patch(frames[i], cx, cy, half)
        if tpl is None:
            continue
        diffs = []
        for j, reach in ((i - 3, 64), (i - 1, 48), (i + 1, 48), (i + 3, 64)):
            t = run.cum[j] - run.cum[i]
            diffs.append(best_match(tpl, frames[j], *review.to_px(x + t[0], y + t[1]), half, reach))
        if any(d is None or d < 20 for d in diffs):
            drop["false_lone: like a patch beside"] += 1
            continue                                 # something like it is there in the frames either side
        falses[i].append(("false_lone", cx, cy, w, h, why + f" (the image differs by {min(diffs):.0f} at best)"))
    # the crops
    d = Path(out) / split
    n = 0
    got = collections.Counter()
    for i in sorted(set(fixes) | set(falses)):
        base = []
        for tid, x, y, wd, hd, s in run.P[i]:        # the review's boxes, with the export's own place and size
            cx, cy = review.to_px(x, y)
            m = [b for b in boxes[i] if math.hypot(b[0] - cx, b[1] - cy) < 2.0]
            b = min(m, key=lambda b: math.hypot(b[0] - cx, b[1] - cy)) if m else (cx, cy, *px_size(x, y, wd, hd), s)
            base.append((tuple(float(v) for v in b[:5]), run.trusted(tid, i, x, y)))
        for _, fx, fy, fw, fh, _ in falses[i]:
            base = [(b, ok) for b, ok in base if math.hypot(b[0] - fx, b[1] - fy) >= max(3.0, 0.5 * max(fw, fh))]
        labels = [(b[:4], b[4], ok) for b, ok in base] + [((cx, cy, w, h), -1.0, True) for _, cx, cy, w, h, _ in fixes[i]]
        if job["cap"] and len(labels) > job["cap"]:
            got["over"] += 1                         # more boxes than the scenario has targets: one is not a target
            continue
        for rule, cx, cy, w, h, why in fixes[i] + falses[i]:
            if got[rule] >= PER_RUN[rule]:
                continue
            x0 = int(np.clip(cx - CROP // 2 + rnd.randint(-48, 48), 0, W - CROP))
            y0 = int(np.clip(cy - CROP // 2 + rnd.randint(-48, 48), 0, H - CROP))
            inside = [lab for lab in labels if x0 <= lab[0][0] < x0 + CROP and y0 <= lab[0][1] < y0 + CROP]
            if not all(ok for *_, ok in inside):
                got["untrusted"] += 1                # another box in it is not known for a target: its label
                continue                             # is not to be trusted
            bb = np.array([(b[0] - x0, b[1] - y0, b[2], b[3]) for b, _, _ in inside], np.float32).reshape(-1, 4)
            tmask = np.zeros((CROP, CROP), np.uint8)
            yy, xx = np.ogrid[0:CROP, 0:CROP]
            for bx, by, bw, bh in bb:
                tmask[((xx - bx) / max(1.0, bw / 2)) ** 2 + ((yy - by) / max(1.0, bh / 2)) ** 2 <= 1] = 1
            np.savez_compressed(d / f"{stem}_{i:05d}_{CODE[rule]}{got[rule]:02d}.npz",
                                rgb=frames[i][y0:y0 + CROP, x0:x0 + CROP], fixed=fixed[y0:y0 + CROP, x0:x0 + CROP],
                                tmask=tmask, boxes=bb, scores=np.array([s for _, s, _ in inside], np.float32),
                                mined=np.str_(rule), fix=np.array([cx - x0, cy - y0, w, h], np.float32),
                                frame=np.int32(i), why=np.str_(why))
            got[rule] += 1
            n += 1
    row["rules"] = {r: got[r] for r in RULES}
    row["dropped"] = dict(drop, untrusted=got["untrusted"], over=got["over"])
    return dict(row, kept=n > 0, crops=n)


def jobs_of(lib, per_folder, vods):
    """The recordings to mine, in order: dynamic and switching folders in turn, then tracking, then static (each kind's
    folders in a fixed shuffle)."""
    kinds, counts, facts = review.scenario_kinds(), review.target_counts(), review.scenario_facts()
    skip = build_data.check_runs()
    by_folder = collections.defaultdict(list)
    for r in lib.recordings:                         # newest first
        m = lib.by_id[r["id"]]
        if not m["stats_file"] or not m["video"]:
            continue
        v = Path(m["video"])
        if Path(vods) not in v.parents:
            continue
        f = v.parent.name
        kind = kinds.get(f.lower())
        if kind is None or (f, v.name) in skip or v.name in eval_moving.SKIP:
            continue
        by_folder[(kind, f)].append(dict(video=str(v), stats=m["stats_file"], kind=kind, folder=f, file=v.name,
                                         split=build_data.split_of(f), cap=counts.get(f.lower()),
                                         limit=facts.get(f.lower(), (None, None))[1],
                                         stem=hashlib.md5(str(v).encode()).hexdigest()[:10]))
    order = {}
    for kind in ("dynamic", "switching", "tracking", "static"):
        fs = sorted(f for k, f in by_folder if k == kind)
        random.Random(0).shuffle(fs)
        order[kind] = [j for f in fs for j in by_folder[(kind, f)][:per_folder]]
    dyn, sw = order["dynamic"], order["switching"]
    mixed = [j for pair in zip(dyn, sw) for j in pair] + dyn[len(sw):] + sw[len(dyn):]
    return mixed + order["tracking"] + order["static"]


def build(a):
    out = Path(a.out)
    for s in ("train", "val", "test"):
        (out / s).mkdir(parents=True, exist_ok=True)
    lib = aimview_tools.Library(a.vods)
    jobs = jobs_of(lib, a.per_folder, a.vods)
    man = out / "manifest.jsonl"
    old = {}
    if man.exists():
        for line in man.read_text(encoding="utf-8").splitlines():
            if line.strip():
                r = json.loads(line)
                old[(r["folder"], r["file"])] = r
    rows, todo = [], []
    for j in jobs:
        j["size"] = Path(j["video"]).stat().st_size
        r = old.pop((j["folder"], j["file"]), None)
        (rows.append(r) if r and r.get("size") == j["size"] else todo.append(j))
    rows += list(old.values())                       # rows from other runs (another --per-folder) stay
    print(f"{len(jobs)} recordings: {len(jobs) - len(todo)} done before, {len(todo)} to do; "
          f"review budget {a.budget:.0f} s", flush=True)
    if a.limit:
        todo = todo[:a.limit]
    det = infer.OnnxDetector(MODEL, threads=4)
    reviewed = queue.Queue(maxsize=a.workers + 1)
    spent = [0.0]
    lock = threading.Lock()

    def reviewer():
        for j in todo:
            if spent[0] >= a.budget:
                break
            folder = out / "reviews" / j["stem"]
            secs = 0.0
            if (folder / "job.json").exists():           # reviewed before: its time counts all the same
                secs = json.loads((folder / "job.json").read_text(encoding="utf-8")).get("seconds", 0.0)
            elif a.reviewed_only:
                continue
            elif not ((folder / "tracks.json").exists() and (folder / "readings.json").exists()):
                try:
                    res = lib.review_video(j["video"], str(MODEL), out=folder, stats=j["stats"], kind=j["kind"],
                                           limit=j["limit"], cap=j["cap"], quiet=True)
                    secs = res["seconds"]
                    (folder / "job.json").write_text(json.dumps(dict(j, seconds=secs)), encoding="utf-8")
                except Exception as e:               # one bad recording does not stop the build
                    reviewed.put((j, None, f"review: {type(e).__name__}: {e}"[:160]))
                    continue
            spent[0] += secs
            reviewed.put((j, folder, secs))
        for _ in range(a.workers):
            reviewed.put(None)

    def miner():
        while (item := reviewed.get()) is not None:
            j, folder, secs = item
            row = dict(folder=j["folder"], file=j["file"], size=j["size"], split=j["split"], kind=j["kind"],
                       stem=j["stem"], kept=False, crops=0)
            if folder is None:
                row["reason"] = secs
            else:
                t0 = time.time()
                try:
                    row.update(mine(j, folder, det, out), review_s=round(secs, 1))
                except Exception as e:
                    row.update(reason=f"{type(e).__name__}: {e}"[:160], review_s=round(secs, 1))
                row["mine_s"] = round(time.time() - t0, 1)
            with lock:
                rows.append(row)
                with open(man, "w", encoding="utf-8") as fh:
                    fh.writelines(json.dumps(r) + "\n" for r in sorted(rows, key=lambda r: (r["split"], r["folder"], r["file"])))
                rs = row.get("rules") or {}
                print(f"[{len(rows)}] {row['split']:5s} {row['kind'][:6]:6s} {row['folder'][:38]:38s} "
                      f"{' '.join(f'{CODE[r]}{rs.get(r, 0)}' for r in RULES)} {row.get('reason', '')} "
                      f"(review {spent[0]:.0f} s, {time.time() - started:.0f} s)", flush=True)

    started = time.time()
    t = threading.Thread(target=reviewer)
    t.start()
    with ThreadPoolExecutor(a.workers) as pool:
        for f in [pool.submit(miner) for _ in range(a.workers)]:
            f.result()
    t.join()
    done = [r for r in rows if "mine_s" in r or r.get("kept")]
    print(f"reviewed for {spent[0]:.0f} s; {len(done)} recordings mined")
    for s in ("train", "val", "test"):
        rs = [r for r in rows if r["split"] == s]
        print(f"{s}: {sum(r['kept'] for r in rs)} of {len(rs)} recordings, {sum(r['crops'] for r in rs)} crops: " +
              ", ".join(f"{r} {sum((x.get('rules') or {}).get(r, 0) for x in rs)}" for r in RULES))


def pick(a):
    """--pick: crops spread over the rules (an equal share each, as far as a rule has crops) and over the recordings
    (one from each in turn), copied with picks.jsonl."""
    data, out = Path(a.out), Path(a.check)
    if out.exists() and any(out.iterdir()):
        sys.exit(f"{out} is not empty")
    rec = {}
    for line in open(data / "manifest.jsonl", encoding="utf-8"):
        r = json.loads(line)
        rec[r.get("stem")] = r
    by_rule = collections.defaultdict(lambda: collections.defaultdict(list))
    for f in sorted(data.glob("*/*.npz")):
        stem, _, tag = f.stem.split("_")
        rule = next(r for r, c in CODE.items() if tag.startswith(c))
        by_rule[rule][stem].append(f)
    rnd = random.Random(a.seed)
    quota, left = {}, a.pick
    for k, rule in enumerate(sorted(by_rule, key=lambda r: sum(map(len, by_rule[r].values())))):
        quota[rule] = min(sum(map(len, by_rule[rule].values())), left // (len(by_rule) - k))
        left -= quota[rule]
    picked = []
    for rule, recs in by_rule.items():
        queues = []
        for stem in sorted(recs):
            fs = list(recs[stem])
            rnd.shuffle(fs)
            queues.append(fs)
        rnd.shuffle(queues)
        got = 0
        while got < quota[rule] and any(queues):
            for q in queues:
                if q and got < quota[rule]:
                    picked.append((rule, q.pop(0)))
                    got += 1
    out.mkdir(parents=True, exist_ok=True)
    with open(out / "picks.jsonl", "w", encoding="utf-8") as fh:
        for rule, f in picked:
            (out / f.parent.name).mkdir(exist_ok=True)
            shutil.copy2(f, out / f.parent.name / f.name)
            z = np.load(f)
            r = rec[f.stem.split("_")[0]]
            fh.write(json.dumps(dict(file=f"{f.parent.name}/{f.name}", folder=r["folder"], video=r["file"],
                                     kind=r["kind"], rule=rule, frame=int(z["frame"]), boxes=len(z["boxes"]),
                                     fix=[round(float(v), 1) for v in z["fix"]], why=str(z["why"]))) + "\n")
    c = collections.Counter(r for r, _ in picked)
    print(f"{len(picked)} crops from {len({f.stem.split('_')[0] for _, f in picked})} recordings into {out}: " +
          ", ".join(f"{r} {c[r]}" for r in RULES))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--vods", default=r"E:\OBS\KovOBS")
    ap.add_argument("--out", default="test_out/vod_model/data_mined")
    ap.add_argument("--per-folder", type=int, default=1, help="the newest recordings with a stats file per folder")
    ap.add_argument("--budget", type=float, default=3600, help="seconds of native review, then stop")
    ap.add_argument("--workers", type=int, default=2, help="recordings mined at once (CPU: decoding, the export)")
    ap.add_argument("--limit", type=int, help="at most this many new recordings (a trial)")
    ap.add_argument("--reviewed-only", action="store_true", help="only the recordings reviewed before (no new review)")
    ap.add_argument("--pick", type=int, help="pick this many crops for a check by eye (with --check)")
    ap.add_argument("--check", help="the check folder --pick copies into")
    ap.add_argument("--seed", type=int, default=0)
    a = ap.parse_args()
    if a.pick:
        if not a.check:
            sys.exit("--pick needs --check <folder>")
        return pick(a)
    build(a)


if __name__ == "__main__":
    main()
