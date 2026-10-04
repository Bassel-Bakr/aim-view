"""VOD review: the pipeline behind python/server.py and the step scripts (track_vod.py, flicks.py, measure.py).

1. track: decode a KovOBS recording with ffmpeg, find the dark round targets outside the overlay in every frame (spread
   over CPU cores), and link them from frame to frame. Every target is still in the world, so they all share the
   camera's motion; the shift between frames is voted from pairs.
2. match: a kill is a track that ends within 0.6 deg of the crosshair. The kills are matched with the stats file's kill
   times (one constant offset), and each kill's target path from the previous kill is a flick.
3. measure: per flick, the reaction, the main flick, where it ended (short of the target, on it, or past it), the
   corrections, the time on the target and the still time before the click, the speed and offset at the click.
4. summarize and judge: medians, splits by distance and direction, the time budget, target choice, pacing, and
   provisional checks for the issues in docs/issues.md (thresholds to settle with the user).
Positions are view degrees from the crosshair: x right, y up.
"""
import collections
import functools
import hashlib
import glob
import os
import re
import json
import math
import multiprocessing as mp
import queue
import statistics as st
import subprocess
import threading
from datetime import datetime
from multiprocessing import shared_memory
from pathlib import Path

import numpy as np
from scipy import ndimage

W, H = 1280, 720
CX, CY = 640.03, 359.75                     # the crosshair's centre at this scale (the red dot, measured)
K = (W / 2) / math.tan(math.radians(51.5))  # 103 deg horizontal FOV (Overwatch scale)
# The KovOBS overlay at 1280 x 720, masked out: session box, timer, clock and FPS, settings box, gun and title,
# crosshair zoom and hand cam, version number.
OVERLAY = ((0, 0, 205, 150), (590, 0, 690, 60), (1160, 0, W, 100), (0, 620, 430, H), (570, 630, 715, H),
           (400, 675, 880, H), (960, 535, W, H), (0, 700, 60, H))   # (400, 675, 880, H): the scenario's name, any length
OVERLAY_KINDS = ("Session stats", "Timer", "Clock", "Settings", "Weapon", "Scenario name", "Webcam", "Version")
EXCLUDE_KINDS = ("Session stats", "Timer", "Clock", "Scenario name", "Magazine", "Weapon", "Settings", "Webcam",
                 "Zoomed crosshair", "Version", "Other")   # what an excluded area can be (the user's list; they can add)
OVERLAY_SHARES = [[x0 / W, y0 / H, x1 / W, y1 / H, kind]  # the default exclude areas, labelled
                  for (x0, y0, x1, y1), kind in zip(OVERLAY, OVERLAY_KINDS)]


def mask_of(boxes):
    """Where targets count: everywhere but the excluded boxes, given as shares of the frame [x0, y0, x1, y1] and
    optionally what the area is (the review app's "Exclude areas": another player's webcam or overlay)."""
    m = np.ones((H, W), bool)
    for x0, y0, x1, y1 in (b[:4] for b in boxes):
        m[int(round(y0 * H)):int(round(y1 * H)), int(round(x0 * W)):int(round(x1 * W))] = False
    return m


MASK = mask_of(OVERLAY_SHARES)


def to_deg(x, y):
    return math.degrees(math.atan((x - CX) / K)), math.degrees(math.atan((CY - y) / math.hypot(K, x - CX)))


def to_px(xd, yd):
    """The inverse of to_deg, at 1280 x 720."""
    x = CX + K * math.tan(math.radians(xd))
    return x, CY - math.tan(math.radians(yd)) * math.hypot(K, x - CX)


# ---- 1. track -------------------------------------------------------------------------------------------------------
def probe(video):
    out = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries",
                          "stream=r_frame_rate:format=duration", "-of", "json", video], capture_output=True, text=True)
    j = json.loads(out.stdout)
    num, den = (int(v) for v in j["streams"][0]["r_frame_rate"].split("/"))
    return num / den, float(j["format"]["duration"])


PEAK = 80            # and its strongest pixel must differ at least this much
DIFF = 30            # how far a spot differs from the wall behind it: brightness difference + 2 x colour difference
FRAME = W * H * 3 // 2                           # one frame as ffmpeg gives it: YUV 4:2:0, brightness full size, colour half


def _blur_up(plane, block, up):
    """The wall behind every pixel of a plane: block means, blurred over 5 x 5 blocks, scaled back up by `up`."""
    h, w = plane.shape
    small = plane.reshape(h // block, block, w * 1).sum(axis=1, dtype=np.uint32)
    small = small.reshape(h // block, w // block, block).sum(axis=2, dtype=np.uint32).astype(np.float32) / block ** 2
    small = ndimage.uniform_filter(small, size=5, mode="nearest")
    return np.repeat(np.repeat(small, up, axis=0), up, axis=1)


def contrast(buf):
    """How far every pixel differs from the wall behind it, in any colour: |brightness - wall| at full size plus
    2 x (|U - wall| + |V - wall|) from the half-size colour planes. A few ms a frame."""
    a = np.frombuffer(buf, np.uint8)
    y = a[:W * H].reshape(H, W)
    u = a[W * H:W * H * 5 // 4].reshape(H // 2, W // 2)
    v = a[W * H * 5 // 4:FRAME].reshape(H // 2, W // 2)
    c = np.abs(y - _blur_up(y, 4, 4))
    cc = np.abs(u - _blur_up(u, 2, 2)) + np.abs(v - _blur_up(v, 2, 2))
    c += 2 * np.repeat(np.repeat(cc, 2, axis=0), 2, axis=1)
    return c


def fixed_map(frames):
    """The pixels that stay put on screen while the view moves (crosshair, HUD text, a gun model): those that stand
    out from the wall in at least 80% of a few dozen frames spread over the run. Also the detector model's 4th input."""
    return np.mean([contrast(f) > DIFF for f in frames], axis=0) >= 0.8


def screen_mask(frames, base=None):
    """Where targets can be seen, and the crosshair. Whatever stays put on screen while the view moves (HUD text, a gun
    model) stands out from the wall in at least 80% of a few dozen frames spread over the run; it is masked, with the
    KovOBS overlay boxes. The crosshair is the fixed spot at the centre, in any colour or shape. It is not masked,
    since in hold-fire runs the target sits on it; detect() skips a spot that is just the crosshair instead.
    Returns (mask, fixed, cross): cross = (x px, y px, area px) or None."""
    fixed = fixed_map(frames)
    lab, n = ndimage.label(fixed)
    cross = None
    if n:
        near = lab[int(CY) - 6:int(CY) + 7, int(CX) - 6:int(CX) + 7]
        ids = [i for i in np.unique(near) if i]
        if ids:
            ys, xs = np.nonzero(np.isin(lab, ids))
            cross = (float(xs.mean()), float(ys.mean()), int(len(xs)))
            fixed = fixed & ~np.isin(lab, ids)
    fixed = ndimage.binary_dilation(fixed, iterations=2)
    return (MASK if base is None else base) & ~fixed, fixed, cross


def _blocks(p, k, how):
    """Block statistics of a plane over k x k blocks: "sum", "max" or "min" (two cheap passes)."""
    h, w = p.shape
    if how == "sum":
        t = p.reshape(h // k, k, w).sum(axis=1, dtype=np.uint32)
        return t.reshape(h // k, w // k, k).sum(axis=2, dtype=np.uint32)
    f = np.max if how == "max" else np.min
    return f(f(p.reshape(h // k, k, w), axis=1).reshape(h // k, w // k, k), axis=2)


_mask_blocks = {}


def detect(buf, mask=None, cross=None):
    """The targets in one frame (YUV 4:2:0 bytes or array, W x H): (x deg, y deg, area px) each. A target is a compact
    spot, 3-120 px across, that differs from the wall behind it, in any colour: brightness difference + 2 x colour
    difference over DIFF. Blocks of 4 x 4 px are checked first at low resolution (could any pixel in the block pass?),
    and the full-resolution contrast is worked out only around those, so a frame takes a few ms."""
    m = MASK if mask is None else mask
    mb = _mask_blocks.get(id(m))
    if mb is None:
        mb = _mask_blocks[id(m)] = m.reshape(H // 4, 4, W // 4, 4).any(axis=(1, 3))
    a = np.frombuffer(buf, np.uint8)
    y = a[:W * H].reshape(H, W)
    u = a[W * H:W * H * 5 // 4].reshape(H // 2, W // 2)
    v = a[W * H * 5 // 4:FRAME].reshape(H // 2, W // 2)
    bgy = ndimage.uniform_filter(_blocks(y, 4, "sum").astype(np.float32) / 16, size=5, mode="nearest")
    dy = np.maximum(_blocks(y, 4, "max") - bgy, bgy - _blocks(y, 4, "min"))
    cc = np.zeros((H // 2, W // 2), np.float32)
    for plane in (u, v):
        bg = ndimage.uniform_filter(_blocks(plane, 2, "sum").astype(np.float32) / 4, size=5, mode="nearest")
        cc += np.abs(plane - np.repeat(np.repeat(bg, 2, axis=0), 2, axis=1))
    cand = (dy + 2 * _blocks(cc, 2, "max") > DIFF) & mb
    lab_s, n = ndimage.label(cand)
    if n > 150:                                      # a menu or the results screen, not play
        return []
    out = []
    for sl in ndimage.find_objects(lab_s) if n else []:
        r0, r1 = max(0, sl[0].start - 1), min(H // 4, sl[0].stop + 1)
        c0, c1 = max(0, sl[1].start - 1), min(W // 4, sl[1].stop + 1)
        ys, xs = slice(4 * r0, 4 * r1), slice(4 * c0, 4 * c1)
        c = np.abs(y[ys, xs] - np.repeat(np.repeat(bgy[r0:r1, c0:c1], 4, axis=0), 4, axis=1))
        c += 2 * np.repeat(np.repeat(cc[2 * r0:2 * r1, 2 * c0:2 * c1], 2, axis=0), 2, axis=1)
        lab, k = ndimage.label((c > DIFF) & m[ys, xs])
        if not k:
            continue
        py, px = np.nonzero(lab)
        ls = lab[py, px]
        w = c[py, px]
        area = np.bincount(ls, minlength=k + 1)
        sw = np.bincount(ls, w, k + 1)
        sx = np.bincount(ls, w * px, k + 1)
        sy = np.bincount(ls, w * py, k + 1)
        peak = np.zeros(k + 1, np.float32)
        np.maximum.at(peak, ls, w)
        found = []
        for i, b in enumerate(ndimage.find_objects(lab), 1):
            h, wd = b[0].stop - b[0].start, b[1].stop - b[1].start
            # a target stands out strongly (peaks near 150); the specks of a textured ceiling reach 60-70
            if 3 <= h <= 120 and 3 <= wd <= 120 and area[i] >= 8 and area[i] >= 0.45 * h * wd and 0.5 <= h / wd <= 2 \
                    and peak[i] >= PEAK:
                x, y_ = 4 * c0 + sx[i] / sw[i], 4 * r0 + sy[i] / sw[i]
                if cross and math.hypot(x - cross[0], y_ - cross[1]) < 2.5 and area[i] <= 1.3 * cross[2]:
                    continue                        # the crosshair alone
                found.append((*to_deg(x, y_), int(area[i])))
        if len(found) <= 8:                         # more in one cluster is texture, not targets
            out += found
    uniq = []                                       # neighbouring clusters' windows overlap: one target, once
    for d in out:
        if all(math.hypot(d[0] - u[0], d[1] - u[1]) > 0.05 for u in uniq):
            uniq.append(d)
    return uniq


SPIKE, SPIKE_RATIO = 1.0, 3.0      # a frame's shift is a spike past 1 deg and 3 times the shifts either side (link)


def _view_shift(prev, pts):
    """How far the view moved since the frame before (link)."""
    shift = (0.0, 0.0)
    if prev and pts:
        # every pairing of a spot before with a spot now is a candidate shift; the one most pairings agree with
        # (within 0.35 deg) wins, and the shift is their mean (numpy: the frames can hold a dozen spots or more)
        A = np.array([(a[1], a[2]) for a in prev])
        B = np.array([(b[0], b[1]) for b in pts])
        D = (B[None, :, :] - A[:, None, :]).reshape(-1, 2)
        ok = np.hypot(D[:, 0], D[:, 1]) <= 6.0
        if ok.any() and len(D) <= 2500:
            M = np.hypot(*(D[:, None, :] - D[None, :, :]).transpose(2, 0, 1)) < 0.35
            counts = np.where(ok, M.sum(axis=1), -1)
            inl = D[M[int(np.argmax(counts))]]
            shift = (float(inl[:, 0].mean()), float(inl[:, 1].mean()))
    return shift


def _follow(prev, pts, shift, next_id):
    """The frame's targets given ids (link): each track before, moved by the shift, takes the nearest target now
    within 0.5 deg; a target nobody took starts a new track. Returns them and the next free id."""
    cur, used = [], set()
    for a in prev:
        px, py = a[1] + shift[0], a[2] + shift[1]
        cand = [(math.hypot(b[0] - px, b[1] - py), j) for j, b in enumerate(pts) if j not in used]
        if cand:
            d, j = min(cand)
            if d < 0.5:
                used.add(j)
                cur.append((a[0], *pts[j]))
    for j, b in enumerate(pts):
        if j not in used:
            cur.append((next_id, *b))
            next_id += 1
    return cur, next_id


def _linked(prev, pts, shift):
    """How many of the tracks before take a target now with this shift (link)."""
    return len(pts) - _follow(prev, pts, shift, 0)[1]


def link(dets):
    """Track ids for the per-frame detections (lists of (x, y, area), or (x, y, area, width deg, height deg, score)
    from the model). Each frame's shift is found first, against the frame before as it was tracked. A kill can fool
    it on a plain wall (another target lines up with the dead one's place for one frame): a spike, more than SPIKE
    deg and SPIKE_RATIO times the shifts either side, is replaced by the mean of those two when that mean links as
    many of the frame before's targets as the spike does (a spike that lines up more of them is the camera's own jerk,
    frames captured unevenly, and stays). Then the frames are tracked with these shifts."""
    found, next_id, prev, before = [], 0, [], []
    for pts in dets:
        found.append(_view_shift(prev, pts))
        before.append(prev)
        prev, next_id = _follow(prev, pts, found[-1], next_id)
    size = [math.hypot(*s) for s in found]
    shifts = list(found)
    for j in range(1, len(found) - 1):
        if size[j] > SPIKE and size[j] > SPIKE_RATIO * max(size[j - 1], size[j + 1]):
            mean = ((found[j - 1][0] + found[j + 1][0]) / 2, (found[j - 1][1] + found[j + 1][1]) / 2)
            if _linked(before[j], dets[j], mean) >= _linked(before[j], dets[j], found[j]):
                shifts[j] = mean
    frames, next_id, prev = [], 0, []
    for fi, pts in enumerate(dets):
        shift = shifts[fi]
        cur, next_id = _follow(prev, pts, shift, next_id)
        frames.append(dict(i=fi, shift=shift, t=[(c[0], round(c[1], 4), round(c[2], 4)) for c in cur],
                           a=[c[3] for c in cur]))
        if any(len(c) > 4 for c in cur):                # the model's box sizes (deg): tracking's on-target test
            frames[-1]["wh"] = [[round(c[4], 3), round(c[5], 3)] for c in cur]
        if any(len(c) > 6 for c in cur):                # and its scores: the review app's faint-target cut-off
            frames[-1]["s"] = [round(c[6], 3) for c in cur]
        prev = cur
    return frames


def _frames(video, keyframes=False):
    """Every frame from ffmpeg as YUV 4:2:0 (the video's own format, so no colour conversion), in order (or only the key frames, for a quick look over the whole run). Seeking is
    avoided: OBS files start with an edit list, and seeking in them lands a frame off."""
    p = subprocess.Popen(["ffmpeg", "-v", "error"] + (["-skip_frame", "nokey"] if keyframes else []) + ["-i", video] +
                         (["-fps_mode", "passthrough"] if keyframes else []) +
                         ["-vf", f"scale={W}:{H}:flags=area,format=yuv420p", "-f", "rawvideo", "-"],
                         stdout=subprocess.PIPE, bufsize=0)
    yield from _read(p, FRAME)


def _read(p, size):
    """Frames of `size` bytes from a process's output, each in a buffer of its own. readinto on an unbuffered pipe
    takes half the time of read(): no copy through Python's buffer and no new bytes object per read."""
    try:
        while True:
            buf = bytearray(size)
            mv, got = memoryview(buf), 0
            while got < size:
                k = p.stdout.readinto(mv[got:])
                if not k:
                    return
                got += k
            yield buf
    finally:
        p.stdout.close()
        p.wait()


SLOTS = 64                                       # frames in flight in shared memory
_slots = _mask = _cross = None


def _attach(name, mask, cross):
    global _slots, _shm, _mask, _cross
    _shm = shared_memory.SharedMemory(name=name)
    _slots = np.ndarray((SLOTS, FRAME), np.uint8, buffer=_shm.buf)
    _mask, _cross = mask, cross


def _detect_slot(slot):
    return detect(_slots[slot], _mask, _cross)


def track(video, progress=None, workers=None, mask=None):
    """Find the screen mask from the key frames, then decode in order into shared memory and detect in parallel,
    then link. Returns dict(fps, frames, fixed): fixed is the share of the screen found fixed (crosshair, HUD)."""
    fps, duration = probe(video)
    total = int(round(fps * duration))
    if progress:
        progress("looking", 0, total)
    keys = list(_frames(video, keyframes=True))
    step = max(1, len(keys) // 60)
    mask, fixed, cross = screen_mask(keys[::step], mask)
    shm = shared_memory.SharedMemory(create=True, size=SLOTS * FRAME)
    slots = np.ndarray((SLOTS, FRAME), np.uint8, buffer=shm.buf)
    dets, pending = [], collections.deque()
    try:
        with mp.Pool(workers or max(1, (mp.cpu_count() or 2) - 2), initializer=_attach,
                     initargs=(shm.name, mask, cross)) as pool:
            for k, buf in enumerate(_frames(video)):
                while len(pending) >= SLOTS - 2:          # the oldest frames done, so their slots are free
                    dets.append(pending.popleft().get())
                slots[k % SLOTS] = np.frombuffer(buf, np.uint8)
                pending.append(pool.apply_async(_detect_slot, (k % SLOTS,)))
                if progress and k % 120 == 0:
                    progress("tracking", k, total)
            while pending:
                dets.append(pending.popleft().get())
    finally:
        shm.close()
        shm.unlink()
    if progress:
        progress("linking", len(dets), total)
    return dict(fps=fps, frames=link(dets), fixed=float(fixed.mean()), key_frames=len(keys), crosshair=cross)


def rgb_frames(video):
    """Every frame as RGB (H, W, 3) uint8 at 1280 x 720, in order: the detector model's input."""
    p = subprocess.Popen(["ffmpeg", "-v", "error", "-i", video, "-vf", f"scale={W}:{H}:flags=area,format=rgb24",
                          "-f", "rawvideo", "-"], stdout=subprocess.PIPE, bufsize=0)
    for buf in _read(p, W * H * 3):
        yield np.frombuffer(buf, np.uint8).reshape(H, W, 3)


def rgb_batches(video, batch=16, ring=4):
    """Every frame as RGB (H, W, 3) at 1280 x 720, in batches: pinned torch tensors (n <= batch, H, W, 3) uint8, the
    same frames as rgb_frames(). A thread reads ffmpeg's output straight into a ring of buffers (no copy per frame)
    while the caller works on the previous batch; a buffer is reused once the caller asks for the next one, so the
    caller must be done with it by then (TorchDetector.batch is: it ends by copying its result back).
    Kept to ffmpeg's own decoding and RGB conversion on purpose: the model was trained on them. Decoding on the GPU or
    converting NV12 on the GPU was faster but changed detections at the crosshair (Pokeball 1: 50 confirmed kills
    against 61)."""
    import torch
    size = W * H * 3
    cmd = ["ffmpeg", "-v", "error", "-i", str(video), "-vf", f"scale={W}:{H}:flags=area,format=rgb24",
           "-f", "rawvideo", "-"]
    bufs = [torch.empty((batch, H, W, 3), dtype=torch.uint8).pin_memory() for _ in range(ring)]
    free, ready = queue.Queue(), queue.Queue()
    for i in range(ring):
        free.put(i)
    stop = threading.Event()

    def read():
        try:
            p = subprocess.Popen(cmd, stdout=subprocess.PIPE, bufsize=0)
            try:
                while not stop.is_set():
                    slot = free.get()
                    view = bufs[slot].numpy().reshape(batch, size)
                    n = 0
                    while n < batch:
                        mv, got = memoryview(view[n]), 0
                        while got < size and (k := p.stdout.readinto(mv[got:])):
                            got += k
                        if got < size:
                            break
                        n += 1
                    if n:
                        ready.put((slot, n))
                    if n < batch:
                        break
            finally:
                p.stdout.close()
                p.wait()
        except BaseException as e:                       # handed to the caller
            ready.put(e)
        ready.put(None)

    threading.Thread(target=read, daemon=True).start()
    try:
        while (item := ready.get()) is not None:
            if isinstance(item, BaseException):
                raise item
            slot, n = item
            yield bufs[slot][:n]
            free.put(slot)
    finally:                                             # the caller stopped early: let the reader end
        stop.set()
        free.put(0)


def _prefetch(it, n):
    """Run a generator in a thread, n items ahead, so decoding overlaps the detector."""
    q = queue.Queue(n)
    end = object()

    def work():
        try:
            for x in it:
                q.put(x)
            q.put(end)
        except BaseException as e:                     # handed to the consumer
            q.put(e)

    threading.Thread(target=work, daemon=True).start()
    while (x := q.get()) is not end:
        if isinstance(x, BaseException):
            raise x
        yield x


SCENARIOS = (r"C:\Program Files (x86)\Steam\steamapps\common\FPSAimTrainer\FPSAimTrainer\Saved\SaveGames\Scenarios",
             r"C:\Program Files (x86)\Steam\steamapps\workshop\content\824270")


@functools.lru_cache(maxsize=1)
def target_counts():
    """Targets alive at once per scenario (lower-case name), from AddedBots in its .sce (one entry per bot)."""
    out = {}
    for p in glob.glob(os.path.join(SCENARIOS[0], "*.sce")) + glob.glob(os.path.join(SCENARIOS[1], "*", "*.sce")):
        with open(p, encoding="utf-8", errors="replace") as f:
            m = re.search(r"^AddedBots=(.*)$", f.read().split("[Map Data]")[0], re.M)
        if m:
            out[os.path.basename(p)[:-4].lower()] = len([b for b in m.group(1).strip().split(";") if b])
    return out



def scenario_kinds():
    """Each scenario's kind (lower-case name): "static" or "dynamic" clicking, "tracking" or "switching". From the
    game's AimTypeTag and AimSubTypeTag; an untagged (older) file is tracking when its first weapon fires fully
    automatic, static clicking when no bot can move (every MaxSpeed 0), and dynamic clicking otherwise."""
    return {n: k for n, (k, _) in scenario_facts().items()}


def scenario_facts():
    """Each scenario's (kind, time limit in seconds or None), by lower-case name. scenario_kinds() has the rule."""
    out = {}
    for p in glob.glob(os.path.join(SCENARIOS[0], "*.sce")) + glob.glob(os.path.join(SCENARIOS[1], "*", "*.sce")):
        with open(p, encoding="utf-8", errors="replace") as f:
            t = f.read().split("[Map Data]")[0]
        tag = (re.search(r"^AimTypeTag=(.*)$", t, re.M) or [None, ""])[1].strip()
        sub = (re.search(r"^AimSubTypeTag=(.*)$", t, re.M) or [None, ""])[1].strip()
        if tag == "Tracking":
            kind = "tracking"
        elif tag == "Target Switching":
            kind = "switching"
        elif tag == "Clicking" and sub in ("Static", "Dynamic"):
            kind = sub.lower()
        else:
            auto = re.search(r"^Category=(\w+)", t.split("[Weapon Profile]", 1)[-1], re.M)
            chars = re.split(r"\r?\n(?=\[Character Profile\])", t)[1:]
            speeds = [float(m) for c in chars if not re.search(r"^Name=Player", c, re.M)
                      for m in re.findall(r"^MaxSpeed=([-\d.]+)", c, re.M)]
            kind = "tracking" if auto and auto[1] == "FullyAuto" else "static" if speeds and max(speeds) == 0 \
                else "dynamic"
        limit = re.search(r"^Timelimit=([\d.]+)", t, re.M)
        out[os.path.basename(p)[:-4].lower()] = (kind, float(limit[1]) if limit else None)
    return out


class AreaWatch:
    """Excluded areas that only sometimes show (a "Last kill" pop-up) are excluded only while they show; the user's
    request, 2026-10-02. Every other frame, each area's stand-out pattern is kept, small: the pixels that differ from
    their neighbours, as text and boxes do and a plain wall does not. After the run, an area is a pop-up when it is
    off for 30% of the run or more, comes and goes 3 times or more (the results screen covering it once at the end
    is not), and looks the same whenever it is on (its on frames match their common pattern, its off frames do not);
    it is then excluded in its on frames and 4 frames either side. Any other area (the
    session box, a webcam) is excluded all the time, as before. An area of the challenge's end screen (kind
    challenge_results) shows once or twice, at the end or between runs: it is excluded only while it shows (excluded
    all the time, it hid the whole of VT FlyTS: 0 of 5 kills). src/popup.rs does the same."""

    STEP = 2                                             # frames between looks

    def __init__(self, boxes):
        self.boxes = [[int(round(v * s)) for v, s in zip(b[:4], (W, H, W, H))] for b in boxes]
        self.steps = [max(1, max(x1 - x0, y1 - y0) // 64) for x0, y0, x1, y1 in self.boxes]
        self.ends = [len(b) > 4 and b[4] == "challenge_results" for b in boxes]
        self.maps = [[] for _ in self.boxes]
        self.n = 0

    def add(self, rgb):
        """One frame (H x W x 3 uint8)."""
        if self.n % self.STEP == 0:
            for (x0, y0, x1, y1), k, out in zip(self.boxes, self.steps, self.maps):
                small = rgb[y0:y1:k, x0:x1:k, 1].astype(np.float32)    # every k-th pixel of the green channel: at
                if min(small.shape) < 3:                              # most about 64 a side, cheap (averaging
                    out.append(np.zeros((1, 1), bool))                # cost 60% of the review's time)
                    continue
                out.append(np.abs(small - ndimage.uniform_filter(small, 3)) > 8)
        self.n += 1

    def showing(self):
        """Per area: a bool per frame (excluded in that frame) for a pop-up, or None (excluded all the time)."""
        out = []
        for maps, end in zip(self.maps, self.ends):
            m = np.array(maps)
            share = m.reshape(len(m), -1).mean(axis=1) if len(m) else np.zeros(0)
            if len(m) < 20:
                out.append(None)
                continue
            if np.percentile(share, 98) < 0.04:          # nothing ever shows: an end screen that never came stays in
                out.append(np.zeros(self.n, bool) if end else None)
                continue
            on = share >= 0.5 * np.percentile(share, 98)
            episodes = ndimage.label(ndimage.binary_closing(on, iterations=5))[1]
            if not end and (on.mean() > 0.7 or episodes < 3):   # always on, or on once: the results screen at the
                out.append(None)                                # end covering it is not a pop-up
                continue
            tpl = m[on].mean(axis=0) > 0.5
            inter = (m & tpl).reshape(len(m), -1).sum(axis=1)
            union = (m | tpl).reshape(len(m), -1).sum(axis=1)
            match = inter / np.maximum(1, union)
            consistent = np.median(match[on]) >= 0.5 and np.median(match[~on]) < 0.2
            if not consistent and not end:
                out.append(None)
                continue
            seen = np.repeat(match >= 0.35 if consistent else on, self.STEP)[:self.n]
            out.append(ndimage.binary_dilation(seen, iterations=4))    # fading in and out
        return out


def track_model(video, detector, progress=None, batch=16, cap=None, mask=None, areas=None):
    """track() with the detector model (python/model) in place of the hand-written detector: any target colour, the
    crosshair and HUD told apart by the fixed map. detector: an infer.TorchDetector (batched, GPU) or OnnxDetector.
    cap: the scenario's target count. Each frame keeps its detections within 2 deg of the crosshair, then the most
    confident others up to the count, plus one more if it scores 0.5 or more: a theme whose wall seams look like
    targets (scoring about 0.3 to 0.36) otherwise floods the tracks. areas: the excluded areas (shares of the frame);
    one that only sometimes shows is excluded only while it shows (AreaWatch)."""
    fps, duration = probe(video)
    total = int(round(fps * duration))
    if progress:
        progress("looking", 0, total)
    fixed = fixed_map(list(_frames(video, keyframes=True))).astype(np.uint8)
    if areas is not None:
        mask = mask_of(areas)
    mask = MASK if mask is None else mask
    watch = AreaWatch(areas) if areas else None
    dets, buf, raw = [], [], []

    def keep(d, m=None):
        m = mask if m is None else m
        if m is mask:
            raw.append(d)
        row = []
        d = [b for b in d if m[min(H - 1, max(0, int(b[1]))), min(W - 1, max(0, int(b[0])))]]
        if cap and len(d) > cap:                       # targets within 2 deg of the crosshair always stay: a target
            near = [b for b in d if math.hypot(*to_deg(float(b[0]), float(b[1]))) < 2.0]   # under it scores low
            rest = sorted((b for b in d if math.hypot(*to_deg(float(b[0]), float(b[1]))) >= 2.0), key=lambda b: -b[4])
            k = max(0, cap - len(near))
            d = near + rest[:k] + [b for b in rest[k:k + 1] if b[4] >= 0.5]   # one more if confident: a new target
            #                                                                   can show while the old one dies
        for cx, cy, w, h, sc in d:
            if m[min(H - 1, max(0, int(cy))), min(W - 1, max(0, int(cx)))]:
                x0, y0 = to_deg(float(cx - w / 2), float(cy - h / 2))
                x1, y1 = to_deg(float(cx + w / 2), float(cy + h / 2))
                row.append((*to_deg(float(cx), float(cy)), int(round(math.pi / 4 * w * h)), x1 - x0, y0 - y1,
                            float(sc)))
        if m is not mask:
            return row
        dets.append(row)

    def reopen():
        """Frames where a pop-up area is off: the targets there count again (the frame's own mask)."""
        if not watch:
            return
        shows = watch.showing()
        if all(s is None for s in shows):
            return
        cache = {}
        for i, d in enumerate(raw):
            on = tuple(s is None or (i < len(s) and bool(s[i])) for s in shows)
            if all(on):
                continue
            if on not in cache:
                cache[on] = mask_of([b for b, o in zip(areas, on) if o])
            dets[i] = keep(d, cache[on])

    if getattr(detector, "dev", None) == "cuda":          # the GPU detector: batches straight from the decoder
        for b in rgb_batches(video, batch):
            if watch:
                for f in b.numpy():
                    watch.add(f)
            for d in detector.batch(b, fixed):
                keep(d)
            if progress and len(dets) % 480 < batch:
                progress("tracking", len(dets), total)
        reopen()
        if progress:
            progress("linking", len(dets), total)
        return dict(fps=fps, frames=link(dets), fixed=float(fixed.mean()), detector=type(detector).__name__)

    def flush():
        outs = detector.batch(np.stack(buf), fixed) if hasattr(detector, "batch") else [detector(f, fixed) for f in buf]
        for d in outs:
            keep(d)
        buf.clear()

    for k, f in enumerate(_prefetch(rgb_frames(video), 4 * batch)):
        if watch:
            watch.add(f)
        buf.append(f)
        if len(buf) == batch:
            flush()
        if progress and k % 120 == 0:
            progress("tracking", k, total)
    if buf:
        flush()
    reopen()
    if progress:
        progress("linking", len(dets), total)
    return dict(fps=fps, frames=link(dets), fixed=float(fixed.mean()), detector=type(detector).__name__)


# ---- 2. match -------------------------------------------------------------------------------------------------------
def load_stats(path):
    lines = open(path, encoding="utf-8", errors="replace").read().splitlines()
    meta = {}
    for l in lines:
        if ":," in l:
            k, v = l.split(":,", 1)
            meta[k] = v
    # the kill rows are the first table, up to its blank line (later tables, such as the weapon summary, can also
    # start with a digit: a weapon named "16ML9 - 080")
    rows = []
    for l in lines[1:]:
        if not l.strip():
            break
        if l[:1].isdigit():
            rows.append(l.split(","))
    return meta, rows


def match(tracks, stats_path, window=0.25):
    """The flicks, anchored on the stats file's kill times (independent ground truth for when each kill happened)."""
    meta, rows = load_stats(stats_path)
    t0 = datetime.strptime(meta["Challenge Start"], "%H:%M:%S.%f")
    kt = [(datetime.strptime(r[1], "%H:%M:%S.%f") - t0).total_seconds() for r in rows]
    return match_times(tracks, kt, [int(r[5]) for r in rows], window)


def match_times(tracks, kt, shots, window=0.25, off=None):
    """The flicks for known kill times kt (seconds) with the shots each kill took: from the stats file, or from the
    session HUD (hud.py, already on the video's clock: off=0).

    1. The video's clock against the kill times' clock: one constant offset, voted from tracks that end at the
       crosshair (unless off is given).
    2. For each kill, the killed target is the track nearest the crosshair in the last `window` seconds before the
       kill: a target held on the crosshair (hold-fire runs) or hidden by it can be lost a few frames early, so the
       track need not end exactly at the kill.
    3. The flick runs from the previous kill, or from the target's first appearance when it appeared after that kill
       (one target at a time), to this kill. Returns (flicks, info)."""
    fps, frames = tracks["fps"], tracks["frames"]
    pos, area = {}, {}
    for f in frames:
        for (tid, x, y), a in zip(f["t"], f.get("a") or [0] * len(f["t"])):
            pos.setdefault(tid, {})[f["i"]] = (x, y)
            area.setdefault(tid, []).append(a)
    ends = sorted((max(p), tid) for tid, p in pos.items()
                  if max(p) < len(frames) - 2 and math.hypot(*p[max(p)]) < 0.6 and len(p) >= 3)
    vt = np.array([e[0] / fps for e in ends])
    if not kt or (off is None and not len(vt)):
        return [], dict(kills_video=len(ends), kills_stats=len(kt), matched=0, offset=None, fps=fps)
    before = {v: k for k, v in appearances(tracks)[1].items()}
    spots = crosshair_spots(frames)
    on_spot = {tid for tid, p in pos.items()
               if spots and all(min(math.hypot(x - a, y - b) for a, b in spots) < 0.1 for x, y in p.values())}
    if off is not None:
        return _attach_kills(pos, area, kt, shots, off, window, fps, ends, before, on_spot)
    best = None
    for a in vt[:40]:
        for b in kt[:40]:
            off = a - b
            m = sum(1 for t in kt if np.min(np.abs(t + off - vt)) < 2.5 / fps)
            if best is None or m > best[0]:
                best = (m, off)
    off = best[1]
    res = [float(vt[np.argmin(np.abs(t + off - vt))] - (t + off)) for t in kt]
    res = [r for r in res if abs(r) < 2.5 / fps]
    if res:
        off += float(np.median(res))                 # refine on every kill that lines up
    return _attach_kills(pos, area, kt, shots, off, window, fps, ends, before, on_spot)


def _attach_kills(pos, area, kt, shots, off, window, fps, ends, before, on_spot=frozenset()):
    """Each kill's target and flick (steps 2 and 3 of match_times). A target lost for a few frames (under the
    crosshair, behind a hit effect) and picked up again as a new track keeps its earlier tracks (`before`). A track
    that never leaves a spot where the detector marks the crosshair (`on_spot`, crosshair_spots()) is the killed target
    only when no other track is near: it sits at the crosshair, so it always looked nearest, and its flick, a few
    frames long, could not be measured."""
    W = max(1, int(round(window * fps)))
    flicks, prev, used = [], None, set()
    for k, (t, n_shots) in enumerate(zip(kt, shots)):
        kf = int(round((t + off) * fps))
        best_tid, best_cost = None, None
        for tid, p in pos.items():
            seen = [i for i in range(kf - W, kf + 3) if i in p]
            if not seen:
                continue
            # its frame nearest the crosshair and the kill: a track can go on past the kill and move off (the tracker
            # picked up the next target), so the frame 2 after it can be far (a missed kill on the Aim Lab upload)
            last = min(seen, key=lambda i: math.hypot(*p[i]) + 4.0 * abs(i - kf) / fps)
            d = math.hypot(*p[last])
            # a big target can be hit at its rim, its center farther off: up to its blob's radius and 0.25 deg more
            r = math.degrees(math.sqrt(area[tid][list(p).index(last)] / math.pi) / K)
            if d > max(1.5, r + 0.25):
                continue
            cost = d + 4.0 * abs(min(last, kf) - kf) / fps + (0.0 if last >= kf - 2 else 0.2) + (1.0 if tid in on_spot else 0)
            if best_cost is None or cost < best_cost:
                best_tid, best_cost = tid, cost
        if best_tid is None:
            # held hidden under the crosshair longer than the window (hold-fire, a tiny target under a red dot): the
            # latest track that ended at the crosshair since the previous kill, up to 1 s back, not taken by a kill
            back = [(max(p), tid) for tid, p in pos.items() if tid not in used
                    and max(prev if prev is not None else -1, kf - 4 * W) < max(p) < kf - W
                    and math.hypot(*p[max(p)]) < 0.6]
            if back:
                last, best_tid = max(back)
        if best_tid is None:
            prev = kf
            continue
        used.add(best_tid)
        chain = [best_tid]
        while chain[-1] in before:
            chain.append(before[chain[-1]])
        p = {i: q for c in chain for i, q in pos[c].items()}
        first = min(p)
        start = prev if prev is not None else int(round(off * fps))
        spawned = first > start + 2
        if spawned:
            start = first
        end = max(i for i in p if i <= kf + 2 and (i <= last or math.hypot(*p[i]) <= 1.5))
        traj = [(i, *p[i]) for i in range(start, end + 1) if i in p]
        a = [x for c in chain for x in area[c]]     # the whole chain: the last track may be half under the crosshair
        flicks.append(dict(n=k + 1, kill_frame=end, stats_frame=kf, start_frame=start, shots=n_shots, traj=traj,
                           spawned=spawned, area=st.median(a) if any(a) else None))
        prev = kf
    # confirmed: the target was last seen within 0.6 deg of the crosshair, within 2 frames of the stats kill time
    confirmed = sum(1 for f in flicks if f["traj"] and abs(f["traj"][-1][0] - f["stats_frame"]) <= 2
                    and math.hypot(f["traj"][-1][1], f["traj"][-1][2]) < 0.6)
    info = dict(kills_video=len(ends), kills_stats=len(kt), matched=len(flicks), confirmed=confirmed, offset=off, fps=fps)
    return flicks, info


def _own_speed(p, e, cum):
    """A target's speed (deg a frame) where its points p end, at frame e, over its last 3 frames: over the world (the
    camera's turn taken out) and on screen. (0, 0) for both with fewer than 2 points in those frames."""
    w = [i for i in p if e - 3 <= i <= e]
    if len(w) < 2:
        return (0.0, 0.0), (0.0, 0.0)
    i0 = min(w)
    (x0, y0), (x1, y1), k = p[i0], p[e], e - i0
    return ((((x1 - cum[e][0]) - (x0 - cum[i0][0])) / k, ((y1 - cum[e][1]) - (y0 - cum[i0][1])) / k),
            ((x1 - x0) / k, (y1 - y0) / k))


def appearances(tracks, gap=0.5, radius=1.0):
    """Tracks that are one target picked up again. The tracker can lose a target for a few frames (under the crosshair,
    behind a hit effect) and start a new track for it. A track that starts within `gap` seconds of another's end,
    within `radius` deg of where that one would be now (its last place moved by the camera's turn since), continues
    it. Then a track still left alone continues one that ended up to 3 frames before it starts (or up to 2 after),
    within `radius` deg of where that one would be by its own speed over its last 3 frames, over the world or on screen
    (with the tracks it continues when it has 3 points or fewer): a target that moves on its own (Bounce 180's spheres)
    and fools the camera's turn gets a new track every frame or two. Not on a crosshair spot (crosshair_spots(), within
    0.2 deg), where the detector marks the crosshair every frame. Returns (appeared, follows): appeared[tid] = the frame
    the target first appeared on, follows[tid] = the track that continues tid."""
    fr, fps = tracks["frames"], tracks["fps"]
    cum = np.cumsum([[0.0, 0.0]] + [f.get("shift") or [0.0, 0.0] for f in fr[1:]], axis=0)
    first, last, pos = {}, {}, {}
    for f in fr:
        for tid, x, y in f["t"]:
            if tid not in first:
                first[tid] = (f["i"], x, y)
            last[tid] = (f["i"], x, y)
            pos.setdefault(tid, {})[f["i"]] = (x, y)
    g = max(1, int(round(gap * fps)))
    ending = collections.defaultdict(list)
    for tid, (e, x, y) in last.items():
        ending[e].append(tid)
    starts = sorted(first, key=lambda t: first[t][0])
    follows = {}
    for tid in starts:
        s0, qx, qy = first[tid]
        best = None
        for e in range(max(0, s0 - g), s0 + 3):         # up to 2 frames of overlap: a hit target flashes and is
            for prev in ending.get(e, ()):              # picked up again while its old track still has a frame or two
                if prev in follows or prev == tid or first[prev][0] >= s0:
                    continue
                _, x, y = last[prev]
                d = math.hypot(qx - (x + cum[s0][0] - cum[e][0]), qy - (y + cum[s0][1] - cum[e][1]))
                if d < radius and (best is None or d < best[0]):
                    best = (d, prev)
        if best:
            follows[best[1]] = tid
    before = {v: k for k, v in follows.items()}
    spots = crosshair_spots(fr)
    on_spot = lambda x, y: any(math.hypot(x - a, y - b) < 0.2 for a, b in spots)
    for tid in starts:
        s0, qx, qy = first[tid]
        if tid in before or on_spot(qx, qy):
            continue
        best = None
        for e in range(max(0, s0 - 3), s0 + 3):
            for prev in ending.get(e, ()):
                if prev in follows or prev == tid or first[prev][0] >= s0:
                    continue
                _, x, y = last[prev]
                if on_spot(x, y):
                    continue
                p, c = pos[prev], prev
                if len(p) <= 3 and c in before:         # a short piece: its speed with the tracks it continues
                    p = dict(p)
                    while c in before and len(p) <= 3:
                        c = before[c]
                        for i, q in pos[c].items():
                            p.setdefault(i, q)
                (vx, vy), (sx, sy) = _own_speed(p, e, cum)
                n = s0 - e
                for px, py in ((x + cum[s0][0] - cum[e][0] + vx * n, y + cum[s0][1] - cum[e][1] + vy * n),
                               (x + sx * n, y + sy * n)):
                    d = math.hypot(qx - px, qy - py)
                    if d < radius and (best is None or d < best[0]):
                        best = (d, prev)
        if best:
            follows[best[1]] = tid
            before[tid] = best[1]
    appeared = {}
    for tid in starts:
        appeared[tid] = appeared[before[tid]] if tid in before else first[tid][0]
    return appeared, follows


def crosshair_spots(frames, near=0.5, step=0.01):
    """Fixed screen spots where the detector marks the crosshair: detections near the crosshair while the camera turns
    (no target stays put on screen then) that pile up on one point, at least 2% of the turning frames within 0.015 deg
    and most of what lies within 0.1 deg of it. Returns [(x, y) deg], at most 3."""
    turning = [f for f in frames if math.hypot(*(f.get("shift") or (0.0, 0.0))) > 0.1]
    P = np.array([(x, y) for f in turning for _, x, y in f["t"] if math.hypot(x, y) < near]).reshape(-1, 2)
    out = []
    if len(P) < 30:
        return out
    n = int(round(2 * near / step))
    H = np.histogram2d(P[:, 0], P[:, 1], bins=n, range=[[-near, near], [-near, near]])[0]
    for _ in range(3):
        B = sum(np.roll(np.roll(H, a, 0), b, 1) for a in (-1, 0, 1) for b in (-1, 0, 1))
        i, j = np.unravel_index(np.argmax(B), B.shape)
        if B[i, j] < max(25, 0.02 * len(turning)):
            break
        c = np.array([-near + (i + 0.5) * step, -near + (j + 0.5) * step])
        c = P[np.hypot(*(P - c).T) < 0.02].mean(axis=0)
        if np.sum(np.hypot(*(P - c).T) < 0.02) < 0.6 * np.sum(np.hypot(*(P - c).T) < 0.1):
            break                                       # spread out: targets held near the crosshair, not a fixed spot
        out.append((float(c[0]), float(c[1])))
        g = -near + (np.arange(n) + 0.5) * step
        H[np.hypot(g[:, None] - c[0], g[None, :] - c[1]) < 0.06] = 0
    return out


def ghosts(frames, maxd=0.5, share=0.5, moved=0.3):
    """Tracks that are the crosshair, not a target: they never leave `maxd` deg of the crosshair, the camera turned
    more than `moved` deg meanwhile, and they did not move with it (a static target shifts by the camera's turn; more
    than `share` of the turn left unexplained). A detector can take some crosshairs (Aim Lab's) for a target. Also
    tracks of 3 frames or fewer on a crosshair spot (crosshair_spots()): right after a kill they made the dead target
    look picked up again, so its kill was lost or came late."""
    cum = np.cumsum([[0.0, 0.0]] + [f.get("shift") or [0.0, 0.0] for f in frames[1:]], axis=0)
    pos = {}
    for f in frames:
        for tid, x, y in f["t"]:
            pos.setdefault(tid, {})[f["i"]] = (x, y)
    out = set()
    for tid, p in pos.items():
        ks = sorted(p)
        if len(ks) < 2 or max(math.hypot(*p[i]) for i in ks) >= maxd:
            continue
        turn = left = 0.0
        for i, j in zip(ks, ks[1:]):
            dc = cum[j] - cum[i]
            turn += math.hypot(*dc)
            left += math.hypot(p[j][0] - p[i][0] - dc[0], p[j][1] - p[i][1] - dc[1])
        if turn > moved and left > share * turn:
            out.add(tid)
    spots = crosshair_spots(frames)
    for tid, p in pos.items():
        if len(p) <= 3 and spots and all(min(math.hypot(x - a, y - b) for a, b in spots) < 0.1 for x, y in p.values()):
            out.add(tid)
    return out


def without_ghosts(tracks):
    """The tracks without those that are the crosshair (ghosts())."""
    g = ghosts(tracks["frames"])
    if not g:
        return tracks
    keep = lambda f: [j for j, q in enumerate(f["t"]) if q[0] not in g]
    return dict(tracks, frames=[dict(f, t=[f["t"][j] for j in keep(f)],
                                     **({"a": [f["a"][j] for j in keep(f)]} if f.get("a") else {}))
                                for f in tracks["frames"]])


def match_video(tracks):
    """The flicks from the video alone, for a run without a stats file. A kill is a track that ends near the crosshair,
    unless:
      - another track continues it (appearances(): tracking lost the target; it did not die);
      - it is the crosshair (ghosts());
      - three or more steady tracks (5 frames or more, not continued) end within a frame of it (the run ended or
        restarted); flickering false detections, such as a game's HUD text, do not count;
      - another kill was found within 3 frames (the same kill twice).
    A flick's path joins the pieces of its target's track.
    "Near" is the target's radius (from the tracks' median blob area) plus 0.25 deg, and at least 0.6 deg. Flicks are
    built as in match(), with no shot counts. Returns (flicks, info)."""
    tracks = without_ghosts(tracks)
    fps, frames = tracks["fps"], tracks["frames"]
    pos, area = {}, {}
    for f in frames:
        for (tid, x, y), a in zip(f["t"], f.get("a") or [0] * len(f["t"])):
            pos.setdefault(tid, {})[f["i"]] = (x, y)
            if a and math.hypot(x, y) < 3:
                area.setdefault(tid, []).append(a)
    areas = [st.median(v) for v in area.values() if v]
    R = math.degrees(math.sqrt(st.median(areas) / math.pi) / K) if len(areas) >= 5 else 0.43
    near = max(0.6, R + 0.25)
    appeared, follows = appearances(tracks)
    before = {v: k for k, v in follows.items()}
    ends_at = collections.Counter(max(p) for t, p in pos.items() if len(p) >= 5 and t not in follows)
    cands = []
    for tid, p in pos.items():
        e = max(p)
        if tid in follows or e >= len(frames) - 2 or math.hypot(*p[e]) >= near:
            continue
        if ends_at[e - 1] + ends_at[e] + ends_at[e + 1] >= 3:
            continue
        chain = [tid]
        while chain[-1] in before:
            chain.append(before[chain[-1]])
        if sum(len(pos[c]) for c in chain) < 3:
            continue
        cands.append((e, math.hypot(*p[e]), tid, chain))
    cands.sort()
    kills = []
    for c in cands:
        if kills and c[0] - kills[-1][0] <= 3:
            if c[1] < kills[-1][1]:
                kills[-1] = c
            continue
        kills.append(c)
    flicks, prev = [], None
    for k, (end, _, tid, chain) in enumerate(kills):
        p = {i: q for c in chain for i, q in pos[c].items()}
        first = min(p)
        start = prev if prev is not None else first
        spawned = first > start + 2
        if spawned:
            start = first
        traj = [(i, *p[i]) for i in range(start, end + 1) if i in p]
        flicks.append(dict(n=k + 1, kill_frame=end, stats_frame=None, start_frame=start, shots=None, traj=traj,
                           spawned=spawned, area=st.median(area[tid]) if area.get(tid) else None))
        prev = end
    info = dict(source="video", kills_video=len(kills), kills_stats=None, matched=len(flicks), confirmed=None,
                offset=None, fps=fps)
    return flicks, info


def target_radius(flicks, default=0.43):
    """The targets' radius in deg, from their median blob area near the crosshair (0.43 for 1w4ts Voltaic)."""
    a = [f["area"] for f in flicks if f.get("area")]
    if len(a) < 5:
        return default
    return math.degrees(math.sqrt(st.median(a) / math.pi) / K)


# ---- 3. measure -----------------------------------------------------------------------------------------------------
def _arrival(fr, d, R):
    """Where the crosshair reaches the target: the first point of its path inside the target's circle (radius R), the
    path straight between two frames up to 2 frames apart. A fast flick can pass through the target between them.
    Across a longer gap the target was not seen, and the first frame inside the circle counts. Returns (arr, at): arr
    the first frame at or after it (an index into the path), at its time in frames from the path's first frame (a
    fraction of a frame between two); (None, None) when the path never reaches it."""
    if math.hypot(*d[0]) < R:
        return 0, 0.0
    for i in range(1, len(d)):
        (ax, ay), (bx, by) = d[i - 1], d[i]
        t = None
        if fr[i] - fr[i - 1] <= 2:
            ex, ey = bx - ax, by - ay
            a = ex * ex + ey * ey
            b = 2 * (ax * ex + ay * ey)
            c = ax * ax + ay * ay - R * R
            disc = b * b - 4 * a * c
            if a > 0 and disc > 0:
                t = (-b - math.sqrt(disc)) / (2 * a)
        if math.hypot(bx, by) < R or (t is not None and 0 <= t <= 1):
            t = 1.0 if t is None else min(max(t, 0.0), 1.0)
            return i, fr[i - 1] - fr[0] + t * (fr[i] - fr[i - 1])
    return None, None


def measure(flicks, fps, R):
    """Per-flick measures (the keys measure.py has always written, plus settle, still and the time parts)."""
    out = []
    for f in flicks:
        tr = f["traj"]
        if len(tr) < 4 or tr[0][0] > f["start_frame"] + 2:
            continue
        fr = [p[0] for p in tr]
        d = [(p[1], p[2]) for p in tr]
        D0 = math.hypot(*d[0])
        u = (d[0][0] / D0, d[0][1] / D0) if D0 > 1e-6 else (1.0, 0.0)
        along = [x * u[0] + y * u[1] for x, y in d]
        sp = [0.0] + [math.dist(d[i], d[i - 1]) * fps / max(1, fr[i] - fr[i - 1]) for i in range(1, len(d))]
        toward = [0.0] + [(along[i - 1] - along[i]) * fps / max(1, fr[i] - fr[i - 1]) for i in range(1, len(d))]
        # reaction: the first frame from which the crosshair closes on the target at 30 deg/s or more for 2 frames
        mv = next((i for i in range(1, len(d) - 1) if toward[i] > 30 and toward[i + 1] > 30), None)
        peak_i = max(range(len(sp)), key=lambda i: sp[i])
        # the main flick ends at the first frame after the peak where the speed falls below 15% of the peak
        end_i = next((i for i in range(peak_i, len(sp)) if sp[i] < 0.15 * sp[peak_i]), len(sp) - 1)
        arr, at = _arrival(fr, d, R)
        past = -min(along[(mv or 0):])
        # corrections: separate bursts of movement after the main flick (speed above 8 deg/s again)
        bursts, moving = 0, False
        for i in range(end_i + 1, len(sp)):
            if sp[i] > 8 and not moving:
                bursts, moving = bursts + 1, True
            elif sp[i] < 4:
                moving = False
        k = len(d) - 1
        # holding: after the first contact, how often the crosshair slipped off the target and for how long (with a
        # little hysteresis, so a crosshair sitting on the edge does not count as slipping off every frame). From the
        # arrival's frame: one that passed through the target before it is off it there
        breaks, off, inside = 0, 0.0, True
        for i in range(max(arr, 1) if arr is not None else k + 1, k + 1):
            r = math.hypot(*d[i])
            if inside and r > 1.15 * R:
                breaks, inside = breaks + 1, False
            elif not inside and r < R:
                inside = True
            if not inside:
                off += (fr[i] - fr[i - 1]) / fps
        # settling and the still time: speeds over 3 frames, since the capture moves in uneven steps
        ds = [d[0]] + [((d[i - 1][0] + d[i][0] + d[i + 1][0]) / 3, (d[i - 1][1] + d[i][1] + d[i + 1][1]) / 3)
                       for i in range(1, len(d) - 1)] + [d[-1]]
        sps = [0.0] + [math.dist(ds[i], ds[i - 1]) * fps for i in range(1, len(ds))]
        settle = next((i for i in range(arr, k + 1) if all(v < 10 for v in sps[i:k])), k) if arr is not None else None
        total = (fr[k] - fr[0]) / fps
        m = dict(n=f["n"], shots=f["shots"], D0=D0, dir=math.degrees(math.atan2(u[1], u[0])), total=total,
                 react=(fr[mv] - fr[0]) / fps if mv is not None else None,
                 flick=(fr[end_i] - fr[mv]) / fps if mv is not None and end_i > mv else None,
                 peak=sp[peak_i], end_left=along[end_i], end_off=math.hypot(*d[end_i]),
                 arrive=at / fps if arr is not None else None,
                 dwell=(fr[k] - fr[0] - at) / fps if arr is not None else None,
                 past=past, corr=bursts, click_speed=sp[k], click_off=math.hypot(*d[k]), click_off_xy=d[k],
                 settle=(fr[settle] - fr[0] - at) / fps if settle is not None else None,
                 still=(fr[k] - fr[settle]) / fps if settle is not None else None,
                 start_frame=f["start_frame"], kill_frame=f["kill_frame"], spawned=f.get("spawned", False),
                 hold=(fr[k] - fr[0] - at) / fps if arr is not None else None, breaks=breaks, off=off)
        if m["react"] is not None and m["flick"] is not None and m["arrive"] is not None and settle is not None:
            b = sorted([m["react"], min(m["react"] + m["flick"], m["arrive"]), m["arrive"], (fr[settle] - fr[0]) / fps])
            b = [0.0] + [min(x, total) for x in b] + [total]
            m["parts"] = [b[i + 1] - b[i] for i in range(5)]       # react, main flick, onto the target, settle, still
        out.append(m)
    return out


def choices(tracks, flicks):
    """For each kill after the first: was the next target the nearest one on screen (rank 0), and how much farther."""
    byf = {f["i"]: f["t"] for f in tracks["frames"]}
    out = []
    for a, b in zip(flicks, flicks[1:]):
        kf = a["kill_frame"]
        ts = byf.get(kf + 3, [])
        tb = [p for p in b["traj"] if p[0] >= kf + 3]
        if not ts or not tb:
            continue
        dists = sorted(math.hypot(x, y) for _, x, y in ts)
        chosen = math.hypot(tb[0][1], tb[0][2])
        out.append(dict(n=b["n"], rank=sum(1 for d in dists if d < chosen - 0.3), extra=chosen - dists[0]))
    return out


# ---- 4. summarize and judge -----------------------------------------------------------------------------------------
def _q(v, p):
    v = sorted(x for x in v if x is not None)
    return v[int(p * (len(v) - 1))] if v else None


def _med(v):
    v = [x for x in v if x is not None]
    return st.median(v) if v else None


def summarize(ms, ch, meta, info, R, mode="click"):
    n = len(ms)
    video_only = info.get("source") == "video"           # no stats file: kills found in the video, no shots or score
    kills = info["matched"] if video_only else int(meta.get("Kills", 0) or 0)
    shots_total = None if video_only else sum(int(m["shots"]) for m in ms)
    s = dict(
        scenario=meta.get("Scenario"), score=None if video_only or meta.get("Score") is None else float(meta["Score"] or 0), kills=kills,
        misses=int(meta.get("Miss Count", 0) or 0) if "Miss Count" in meta else None,
        fps_avg=float(meta["Avg FPS"]) if meta.get("Avg FPS") else None,
        sens=f"{meta.get('Horiz Sens')} {meta.get('Sens Scale')}" if meta.get("Horiz Sens") else None,
        fov=meta.get("FOV"), radius=R, measured=n, info=info,
        median_interval=_med([m["total"] for m in ms]),
        spread=(st.pstdev([m["total"] for m in ms]) / st.mean([m["total"] for m in ms])) if n > 2 else None,
        react=_med([m["react"] for m in ms]), flick=_med([m["flick"] for m in ms]),
        peak=_med([m["peak"] for m in ms]), arrive=_med([m["arrive"] for m in ms]),
        still=_med([m["still"] for m in ms]), click_speed=_med([m["click_speed"] for m in ms]),
        click_off=_med([m["click_off"] for m in ms]),
        ended_short=sum(1 for m in ms if m["end_left"] > R) / n if n else None,
        ended_past=sum(1 for m in ms if m["end_left"] < -R) / n if n else None,
        crossed_past=sum(1 for m in ms if m["past"] > R) / n if n else None,
        moving_clicks=sum(1 for m in ms if m["click_speed"] > 20) / n if n else None,
        shots=shots_total, mode=mode,
        accuracy=float(meta["Hit Count"]) / (float(meta["Hit Count"]) + float(meta["Miss Count"]))
        if meta.get("Hit Count") and meta.get("Miss Count") and float(meta["Hit Count"]) + float(meta["Miss Count"]) > 0
        else None,
    )
    held = [m for m in ms if m.get("hold")]
    if held:
        s["hold"] = _med([m["hold"] for m in held])
        s["slipped"] = sum(1 for m in held if m["breaks"]) / len(held)
        s["off_share"] = sum(m["off"] for m in held) / max(1e-9, sum(m["hold"] for m in held))
    on = [m for m in ms if m["end_left"] <= R and m["still"] is not None]
    sh = [m for m in ms if m["end_left"] > R and m["still"] is not None]
    s["still_landed"], s["still_corrected"] = _med([m["still"] for m in on]), _med([m["still"] for m in sh])
    short = [m["end_left"] / m["D0"] for m in ms if m["end_left"] > R and m["D0"] > 2]
    s["short_covered"] = 1 - st.median(short) if short else None
    mid = [m for m in ms if 10 <= m["D0"] < 25]
    s["mid_short_cost"] = (_med([m["total"] for m in mid if m["end_left"] > R]) or 0) - (
        _med([m["total"] for m in mid if m["end_left"] <= R]) or 0) if mid else None
    parts = [m["parts"] for m in ms if "parts" in m]
    s["budget"] = [sum(p[i] for p in parts) / len(parts) for i in range(5)] if parts else None
    s["by_distance"] = []
    for lo, hi in ((0, 5), (5, 10), (10, 15), (15, 25), (25, 90)):
        g = [m for m in ms if lo <= m["D0"] < hi]
        if g:
            s["by_distance"].append(dict(lo=lo, hi=hi, n=len(g), interval=_med([m["total"] for m in g]),
                                         react=_med([m["react"] for m in g]),
                                         short=sum(1 for m in g if m["end_left"] > R) / len(g),
                                         past=sum(1 for m in g if m["end_left"] < -R) / len(g),
                                         still=_med([m["still"] for m in g])))
    # eight 45-degree sectors, each centred on its direction (0 = right, 90 = up). beyond: the median time a kill took
    # beyond what its distance predicts, from Fitts' law fitted to the whole run (t = a + b log2(1 + D / W), W = 2R)
    fit = _fitts(ms, 2 * R)
    s["by_direction"] = []
    for k, name in enumerate(DIRECTIONS):
        g = [m for m in ms if round((m["dir"] % 360) / 45) % 8 == k]
        if g:
            s["by_direction"].append(dict(name=name, n=len(g), interval=_med([m["total"] for m in g]),
                                          distance=_med([m["D0"] for m in g]),
                                          short=sum(1 for m in g if m["end_left"] > R) / len(g),
                                          past=sum(1 for m in g if m["end_left"] < -R) / len(g),
                                          beyond=_med([m["total"] - fit(m["D0"]) for m in g
                                                       if m["total"] is not None and m["D0"] is not None]) if fit else None))
    s["nearest_chosen"] = sum(1 for c in ch if c["rank"] == 0) / len(ch) if ch else None
    s["extra_when_not_nearest"] = _med([c["extra"] for c in ch if c["rank"] > 0])
    # pacing: kill intervals in the first and the last third of the run
    if n >= 30:
        third = n // 3
        s["pace_first"], s["pace_last"] = _med([m["total"] for m in ms[:third]]), _med([m["total"] for m in ms[-third:]])
    return s


DIRECTIONS = ("right", "up-right", "up", "up-left", "left", "down-left", "down", "down-right")


CAM_STEP, CAM_T = 0.15, 80                     # camera_motion's grid step (deg) and tile size (grid cells)


@functools.lru_cache(maxsize=1)
def _cam_grid():
    """The angular grid camera_motion resamples frames onto (az -36 to 36 deg, el 18 to -18, CAM_STEP apart; the
    review's own degrees, so a turn of the camera is a plain shift on it), as bilinear gather indices and weights,
    and each tile's centre."""
    T = CAM_T
    az, el = np.meshgrid((np.arange(6 * T) - 3 * T + 0.5) * CAM_STEP, (1.5 * T - np.arange(3 * T) - 0.5) * CAM_STEP)
    px = CX + K * np.tan(np.radians(az))
    py = CY - np.tan(np.radians(el)) * np.hypot(K, px - CX)
    x0, y0 = np.floor(px).astype(int), np.floor(py).astype(int)
    fx, fy = (px - x0).astype(np.float32), (py - y0).astype(np.float32)
    i00 = np.clip(y0, 0, H - 2) * W + np.clip(x0, 0, W - 2)
    taz = np.array([az[0, (k % 6) * T:(k % 6 + 1) * T].mean() for k in range(18)])
    tel = np.array([el[(k // 6) * T:(k // 6 + 1) * T, 0].mean() for k in range(18)])
    hann = np.outer(np.hanning(T), np.hanning(T)).astype(np.float32)
    return i00, fx, fy, taz, tel, hann


def _cam_tiles(img):
    i00, fx, fy, _, _, _ = _cam_grid()
    f = img.ravel()
    g = (f[i00] * (1 - fx) + f[i00 + 1] * fx) * (1 - fy) + (f[i00 + W] * (1 - fx) + f[i00 + W + 1] * fx) * fy
    return g.reshape(3, CAM_T, 6, CAM_T).transpose(0, 2, 1, 3).reshape(18, CAM_T, CAM_T)


def camera_motion(video, frames, mask=None, fixed=None, progress=None, thr=0.08):
    """How the camera turned, from the video alone (no mouse log): the room slides across the screen as the view
    turns. Each frame is resampled onto an angular grid round the crosshair (az -36 to 36 deg, el -18 to 18), cut into
    18 tiles of 12 deg, and each tile is matched with the frame before by phase correlation. A tile is left out while
    a tracked target is in it, when a tenth of it is HUD, excluded area or the fixed map, or when it has too little
    texture (peak under thr). The frame's reading is the mean of the tiles that agree with their median within
    0.1 deg, if 3 or more do. Returns per frame (dx, dy, tiles) in deg, the room's move on screen since the frame
    before (a static target's move; the camera turned by minus that), or None.
    Checked on static runs, where link()'s shift (the static targets' common move) is the truth: median error 0.013 to
    0.042 deg, 90th percentile 0.05 to 0.11; no reading in 2% (tiled walls) to 22% (plain white walls) of frames."""
    from scipy import fft as sfft
    _, _, _, taz, tel, hann = _cam_grid()
    bad = ~(MASK if mask is None else mask)
    if fixed is not None:
        bad = bad | ndimage.binary_dilation(fixed.astype(bool), iterations=3)
    static_ok = _cam_tiles(bad.astype(np.float32)).reshape(18, -1).mean(axis=1) < 0.1
    T = CAM_T
    p = subprocess.Popen(["ffmpeg", "-v", "error", "-i", video, "-vf", f"scale={W}:{H}:flags=area,format=gray",
                          "-f", "rawvideo", "-"], stdout=subprocess.PIPE, bufsize=0)
    out, prev = [], None

    def sub(a, b, m):
        d = a - 2 * m + b
        return 0.0 if d >= 0 else 0.5 * (a - b) / d

    for i, buf in enumerate(_read(p, W * H)):
        t = _cam_tiles(np.frombuffer(buf, np.uint8).astype(np.float32))
        F = sfft.rfft2((t - t.mean(axis=(1, 2), keepdims=True)) * hann, workers=4)
        if progress and i % 600 == 0:
            progress("camera", i, len(frames))
        if prev is None:
            out.append(None)
            prev = F
            continue
        ok = static_ok.copy()
        for f in (frames[i - 1], frames[i]) if i < len(frames) else ():
            for (_, x, y), a in zip(f["t"], f["a"]):
                r = math.degrees(math.atan(math.sqrt(a / math.pi) / K)) + 1.5
                ok &= ~((np.abs(taz - x) < 6 + r) & (np.abs(tel - y) < 6 + r))
        R = F * np.conj(prev)
        R /= np.maximum(np.abs(R), 1e-6)
        c = sfft.irfft2(R, s=(T, T), workers=4)
        prev = F
        sh = []
        for k in np.nonzero(ok)[0]:
            ck = c[k]
            py, px = divmod(int(np.argmax(ck)), T)
            pk = ck[py, px]
            if pk < thr:
                continue
            dx = px + sub(ck[py, (px - 1) % T], ck[py, (px + 1) % T], pk)
            dy = py + sub(ck[(py - 1) % T, px], ck[(py + 1) % T, px], pk)
            sh.append(((dx - T if dx > T / 2 else dx) * CAM_STEP, -(dy - T if dy > T / 2 else dy) * CAM_STEP))
        if len(sh) < 3:
            out.append(None)
            continue
        a = np.array(sh)
        m = np.median(a, axis=0)
        agree = a[np.hypot(*(a - m).T) < 0.1]
        out.append((float(agree[:, 0].mean()), float(agree[:, 1].mean()), len(agree)) if len(agree) >= 3 else None)
    return out


def _smooth(v, k):
    """Moving mean over k frames of a (n, 2) array with gaps (NaN), centred; NaN where no value is in reach."""
    ok = ~np.isnan(v[:, 0])
    w = np.ones(k)
    num = np.stack([np.convolve(np.where(ok, v[:, j], 0), w, "same") for j in range(2)], axis=1)
    den = np.convolve(ok.astype(float), w, "same")[:, None]
    return np.where(den > 0, num / np.maximum(den, 1e-9), np.nan)


def track_motion(frames, fps, cam, i0, i1, inside, switching=None):
    """Tracking diagnostics from the target's own motion and the camera's (camera_motion), frames i0 to i1. The
    target is the track nearest the crosshair, and offsets are from its centre line (a sphere's centre, a capsule's
    long axis). Its own motion is its move on screen less the room's move; the mouse's is the camera's turn. Both are smoothed over 0.05 s. While the target moves (over 5 deg/s) and the crosshair
    is engaged with it (within 2 deg of its line, or 5 radii), the crosshair's offset from it is split along the
    target's motion (positive: ahead of it) and across. The time after a bot's death until the crosshair is on a
    target again (switching) is left out.
    - lag: the median offset along the motion, in deg and in ms at the target's speed (negative: behind it);
    - off_ahead, off_behind, off_side: the shares of the time off the target spent ahead of it (past its leading
      edge), behind it, or to the side;
    - overshoots: stretches of 2 frames or more ahead of the target past its edge, per second of motion, and the median
      distance past the edge;
    - swings: over-correcting, the crosshair crossing over the target's center from behind to ahead or back by half
      the target's width or more, per second of steady motion (no direction change within 0.25 s); corrections: the
      crosshair's turns back toward the middle along the motion (its offset's turning points, beyond 0.05 deg);
      overcorrect: swings as a share of corrections;
    - reversals: the target's direction changes (horizontal or vertical; 8 deg/s or more either side, 0.1 s from the
      change). reaction: the median time until the mouse moves the new way. reversal_overshoot: the share of
      reversals after which the crosshair carried on the old way past the target's edge within 0.4 s, and the median
      distance past it;
    - by_direction: per direction of the target's motion (8 sectors), the share of the moving time, the share on the
      target, the median distance from its centre and the median offset along the motion;
    - error_h, error_v: the median horizontal and vertical distance from the target's centre (on or near it).
    Rates and medians are given only with 10 s or more of engaged motion (and a fifth of the run): seconds says how
    much there was, and reason why there are none."""
    n = len(frames)
    i1 = min(i1, n)
    tgt = [None] * n
    for i in range(i0, i1):
        f, best = frames[i], None
        for k, (tid, x, y) in enumerate(f["t"]):
            if best is None or math.hypot(x, y) < best[0]:
                if "wh" in f:
                    w, h = f["wh"][k]
                else:
                    w = h = 2 * math.degrees(math.atan(math.sqrt(f["a"][k] / math.pi) / K))
                best = (math.hypot(x, y), tid, x, y, w, h)
        if best:                                        # the offset from the centre line (a capsule's long axis)
            d_, tid, x, y, w, h = best
            lx = math.copysign(max(0.0, abs(x) - max(0.0, w - h) / 2), x)
            ly = math.copysign(max(0.0, abs(y) - max(0.0, h - w) / 2), y)
            best = (math.hypot(lx, ly), tid, x, y, w, h, lx, ly, min(w, h) / 2)
        tgt[i] = best
    own = np.full((n, 2), np.nan)
    mouse = np.full((n, 2), np.nan)
    for i in range(i0 + 1, i1):
        c = cam[i] if i < len(cam) else None
        if c is None:
            continue
        mouse[i] = (-c[0] * fps, -c[1] * fps)
        a, b = tgt[i - 1], tgt[i]
        if a and b and a[1] == b[1]:
            own[i] = ((b[2] - a[2] - c[0]) * fps, (b[3] - a[3] - c[1]) * fps)
    k = max(3, int(round(0.05 * fps)) | 1)
    own, mouse = _smooth(own, k), _smooth(mouse, k)
    speed = np.hypot(own[:, 0], own[:, 1])
    rng = range(i0, i1)
    # engaged: within 2 deg of the target's line, or 5 radii for a big one (a jump to another target, or a target
    # lost, is not tracking; Pokeball Frenzy's switches read as overshoots without this)
    sw = np.zeros(n, bool) if switching is None else switching    # after a bot's death: switching, not tracking
    moving = [i for i in rng if tgt[i] and not np.isnan(speed[i]) and speed[i] > 5 and not sw[i]
              and tgt[i][0] <= max(2.0, 5 * tgt[i][8])]
    out = dict(camera=sum(1 for i in rng if i < len(cam) and cam[i] is not None) / max(1, i1 - i0),
               target_speed=None, mouse_speed=None, lag=None, lag_ms=None, off_ahead=None, off_behind=None,
               off_side=None, overshoots=None, overshoot_dist=None, swings=None, reversals=0, reaction=None,
               reversal_overshoot=None, reversal_overshoot_dist=None, by_direction=[], error_h=None, error_v=None)
    near = [tgt[i] for i in rng if tgt[i] and tgt[i][0] <= 3 * max(tgt[i][8], 0.2)]
    if near:
        out.update(error_h=float(np.median([abs(t[6]) for t in near])), error_v=float(np.median([abs(t[7]) for t in near])))
    out["seconds"] = len(moving) / fps
    if len(moving) < fps * max(10.0, 0.2 * (i1 - i0) / fps):   # under 10 s (or a fifth of the run) of engaged motion
        out["reason"] = "too little tracking of a moving target to read"   # read: Pokeball Frenzy (several slow
        return out                                                         # targets, killed in turn) has 2 s
    al, ac, ra, rc = {}, {}, {}, {}
    for i in moving:
        lx, ly, r = tgt[i][6:9]
        ux, uy = own[i] / speed[i]
        cx, cy = -lx, -ly                               # the crosshair from the target's centre line
        al[i], ac[i] = cx * ux + cy * uy, -cx * uy + cy * ux
        ra[i] = rc[i] = r
    # each moving frame's offset along the motion (positive: ahead) and across it, and the target's radius: where the
    # crosshair sat around the target
    out["around"] = [[float(al[i]), float(ac[i]), float(ra[i])] for i in moving]
    mv = np.array(moving)
    out.update(target_speed=float(np.median(speed[mv])),
               mouse_speed=float(np.nanmedian(np.hypot(mouse[mv, 0], mouse[mv, 1]))),
               lag=float(np.median([al[i] for i in moving])),
               lag_ms=float(np.median([al[i] / speed[i] for i in moving])) * 1000)
    off = [i for i in moving if not inside[i]]
    if off:
        ahead = sum(1 for i in off if al[i] > ra[i])
        behind = sum(1 for i in off if al[i] < -ra[i])
        out.update(off_ahead=ahead / len(off), off_behind=behind / len(off), off_side=1 - (ahead + behind) / len(off))
    seconds = len(moving) / fps
    past = np.zeros(n, bool)
    past[mv] = [al[i] > ra[i] + 0.05 for i in moving]
    lab, m = ndimage.label(ndimage.binary_closing(past, iterations=1))
    eps = [sl[0] for sl in ndimage.find_objects(lab) if sl[0].stop - sl[0].start >= 2] if m else []
    out["overshoots"] = len(eps) / seconds
    if eps:
        out["overshoot_dist"] = float(np.median([max(al[i] - ra[i] for i in range(e.start, e.stop) if i in al)
                                                 for e in eps]))
    # direction changes of the target, on each axis
    d = int(round(0.1 * fps))
    revs = []
    for ax in (0, 1):
        v = own[:, ax]
        last = -10 ** 9
        for i in range(i0 + d, i1 - d):
            a, b = v[i - d], v[i + d]
            if np.isnan(a) or np.isnan(b) or a * b >= 0 or min(abs(a), abs(b)) < 8 or i - last < 2 * d:
                continue
            j = i - d + int(np.argmin(np.abs(v[i - d:i + d + 1])))    # the change: where it is slowest
            if j - last >= 2 * d and tgt[j] and tgt[j][0] <= max(2.0, 5 * tgt[j][8]) and not sw[j]:
                revs.append((j, ax, 1 if b > 0 else -1))
                last = j
    react, ov = [], []
    for j, ax, sgn in revs:
        r = next((t for t in range(j, min(i1, j + int(0.5 * fps))) if not np.isnan(mouse[t, ax])
                  and mouse[t, ax] * sgn > 3), None)
        if r is not None:
            react.append((r - j) / fps * 1000)
        o = [tgt[t][6 + ax] * sgn - tgt[t][8] for t in range(j, min(i1, j + int(0.4 * fps))) if tgt[t]]
        if o:
            ov.append(max(o))
    steady = np.zeros(n, bool)
    steady[mv] = True
    for j, _, _ in revs:
        steady[max(0, j - int(0.25 * fps)):j + int(0.25 * fps)] = False
    # corrections: each turn of the crosshair back toward the bot's middle along its motion (the offset's turning
    # points, jitter under 0.05 deg ignored); swings: the crosshair crossing over the middle to the other side by
    # half the bot's width or more. Both in steady motion only, and counted afresh after a gap.
    swings, state, corrections, zz, prev = 0, 0, 0, None, None
    for i in moving:
        if not steady[i] or (prev is not None and i - prev > 2):
            state, zz = 0, None
        prev = i
        if not steady[i]:
            continue
        hcut = max(0.05, ra[i] / 2)
        s_ = 1 if al[i] > hcut else -1 if al[i] < -hcut else 0
        if s_ and state and s_ != state:
            swings += 1
        state = s_ or state
        v = al[i]
        if zz is None:
            zz = [0, v]
        elif zz[0] == 0:
            if abs(v - zz[1]) > 0.05:
                zz = [1 if v > zz[1] else -1, v]
        elif (v - zz[1]) * zz[0] > 0:
            zz[1] = v
        elif abs(v - zz[1]) > 0.05:
            corrections += 1
            zz = [-zz[0], v]
    out.update(reversals=len(revs), reaction=float(np.median(react)) if react else None,
               reversal_overshoot=sum(1 for o in ov if o > 0.05) / len(ov) if ov else None,
               reversal_overshoot_dist=float(np.median([o for o in ov if o > 0.05])) if any(o > 0.05 for o in ov) else None,
               swings=float(swings / max(1e-9, steady.sum() / fps)), swing_count=swings, corrections=corrections,
               overcorrect=swings / corrections if corrections else None)
    # what the off-target time went on, in frames (for the review's what-if estimates)
    turn_win = np.zeros(n, bool)
    for j, _, _ in revs:
        turn_win[j:j + int(0.4 * fps)] = True
    off_mv = [i for i in moving if not inside[i]]
    dirs = {}
    for i in moving:
        k_ = round((math.degrees(math.atan2(own[i, 1], own[i, 0])) % 360) / 45) % 8
        dirs.setdefault(k_, [0, 0])
        dirs[k_][0] += 1
        dirs[k_][1] += bool(inside[i])
    big = [v for v in dirs.values() if v[0] >= 0.05 * len(moving)]
    best = max((v[1] / v[0] for v in big), default=0)
    out["frames"] = dict(ahead=sum(1 for i in off_mv if al[i] > ra[i]), behind=sum(1 for i in off_mv if al[i] < -ra[i]),
                         turns=int(sum(1 for i in range(i0, i1) if turn_win[i] and not inside[i] and not sw[i])),
                         directions=float(sum(max(0.0, best * v[0] - v[1]) for v in dirs.values())))
    for k_, name in enumerate(DIRECTIONS):
        g = [i for i in moving if round((math.degrees(math.atan2(own[i, 1], own[i, 0])) % 360) / 45) % 8 == k_]
        if g:
            out["by_direction"].append(dict(name=name, share=len(g) / len(moving), on=sum(inside[i] for i in g) / len(g),
                                            distance=float(np.median([tgt[i][0] for i in g])),   # from the line
                                            lag=float(np.median([al[i] for i in g]))))
    return out


def faint_scores(frames, near=2.0):
    """The review app's faint-target cut-off, its scores: each track's 90th-percentile score away from the crosshair
    (`near` deg; a target under the crosshair scores low), from tracks of 3 or more such frames, and the recording's
    level, the 90th percentile of those weighted by frames. Returns ({track: score}, {track: frames}, level or None).
    A track scoring more than the user's offset below the level is left out. app.js (faintScores) does the same."""
    sc = {}
    for f in frames:
        for (tid, x, y), s in zip(f["t"], f.get("s") or []):
            if math.hypot(x, y) >= near:
                sc.setdefault(tid, []).append(s)
    q, n = {}, {}
    for tid, v in sc.items():
        if len(v) >= 3:
            v = sorted(v)
            q[tid], n[tid] = v[min(len(v) - 1, int(0.9 * (len(v) - 1) + 0.5))], len(v)
    level, acc, total = None, 0, sum(n.values())
    for tid in sorted(q, key=q.get):
        acc += n[tid]
        if acc >= 0.9 * total:
            level = q[tid]
            break
    return q, n, level


def countdown_end(video, until):
    """The frame a run starts on: the one after the last frame showing KovaaK's countdown ("Challenge begins in",
    over a teal bar of colour (0, 240, 184) that shrinks as it counts down, at x 518 to 698 and y 214 to 246 at
    1280 x 720), in the first `until` seconds, or None. A recording can start before the scenario is restarted, with
    a bot already showing, so the first frame with a bot is not the start (Controlsphere: the countdown ends 2 s in)."""
    p = subprocess.Popen(["ffmpeg", "-v", "error", "-t", f"{until:.2f}", "-i", video, "-vf",
                          f"scale={W}:{H}:flags=area,crop=300:60:490:200,format=rgb24", "-f", "rawvideo", "-"],
                         stdout=subprocess.PIPE, bufsize=0)
    last = None
    for i, buf in enumerate(_read(p, 300 * 60 * 3)):
        a = np.frombuffer(buf, np.uint8).reshape(60, 300, 3).astype(np.int16)
        teal = (a[..., 0] < 60) & (a[..., 1] > 200) & (np.abs(a[..., 2] - 184) < 45)
        if teal.sum() >= 40:
            last = i
    return None if last is None else last + 1


def stats_length(stats_path):
    """A run's length in seconds from its stats file: the file is named for the moment the run ended (to the second)
    and holds its start (Challenge Start, to the millisecond), so the length is their gap, rounded up. More reliable
    than the scenario's Timelimit, which can differ from what was played (Pasu Track Smaller: 42 against 60 s)."""
    meta = load_stats(stats_path)[0]
    try:
        end = datetime.strptime(Path(stats_path).stem.split(" - ")[-1].replace(" Stats", ""), "%Y.%m.%d-%H.%M.%S")
        t0 = datetime.strptime(meta["Challenge Start"], "%H:%M:%S.%f")
    except (KeyError, ValueError):
        return None
    d = (end.replace(year=1900, month=1, day=1) - t0).total_seconds() % 86400
    return float(math.ceil(d)) if 0 < d < 3600 else None


def without_faint(frames, offset, near=2.0):
    """The frames without the tracks the user's faint-target cut-off leaves out (faint_scores, the recording's level
    less offset), and the cut and how many tracks went. A tracking run uses near=0: its bot is under the crosshair
    most of the time, so frames near the crosshair must count (Aethercontrol Easy: 82 of the bot's 3,618 frames lie
    2 deg or more away)."""
    q, _, level = faint_scores(frames, near)
    if level is None:
        return frames, None, 0
    cut = level - offset
    gone = {t for t, v in q.items() if v < cut}
    out = []
    for f in frames:
        keep = [k for k, t in enumerate(f["t"]) if t[0] not in gone]
        g = dict(f)
        for key in ("t", "a", "wh", "s"):
            if key in f:
                g[key] = [f[key][k] for k in keep]
        out.append(g)
    return out, round(cut, 3), len(gone)


def tracking_crosshair(frames, near=0.5, step=0.01):
    """Where a detector marks the crosshair in a tracking run, and the size of the box it gives it. The clicking rule
    (crosshair_spots) reads the frames where the camera turns, as no target stays put on screen then; in a tracking
    run the bot stays near the crosshair too. But the crosshair's box sits on one to three fixed points, a fraction
    of a pixel across, in most frames, and a bot never stays that still: a pile of boxes within 0.015 deg of a point
    in 5% of all the frames or more (the valorant run's crosshair boxes, put into the 12 tracking runs of
    eval_moving.py: 12% to 37%; the bots of those runs: 1.1% at most), then more such points within 0.3 deg of the
    first in 2% or more (its second point: 2.6% to 12%). Only boxes with a size count. Returns [(x, y) deg], at most
    3, and the median (w, h) deg of the boxes within 0.02 deg of the first point, or [] and None."""
    P = np.array([(x, y) for f in frames if "wh" in f for _, x, y in f["t"] if math.hypot(x, y) < near]).reshape(-1, 2)
    S = np.array([wh for f in frames if "wh" in f for (_, x, y), wh in zip(f["t"], f["wh"])
                  if math.hypot(x, y) < near]).reshape(-1, 2)
    out, size = [], None
    if len(P) < 30:
        return out, size
    n = int(round(2 * near / step))
    H = np.histogram2d(P[:, 0], P[:, 1], bins=n, range=[[-near, near], [-near, near]])[0]
    g = -near + (np.arange(n) + 0.5) * step
    for _ in range(3):
        B = sum(np.roll(np.roll(H, a, 0), b, 1) for a in (-1, 0, 1) for b in (-1, 0, 1))
        if out:                                     # its other points lie near the first
            B[np.hypot(g[:, None] - out[0][0], g[None, :] - out[0][1]) > 0.3] = 0
        i, j = np.unravel_index(np.argmax(B), B.shape)
        if B[i, j] < max(25, (0.02 if out else 0.05) * len(frames)):
            break
        on = np.hypot(*(P - np.array([g[i], g[j]])).T) < 0.02
        if not on.any():
            break
        c = P[on].mean(axis=0)
        if not out:
            on = np.hypot(*(P - c).T) < 0.02
            size = (float(np.median(S[on, 0])), float(np.median(S[on, 1])))
        out.append((float(c[0]), float(c[1])))
        H[np.hypot(g[:, None] - c[0], g[None, :] - c[1]) < 0.06] = 0
    return out, size


def without_crosshair(frames):
    """The frames without the boxes a detector puts on the crosshair in a tracking run (tracking_crosshair): those
    within 0.1 deg of one of its points, with a width and a height within 20% of its box's. With such boxes the
    crosshair is on a target in nearly every frame: put into the 12 tracking runs of eval_moving.py, the valorant
    run's crosshair boxes (from its fast turns, where no target is under the crosshair) took on_target from 0.27-0.87
    to 0.97-1.0; with them left out it comes within 0.034 of the runs' own (0.019 on average). A bot of the
    crosshair's size that sits on one of its points goes with them (Pokeball 1w2ts: 0.006 of the run). A run with no
    such points is left as it is."""
    spots, size = tracking_crosshair(frames)
    if not spots:
        return frames
    (w0, h0), out = size, []
    for f in frames:
        if "wh" not in f:
            out.append(f)
            continue
        keep = [k for k, ((_, x, y), (w, h)) in enumerate(zip(f["t"], f["wh"]))
                if not (any(math.hypot(x - a, y - b) < 0.1 for a, b in spots)
                        and abs(w - w0) <= 0.2 * w0 and abs(h - h0) <= 0.2 * h0)]
        g = dict(f)
        for key in ("t", "a", "wh", "s"):
            if key in f:
                g[key] = [f[key][k] for k in keep]
        out.append(g)
    return out


def track_summary(tracks, meta, limit=None, gap=0.1, cam=None, deaths=None, start=None):
    """A tracking run (bots that stay alive, so the stats file holds totals only): how the crosshair stayed on the
    target, from the tracks. The crosshair is on the target in a frame when it lies inside a target's box plus 0.05 deg
    (the model splits a thin capsule into several short boxes, so any box counts; tracks without box sizes, from the
    hand-written detector, take the target's area as a disc). The run starts at the first frame with a target (KovOBS
    starts recording with the run) and lasts the scenario's time limit (limit, seconds; the recording goes on for the
    results screen), or to the last frame with a target.
    on_target: the share of the run on the target; accuracy: the stats file's hits / (hits + misses), the game's own
    measure of the same thing (on 12 held-out tracking runs full_v3's on_target came within 0.075 of it on average).
    error: the median distance from the target's centre line (a sphere's centre, a capsule's long axis) in the frames
    on or near it (within 3 half-widths). lost: the stretches off the target longer than gap seconds (the bot lost),
    per second of tracking, and back: their median length (the time to get back on). lost_cost: the accuracy those
    stretches cost, their time as a share of the tracking time; slip_cost: what the shorter slips cost. With
    on_target they add up to 1.
    start: the run's first frame when known (the stats file's challenge start, placed by the matched kills, or the end
    of KovaaK's countdown, countdown_end). Without it, the run ends at the last frame on a target and starts its
    length (limit) before.
    deaths (frames, from the stats file's kill rows or the session HUD): where bots die. From a death until the
    crosshair is on a target again is switching, not tracking: it is left out of on_target, the lost stretches and the
    motion measures, and measured on its own. switches: per death [death, back on, first frame a target shows];
    on_all: the share on a target over the whole run, switching included, as the stats file's accuracy counts it.
    to_next: the median time from a death to being on a target again, split into waiting (no target on screen yet)
    and onto (from the first target shown to being on it); switching: the share of the run spent switching.
    per_second: per second, [the share on the target, the share switching].
    motion (with cam, camera_motion's reading): track_motion's diagnostics.
    The boxes a detector puts on the crosshair are left out first (without_crosshair)."""
    fr, fps = without_crosshair(tracks["frames"]), tracks["fps"]
    near, inside = [], []
    for f in fr:
        best, ins = None, False
        for k, ((tid, x, y), a) in enumerate(zip(f["t"], f["a"])):
            if "wh" in f:
                w, h = f["wh"][k]
                ins = ins or (abs(x) <= w / 2 + 0.05 and abs(y) <= h / 2 + 0.05)
                d = math.hypot(max(0.0, abs(x) - max(0.0, w - h) / 2), max(0.0, abs(y) - max(0.0, h - w) / 2))
                r = min(w, h) / 2
            else:
                r = math.degrees(math.atan(math.sqrt(a / math.pi) / K))
                d = math.hypot(x, y)
                ins = ins or d <= r + 0.05
            if best is None or d < best[0]:
                best = (d, r)
        near.append(best)
        inside.append(ins)
    hits, miss = meta.get("Hit Count"), meta.get("Miss Count")
    acc = float(hits) / (float(hits) + float(miss)) if hits and miss and float(hits) + float(miss) > 0 else None
    s = dict(scenario=meta.get("Scenario"), score=float(meta["Score"]) if meta.get("Score") else None, accuracy=acc,
             fps_avg=float(meta["Avg FPS"]) if meta.get("Avg FPS") else None, mode="track",
             sens=f"{meta.get('Horiz Sens')} {meta.get('Sens Scale')}" if meta.get("Horiz Sens") else None,
             on_target=None, error=None, lost=None, back=None, longest_off=None, per_second=[], start=None, end=None,
             bots=0, switches=[], to_next=None, waiting=None, onto=None, switching=None)
    idx = [i for i, b in enumerate(near) if b]
    if not idx:
        return s
    i0, i1 = idx[0], idx[-1] + 1
    if start is not None:
        i0 = max(0, int(start))
        if limit:
            i1 = min(len(near), i0 + int(round(limit * fps)))
    elif limit:                                     # nothing places the start (targets show in the countdown too):
        last = max((i for i, v in enumerate(inside) if v), default=i1 - 1)   # the run ends where the tracking does,
        i1 = last + 1                               # its last frame on a target, and starts its length before
        i0 = max(idx[0], i1 - int(round(limit * fps)))   # (Controlsphere: frames 0-60 s cut the run's last 2 s)
    if deaths and max(deaths) >= i1:                # the run went on past the limit given: up to its last death
        i1 = min(len(near), max(deaths) + int(fps))
    sw = np.zeros(len(fr), bool)                    # switching: from a bot's death until on a target again
    ds = sorted({int(d) for d in deaths or [] if i0 <= d < i1})
    for k, d in enumerate(ds):
        end = ds[k + 1] if k + 1 < len(ds) else i1
        back_on = next((t for t in range(d + 1, end) if inside[t]), end)
        seen = next((t for t in range(d + 1, back_on + 1) if t < len(fr) and fr[t]["t"]), back_on)
        sw[d:back_on] = True
        s["switches"].append([d, back_on, seen])
    tracking = ~sw[i0:i1]
    on = np.array(inside[i0:i1]) & tracking
    err = [b[0] for b, t in zip(near[i0:i1], tracking) if t and b and b[0] <= 3 * b[1]]
    lab, n = ndimage.label(~on & tracking)
    offs = [int(x) for x in np.bincount(lab.ravel())[1:]] if n else []
    offs = [k for k in offs if k / fps > gap]
    sec = max(1, int(round(fps)))
    secs = max(1e-9, tracking.sum() / fps)
    s.update(on_target=float(on.sum() / max(1, tracking.sum())), on_all=float(np.mean(inside[i0:i1])),
             error=float(np.median(err)) if err else None,
             lost=len(offs) / secs, lost_cost=float(sum(offs) / max(1, tracking.sum())),
             slip_cost=float(max(0.0, 1 - on.sum() / max(1, tracking.sum()) - sum(offs) / max(1, tracking.sum()))),
             back=float(np.median(offs)) / fps if offs else None,
             longest_off=max(offs) / fps if offs else None, start=i0, end=i1,
             per_second=[[round(float(on[k:k + sec].mean()), 3), round(float((~tracking[k:k + sec]).mean()), 3)]
                         for k in range(0, len(on), sec)])
    if ds:
        w = s["switches"]
        s.update(bots=len(ds), to_next=float(np.median([b - d for d, b, _ in w])) / fps,
                 waiting=float(np.median([e - d for d, _, e in w])) / fps,
                 onto=float(np.median([b - e for _, b, e in w])) / fps, switching=float((~tracking).mean()))
    if cam is not None:
        s["motion"] = track_motion(fr, fps, cam, i0, i1, inside, sw)
    s["what_if"] = what_if(s, tracking, on, offs, fps)
    return s


def what_if(s, tracking, on, offs, fps):
    """Estimates of how much the time on target (and so the accuracy) would rise if one thing changed, all else the
    same: the off-target time that thing accounts for, as a share of the whole run (points of accuracy, which counts
    the whole run). They
    overlap, so they do not add up; each is a ceiling. Sorted, biggest first."""
    T, R = max(1, int(tracking.sum())), max(1, len(tracking))   # tracking frames, and the whole run's
    out = []

    def add(what, frames, how):                     # against the whole run, as the game's accuracy counts it
        if frames and frames > 0:
            out.append(dict(what=what, gain=float(frames / R), how=how))
    lost = sum(offs)
    add("Get back on twice as fast", lost / 2, "Half the time off the bot in drops longer than 0.1 s.")
    add("Don't slip", max(0, T - int(on.sum()) - lost), "The time off the bot in slips shorter than 0.1 s.")
    m = s.get("motion") or {}
    f = m.get("frames") or {}
    add("Don't lead", f.get("ahead"), "The time off the bot ahead of it, past its leading edge.")
    add("Don't trail", f.get("behind"), "The time off the bot behind it, trailing it.")
    add("Don't get thrown by its turns", f.get("turns"), "The time off the bot in the 0.4 s after each of its direction "
        "changes.")
    add("Track every direction like your best one", f.get("directions"), "Each direction of the bot's motion "
        "brought up to the time on target of your best one (among those it moved in 5% of the time or more).")
    sec = max(1, int(round(fps)))
    per = [(on[k:k + sec].sum(), tracking[k:k + sec].sum()) for k in range(0, len(on), sec)]
    if len(per) >= 20:
        w = [(sum(a for a, _ in per[k:k + 10]), sum(b for _, b in per[k:k + 10])) for k in range(len(per) - 9)]
        best10 = max((a / b for a, b in w if b >= 5 * sec), default=None)   # with 5 s of tracking or more
        if best10 is not None:
            add("Keep up your best 10 seconds all run", (best10 - s["on_target"]) * T,
                f"Your best 10 seconds were {round(100 * best10)}% on target.")
    if s.get("switches"):
        add("Get onto the next bot 100 ms faster", min(len(s["switches"]) * 0.1 * fps,
                                                    sum(b - d for d, b, _ in s["switches"])),
            "100 ms less per switch between bots.")
    return sorted(out, key=lambda r: -r["gain"])


def _fitts(ms, W):
    """Fitts' law fitted to the run's kills by least squares: a function from a flick's distance to its predicted time,
    or None with too few kills."""
    pts = [(math.log2(1 + m["D0"] / W), m["total"]) for m in ms if m["total"] is not None and m["D0"]]
    if len(pts) < 3 or W <= 0:
        return None
    mx, my = st.mean(p[0] for p in pts), st.mean(p[1] for p in pts)
    sxx = sum((p[0] - mx) ** 2 for p in pts)
    b = sum((p[0] - mx) * (p[1] - my) for p in pts) / sxx if sxx else 0.0
    a = my - b * mx
    return lambda D: a + b * math.log2(1 + D / W)


def ms_(v):
    return f"{1000 * v:.0f} ms" if v is not None else "-"


def judge(s):
    """Provisional checks, one per issue in the list (numbers as in docs/issues.md). Each: the issue, the number it
    reads, a plain verdict and why. The thresholds are first guesses until the issue list is settled with the user."""
    out = []

    def add(num, title, value, flag, why):
        out.append(dict(issue=num, title=title, value=value, flag=flag, why=why))

    if s["react"] is not None:
        add(1, "Slow start", f"{1000 * s['react']:.0f} ms to start moving after a kill (median)",
            "attention" if s["react"] > 0.15 else "fine",
            "With several targets on screen this is the switch to the next target, not a reaction to seeing it. "
            "Over 150 ms is flagged.")
    if s["ended_past"] is not None:
        add(9, "Overflick", f"{100 * s['ended_past']:.0f}% of main flicks ended past the far edge",
            "attention" if s["ended_past"] > 0.15 else "fine", "Over 15% is flagged.")
    if s["ended_short"] is not None:
        cost = s.get("mid_short_cost")
        add(10, "Stopping short", f"{100 * s['ended_short']:.0f}% of main flicks stopped short"
            + (f", covering {100 * s['short_covered']:.0f}% of the way" if s.get("short_covered") else ""),
            "attention" if cost is not None and cost > 0.03 else "fine",
            "Stopping short is normal; it is flagged only when it costs time: for 10-25 deg flicks the short ones took "
            + (f"{1000 * cost:+.0f} ms" if cost is not None else "an unknown time") + " against the ones that landed.")
    if s.get("mode") == "hold" and s.get("slipped") is not None:
        add(24, "Unstable landing", f"the crosshair slipped off the target after reaching it in {100 * s['slipped']:.0f}% "
            "of kills", "attention" if s["slipped"] > 0.3 else "fine",
            "Hold-fire run: each slip breaks the hold and costs time. Over 30% is flagged.")
        add(24, "Time off the target while holding", f"{100 * s['off_share']:.0f}% of the time between reaching a target "
            f"and killing it was spent off it (median hold {ms_(s['hold'])})", "attention" if s["off_share"] > 0.15 else "fine",
            "Over 15% is flagged.")
    if s.get("mode") != "hold" and s["still"] is not None and s["median_interval"]:
        share = s["still"] / s["median_interval"]
        add(36, "Waiting on the target", f"{1000 * s['still']:.0f} ms still on the target before the click "
            f"({100 * share:.0f}% of a kill)", "attention" if s["still"] > 0.08 else "fine",
            (f"When the flick landed on the target it waited {1000 * s['still_landed']:.0f} ms; when it was "
             f"corrected in, {1000 * s['still_corrected']:.0f} ms. " if s.get("still_landed") and s.get("still_corrected")
             else "") + "Over 80 ms is flagged.")
    if s.get("mode") != "hold" and s["moving_clicks"] is not None:
        add(34, "Clicking while still moving", f"{100 * s['moving_clicks']:.0f}% of clicks at over 20 deg/s",
            "attention" if s["moving_clicks"] > 0.10 else "fine", "Over 10% is flagged.")
    if s["nearest_chosen"] is not None:
        add(56, "Target choice", f"{100 * s['nearest_chosen']:.0f}% of the time the next target was the nearest"
            + (f"; otherwise {s['extra_when_not_nearest']:.1f} deg farther" if s["extra_when_not_nearest"] else ""),
            "attention" if s["nearest_chosen"] < 0.6 else "fine",
            "A farther target can be a planned route, so this is a prompt to check, not a verdict. Under 60% is flagged.")
    if s.get("pace_first") and s.get("pace_last"):
        drop = s["pace_last"] / s["pace_first"] - 1
        add(49, "Pacing drop", f"kills took {100 * drop:+.0f}% longer in the last third than in the first",
            "attention" if drop > 0.10 else "fine", "Over 10% slower is flagged.")
    dirs = [d for d in s["by_direction"] if d["n"] >= 10 and d.get("beyond") is not None]
    if len(dirs) >= 3 and s["median_interval"]:
        worst = max(dirs, key=lambda d: d["beyond"])
        extra = worst["beyond"] - st.median(d["beyond"] for d in dirs if d is not worst)
        share = extra / s["median_interval"]
        add(13, "Direction bias", f"flicks {worst['name']} took {ms_(extra)} more than the other directions for their "
            f"distance ({100 * share:.0f}% of the median kill)", "attention" if share > 0.15 else "fine",
            "Each direction is compared with what its distances predict (Fitts' law fitted to the run), so far and near "
            "directions compare fairly. Directions with fewer than 10 flicks are left out. Over 15% of the median kill "
            "is flagged.")
    if s.get("mode") != "hold" and s.get("misses") is not None and s["kills"]:
        rate = s["misses"] / max(1, s["kills"] + s["misses"])
        add(39, "Misses", f"{s['misses']} misses ({100 * rate:.1f}% of shots)", "attention" if rate > 0.08 else "fine",
            "Over 8% is flagged.")
    return out


def run_window(run, fps, limit):
    """The user's run window for a recording (run.json: start, end, length in seconds, any of them None) as (first
    frame, length in seconds), or (None, limit) where it says nothing. Two of the three settle the third; a start or
    an end alone takes the length given (the stats file's or the scenario's)."""
    if not run:
        return None, limit
    a, b, L = run.get("start"), run.get("end"), run.get("length") or None
    if a is not None and b is not None and b > a:
        return round(a * fps), b - a
    L = L or limit
    if a is not None:
        return round(a * fps), L
    if b is not None and L:
        return round(max(0.0, b - L) * fps), L
    return None, L


def review(video, stats, out_dir, progress=None, detector=None, exclude=None, run=None, faint=None):
    """The whole pipeline, caching tracks.json, flicks.json, measures.json and report.json in out_dir.
    Kill times come from the stats file; without one, from KovaaK's session HUD in the video (hud.py: kills, shots and
    hits, frame by frame; the score from the file name); without a readable HUD, from the video alone (match_video:
    no score, shots, misses or accuracy). info["source"] says which: "stats", "hud" (KovaaK's),
    "aimlab" (Aim Lab's POINTS) or "video". run: the user's run window (run_window), which a tracking run's measures
    use before any other start or length. faint: the user's faint-target cut-off ({on, offset}); a tracking run's
    measures leave out the tracks it cuts (without_faint)."""
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    scenario, score = (Path(video).stem.rsplit(" - ", 2) + ["", ""])[:2]
    kind, limit = scenario_facts().get(scenario.lower(), (None, None))
    tracking = kind == "tracking"
    hud_box, hud_thread = {}, None
    hp = out_dir / "hud.json"
    reader = hashlib.md5((Path(__file__).parent / "hud.py").read_bytes()).hexdigest()[:10]   # a changed reader reads
    cached = json.load(open(hp)) if hp.exists() else None                                    # the HUD again
    if not stats:                                    # read the HUD while the tracking runs: two decodes side by side
        if isinstance(cached, dict) and cached.get("reader") == reader:
            hud_box["r"] = cached["r"]
        else:
            import hud as hud_mod

            def read_hud():
                try:
                    hud_box["r"] = hud_mod.read(video) or hud_mod.read_aimlab(video)
                except Exception as e:              # a HUD that cannot be read is not fatal: the video fallback runs
                    hud_box["r"], hud_box["error"] = None, str(e)
            hud_thread = threading.Thread(target=read_hud, daemon=True)
            hud_thread.start()
    tp = out_dir / "tracks.json"
    if tp.exists():
        tracks = json.load(open(tp))
    else:
        k = target_counts().get(Path(video).stem.rsplit(" - ", 2)[0].lower())
        mask = None if exclude is None else mask_of(exclude)       # the areas the user excluded (default: KovOBS's)
        tracks = track_model(video, detector, progress, cap=k, areas=exclude) if detector is not None \
            else track(video, progress, mask=mask)
        json.dump(tracks, open(tp, "w"))
    if progress:
        progress("measuring", 0, 1)
    if tracking:                                     # the time on the target, and where bots died if they do
        mask = mask_of(exclude) if exclude else MASK
        cp = out_dir / "camera.json"                # the camera's turn, kept with the tracks it was read for
        cam = json.load(open(cp)) if cp.exists() and cp.stat().st_mtime >= tp.stat().st_mtime else None
        if cam is None or len(cam) < len(tracks["frames"]) - 2:
            cam = camera_motion(video, tracks["frames"], mask, fixed_map(list(_frames(video, keyframes=True))),
                                progress)
            json.dump(cam, open(cp, "w"))
        deaths, source, start = [], "stats" if stats else "video", None
        if stats:
            limit = stats_length(stats) or limit
            if load_stats(stats)[1]:                 # bots that die: their kills, matched in the video
                fl, inf = match(tracks, stats)
                deaths = [f["kill_frame"] for f in fl]
                if inf.get("offset") is not None and inf["matched"]:
                    start = round(inf["offset"] * tracks["fps"])     # the challenge start on the video's clock
        else:
            if hud_thread:
                hud_thread.join()
                json.dump(dict(reader=reader, r=hud_box.get("r")), open(hp, "w"))
            h = hud_box.get("r")
            if h and h.get("game") != "aimlab" and h.get("kills"):
                deaths, source = list(h["kills"]), "hud"
        if start is None and limit:                 # no kills to place the start: KovaaK's countdown ends it
            n = len(tracks["frames"])
            start = countdown_end(video, max(5.0, n / tracks["fps"] - limit + 3))
        if run:                                     # the user's own window comes first
            r0, limit = run_window(run, tracks["fps"], limit)
            start = r0 if r0 is not None else start
        measured, cut = tracks, None
        if faint and faint.get("on"):               # the user's cut-off: the measures leave out what it cuts
            frames_, c_, gone = without_faint(tracks["frames"], float(faint.get("offset", 0.3)), near=0.0)
            measured, cut = dict(tracks, frames=frames_), dict(offset=faint.get("offset"), cut=c_, tracks=gone)
        s = track_summary(measured, load_stats(stats)[0] if stats else {"Scenario": scenario}, limit, cam=cam,
                          deaths=deaths, start=start)
        s["faint"] = cut
        s["info"] = dict(source=source)
        report = dict(video=str(video), stats=str(stats) if stats else None, summary=s, issues=[], flicks=[],
                      mode="track", paths={}, fps=tracks["fps"], geometry=dict(W=W, H=H, CX=CX, CY=CY, K=K),
                      appeared={}, crosshair=[], run=run, limit=limit)
        json.dump(report, open(out_dir / "report.json", "w"))
        return report
    score = (re.match(r"\s*(\d+(?:\.\d+)?)", score) or [None, None])[1]    # other recorders name files freely
    if stats:
        flicks, info = match(tracks, stats)
        info["source"] = "stats"
        meta, rows = load_stats(stats)
        per_kill = sorted(int(r[5]) for r in rows) if rows else [1]
    else:
        if hud_thread:
            if progress:
                progress("reading the HUD", 0, 1)
            hud_thread.join()
            json.dump(dict(reader=reader, r=hud_box.get("r")), open(hp, "w"))
        h = hud_box.get("r")
        if h and h.get("game") == "aimlab" and len(h["kills"]) > 1.3 * len(match_video(tracks)[0]):
            h = None                # Aim Lab counts hits: in a task whose targets take several hits, the video's kills
        if h and h["kills"]:
            fps = tracks["fps"]
            # shots per kill. The Accuracy row can update up to a third of a second after the Kill Count, so a shot can
            # land in the next kill's stretch. In one-hit scenarios (hits about equal to kills) each kill is one hit
            # plus the misses in its stretch; otherwise the shots in its stretch.
            k, sh, hi = h["kills"], collections.Counter(h["shots"]), collections.Counter(h["hits"])
            spans = list(zip([-1] + k[:-1], k))
            if h["hits"] and len(h["hits"]) <= 1.2 * len(k):
                miss = {f: sh[f] - hi.get(f, 0) for f in sh if sh[f] > hi.get(f, 0)}
                shots = [1 + sum(n for f, n in miss.items() if a < f <= b) for a, b in spans]
            else:
                shots = [sum(n for f, n in sh.items() if a < f <= b) for a, b in spans]
            # Aim Lab's crosshair is marked as a target by every model: its tracks were taken for the killed target
            # and 42 of 206 flicks went unmeasured. In KovaaK's runs they stay: a target held under the crosshair
            # looks like them (on a valorant run, leaving them out lost 16 of 66 stats-file kills)
            t = without_ghosts(tracks) if h.get("game") == "aimlab" else tracks
            flicks, info = match_times(t, [f / fps for f in h["kills"]], shots, off=0.0)
            info["source"] = "aimlab" if h.get("game") == "aimlab" else "hud"
            fin = h["final"]
            meta = {"Scenario": scenario, "Kills": fin["kills"], "Score": score if score is not None else h.get("points")}
            if fin.get("shots") is not None:
                meta.update({"Hit Count": fin["hits"], "Miss Count": fin["shots"] - fin["hits"]})
            per_kill = sorted(shots) or [1]
        else:
            flicks, info = match_video(tracks)
            meta, per_kill = {"Scenario": scenario}, [1]
    json.dump(flicks, open(out_dir / "flicks.json", "w"))
    R = target_radius(flicks)
    ms = measure(flicks, tracks["fps"], R)
    json.dump(ms, open(out_dir / "measures.json", "w"))
    mode = "hold" if per_kill[len(per_kill) // 2] > 3 else "click"
    ch = choices(tracks, flicks)
    s = summarize(ms, ch, meta, info, R, mode)
    appeared, _ = appearances(tracks)
    report = dict(video=str(video), stats=str(stats) if stats else None, summary=s, issues=judge(s), flicks=ms, mode=mode,
                  paths={str(f["n"]): f["traj"] for f in flicks}, fps=tracks["fps"],
                  geometry=dict(W=W, H=H, CX=CX, CY=CY, K=K), appeared={str(k): v for k, v in appeared.items()},
                  crosshair=crosshair_spots(tracks["frames"]), run=run)
    json.dump(report, open(out_dir / "report.json", "w"))
    return report
