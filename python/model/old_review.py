"""The parts of the old Python review (python/retired/review.py) that the training scripts still use, copied verbatim by
scripts/extract_module.py on 2026-10-04: reading frames, the fixed map, the hand-written detector (build_data.py's
labels), the old tracker and matching (build_kills.py, build_mined.py, and hand_crops.py's cut-off crops, which the
browser's crops mirror), the camera watch (contract.py), stats files and scenario facts.

Frozen: the app's review is the Rust core (src/), and a change to it is not made here. The scripts that grade a model
(accept.py, eval_vods.py, eval_moving.py, eval_video_alone.py) use the core, through aimview_tools.py and
examples/review.rs. Kept as it was, so the training data it builds stays the same."""


import collections
import functools
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
import sys
import numpy as np
from scipy import ndimage

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import local_config  # noqa: E402


W, H = 1280, 720


CX, CY = 640.03, 359.75                     # the crosshair's centre at this scale (the red dot, measured)


K = (W / 2) / math.tan(math.radians(51.5))  # 103 deg horizontal FOV (Overwatch scale)


# The KovOBS overlay at 1280 x 720, masked out: session box, timer, clock and FPS, settings box, gun and title,
# crosshair zoom and hand cam, version number.
OVERLAY = ((0, 0, 205, 150), (590, 0, 690, 60), (1160, 0, W, 100), (0, 620, 430, H), (570, 630, 715, H),
           (400, 675, 880, H), (960, 535, W, H), (0, 700, 60, H))   # (400, 675, 880, H): the scenario's name, any length


OVERLAY_KINDS = ("Session stats", "Timer", "Clock", "Settings", "Weapon", "Scenario name", "Webcam", "Version")


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


# without Steam's folder, a folder that is not there: no scenario files, as when KovaaK's is not installed
SCENARIOS = tuple(str(local_config.kovaak(name) or local_config.ROOT / "no Steam") for name in ("scenarios", "workshop"))


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
    tracks = without_crosshair_ends(tracks)
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


ON_SPOT_DEG = 0.1             # a box this near the crosshair's center (crosshair_center()) is on the crosshair spot


TURNING_DEG = 0.1             # the camera turns on a frame whose view shift is more: no target stays put on screen then


RING_DEG = (0.2, 0.4)         # the ring around the center whose boxes the spot's are weighed against


SPOT_DENSITY = 5.0            # how many times as densely as the ring's the spot's boxes lie when the crosshair is boxed


SPOT_SHARE, SPOT_BOXES = 0.02, 25   # the fewest boxes on the spot: a share of the turning frames, and a number


SAME_SIZE = 0.2               # a box has the crosshair box's size when its width and height are each within this share


def crosshair_center():
    """Where the detector's box on the crosshair sits (deg): the screen's center, since the crosshair is always drawn
    there. A box's center is in the pixels' own numbering (pixel i's center is at i: the labels the detector learned
    from were blob centroids), so the center of a frame W pixels wide is at W / 2 - 0.5."""
    return to_deg(W / 2 - 0.5, H / 2 - 0.5)


def _turning(frame):
    """Whether the camera turns on a frame (TURNING_DEG)."""
    return math.hypot(*(frame.get("shift") or (0.0, 0.0))) > TURNING_DEG


def crosshair_spots(frames):
    """The crosshair spot, if the detector marks the crosshair: its center (crosshair_center()), when the boxes within
    ON_SPOT_DEG of it on the frames where the camera turns are at least SPOT_SHARE of those frames (and SPOT_BOXES),
    and lie at least SPOT_DENSITY times as densely as the boxes in the ring around it (RING_DEG): a target held near
    the crosshair while the camera turns spreads over both. Returns [(x, y) deg], at most 1."""
    center = crosshair_center()
    turning_frames = on_spot = in_ring = 0
    for frame in filter(_turning, frames):
        turning_frames += 1
        for _, x, y in frame["t"]:
            distance = math.hypot(x - center[0], y - center[1])
            if distance < ON_SPOT_DEG:
                on_spot += 1
            elif RING_DEG[0] <= distance < RING_DEG[1]:
                in_ring += 1
    enough = on_spot >= max(SPOT_SHARE * turning_frames, SPOT_BOXES)
    spot_area = ON_SPOT_DEG * ON_SPOT_DEG
    ring_area = RING_DEG[1] * RING_DEG[1] - RING_DEG[0] * RING_DEG[0]
    dense = on_spot / spot_area >= SPOT_DENSITY * in_ring / ring_area
    return [center] if enough and dense else []


def _kept(frame, keep):
    """A frame with only the targets `keep` (one bool each) keeps, in each of its lists of that length."""
    lists = [key for key in ("t", "a", "wh", "s") if frame.get(key) is not None and len(frame[key]) == len(keep)]
    return dict(frame, **{key: [value for value, kept in zip(frame[key], keep) if kept] for key in lists})


def without_crosshair_ends(tracks):
    """The tracks without the end of each one that turns into the crosshair's box: where the detector marks the
    crosshair, the tracker hands it the killed target's track, which runs on until the camera turns (the box stays put
    on screen). A box is the crosshair's on the spot (crosshair_spots(), within ON_SPOT_DEG) with the size of the
    crosshair's box (SAME_SIZE of the median width and height of the boxes on the spot while the camera turns: the
    crosshair never changes). A track whose last boxes are the crosshair's, after a box of another size, and that ends
    as the camera turns, ends at that box. One that ends with the camera still keeps its end: the crosshair's box went
    with the target (a detector can mark the crosshair only over a target). So does a target as big as the crosshair's
    box, which cannot be told from it."""
    frames = tracks["frames"]
    spots = crosshair_spots(frames)
    if not spots:
        return tracks
    center = spots[0]

    def on_spot(x, y):
        return math.hypot(x - center[0], y - center[1]) < ON_SPOT_DEG

    def sized_boxes(frame):                         # its boxes with their sizes (none in tracks without sizes)
        return zip(frame["t"], frame["wh"]) if frame.get("wh") is not None else ()

    sizes = [size for frame in filter(_turning, frames) for (_, x, y), size in sized_boxes(frame) if on_spot(x, y)]
    if not sizes:
        return tracks
    crosshair = st.median([width for width, _ in sizes]), st.median([height for _, height in sizes])

    def crosshair_sized(width, height):
        return (abs(width - crosshair[0]) <= SAME_SIZE * crosshair[0]
                and abs(height - crosshair[1]) <= SAME_SIZE * crosshair[1])

    last_other, end_frame = {}, {}      # each track's last box not the crosshair's (frame, its size), and its end
    for frame in frames:
        for (track, x, y), size in sized_boxes(frame):
            if not on_spot(x, y) or not crosshair_sized(*size):
                last_other[track] = (frame["i"], crosshair_sized(*size))
            end_frame[track] = frame["i"]
    cut_after = {track: last for track, (last, sized) in last_other.items()
                 if not sized and end_frame[track] + 1 < len(frames) and _turning(frames[end_frame[track] + 1])}
    if not cut_after:
        return tracks
    keep = [[point[0] not in cut_after or frame["i"] <= cut_after[point[0]] for point in frame["t"]] for frame in frames]
    return dict(tracks, frames=[_kept(frame, kept) for frame, kept in zip(frames, keep)])


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
