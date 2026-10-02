"""Read KovaaK's session HUD in a recording: the box headed SESSION, with Kill Count, KPS, Accuracy (hits/shots),
Damage, SPM and Avg TTK. It is optional in KovaaK's, but most players show it. When a run has no stats file, it gives
the kill times (the Kill Count changes 0 to 2 frames after each kill) and the shots, hits and misses.

The theme can recolour it, so nothing assumes a colour: each frame's text is told from the box behind it by contrast,
and the digits are learned from the recording itself. The Kill Count goes up one kill at a time, so its last digit
cycles 0 to 9: the order of the glyph shapes gives the digits' order, and the shape on the units place when the tens
place changes is 0. No font file is needed. If the box is missing or reads inconsistently, read() returns None and the
review falls back to finding kills in the video alone (review.match_video).
"""
import collections
import subprocess
from pathlib import Path

import numpy as np
from PIL import Image
from scipy import ndimage

BOX = (0, 0, 900 / 2560, 330 / 1440)      # where the box can be, as shares of the frame (x0, y0, x1, y1); it grows to fit
BW, BH = 900, 330                          # its widest number. The region is scaled to this (2560 x 1440 pixels)
SEED = (49, 100, 57, 108)                  # a patch inside the box's left edge, between the header and Kill Count
GW, GH = 16, 24                            # glyphs are compared at this size
SAME = 0.97                                # glyphs this alike (cosine of their grey images) are the same shape


def _frames(video, keyframes=False, box=BOX, size=(BW, BH), scaler="area"):
    """A region of every frame (or of the key frames), grey, scaled to size; by default KovaaK's box region."""
    (x0, y0, x1, y1), (BW, BH) = box, size
    vf = f"crop=iw*{x1 - x0}:ih*{y1 - y0}:iw*{x0}:ih*{y0},scale={BW}:{BH}:flags={scaler},format=gray"
    p = subprocess.Popen(["ffmpeg", "-v", "error"] + (["-skip_frame", "nokey"] if keyframes else []) + ["-i", str(video)] +
                         (["-fps_mode", "passthrough"] if keyframes else []) + ["-vf", vf, "-f", "rawvideo", "-"],
                         stdout=subprocess.PIPE, bufsize=0)
    size = BW * BH
    try:
        while True:
            buf = bytearray(size)
            mv, got = memoryview(buf), 0
            while got < size:
                k = p.stdout.readinto(mv[got:])
                if not k:
                    return
                got += k
            yield np.frombuffer(buf, np.uint8).reshape(BH, BW)
    finally:
        p.stdout.close()
        p.wait()


def _ink(img):
    """Text pixels: far from the region's most common level (the box), in either direction."""
    img = img.astype(np.int16)
    bg = np.median(img)
    d = np.abs(img - bg)
    return d > max(25, 0.5 * np.percentile(d, 99.5))


def _spans(mask, min_len=1):
    """Runs of True in a 1-d mask, as (start, end)."""
    out, cur = [], None
    for i, v in enumerate(mask):
        if v and cur is None:
            cur = i
        elif not v and cur is not None:
            if i - cur >= min_len:
                out.append((cur, i))
            cur = None
    if cur is not None and len(mask) - cur >= min_len:
        out.append((cur, len(mask)))
    return out


def layout(video):
    """The box's text rows from the key frames: (rows, x0, x1) with rows as (y0, y1), or None without a box. The
    median over key frames keeps the box and its labels and washes out the moving scene and the changing numbers."""
    keys = list(_frames(video, keyframes=True))
    if len(keys) < 3:
        return None
    med = np.median(np.stack(keys), axis=0)
    # the box: the area at the level of a patch inside its left edge (the scene around it is another level). The patch
    # is where the box is in the user's recordings; other players' HUDs are smaller or placed elsewhere, so other
    # patches are tried until one gives a box
    tried = set()
    for sx, sy in [SEED[:2]] + [(x, y) for y in range(40, 240, 12) for x in range(20, 320, 12)]:
        x0, y0, x1, y1 = sx, sy, sx + 8, sy + 8
        level = np.median(med[y0:y1, x0:x1])
        near = np.abs(med - level) < 15
        if near[y0:y1, x0:x1].mean() < 0.9:                    # not a flat patch
            continue
        lab, n = ndimage.label(near)
        seed = np.bincount(lab[y0:y1, x0:x1].ravel(), minlength=n + 1)
        seed[0] = 0
        if not n or not seed.any():
            continue
        big = int(np.argmax(seed))
        ys, xs = np.where(lab == big)
        by0, by1, bx0, bx1 = ys.min(), ys.max() + 1, xs.min(), xs.max() + 1
        if (by0, bx0) in tried or by1 - by0 < 0.3 * BH or bx1 - bx0 < 0.2 * BW or by1 >= BH - 2 or bx1 >= BW - 2:
            continue                                            # too small, or the open scene running off the region
        tried.add((by0, bx0))
        if len(ys) < 0.6 * (by1 - by0) * (bx1 - bx0):           # a box is a filled rectangle (with text holes)
            continue
        m = 4                                                   # keep off the box's rounded edge
        ink = _ink(med[by0 + m:by1 - m, bx0 + m:bx1 - m])
        rows = [(r0 + by0 + m, r1 + by0 + m) for r0, r1 in _spans(ink.sum(1) > 2, 6)]
        # a header (SESSION and the clock) and six rows under it; Kill Count is the first of them, Accuracy the third.
        # The compact HUD has four rows in two columns (Kill Count and SPM, Accuracy, Damage, Avg TTK and KPS), each
        # value just after its label's colon: starts[k] is where row k's value can start (None: read the rightmost)
        if len(rows) >= 4:
            x0, x1 = int(bx0 + m), int(bx1 - m)
            starts = [_label_end(_ink(med[max(0, a - 3):b + 3, x0:x1])) if len(rows) == 5 else None for a, b in rows]
            return rows, x0, x1, starts
    return None


def _label_end(ink):
    """The column just past the first colon in a row of text: a narrow glyph made of dots only (an i has a stem; the
    colon's upper dot can fade in a blurred recording)."""
    for a, b in _spans(ink.any(0)):
        runs = _spans(ink[:, a:b].any(1))
        if b - a <= max(3, 0.3 * ink.shape[0]) and runs and all(r1 - r0 <= 0.35 * ink.shape[0] for r0, r1 in runs):
            return b
    return None


def _value_glyphs(band, start=None):
    """The value's glyphs in one row: the rightmost group of ink columns, cut from the label by a wide gap (or, given
    start, the first group from there on). Each glyph is cropped to its ink and scaled to GW x GH (grey), with its
    height share (to tell commas and dots)."""
    ink = _ink(band)
    d = np.abs(band.astype(np.float32) - np.median(band))
    strength = np.clip(d / max(25.0, float(np.percentile(d, 99.5))), 0, 1).astype(np.float32)
    # the box widens as its numbers grow: past its right edge the scene fills whole columns, which text never does
    solid = np.where(ink.mean(0) > 0.85)[0]
    solid = solid[solid > 0.2 * ink.shape[1]]
    if len(solid):
        ink = ink[:, :solid[0]]
    cols = _spans(ink.any(0))
    if not cols:
        return []
    if start is not None:
        cols = [c for c in cols if c[0] >= start]
        if not cols:
            return []
        group = [cols[0]]
        for c in cols[1:]:
            if c[0] - group[-1][1] > 0.07 * band.shape[1]:   # the gap before the next label
                break
            group.append(c)
    else:
        group = [cols[-1]]
        for c in reversed(cols[:-1]):
            if group[0][0] - c[1] > 0.07 * band.shape[1]:    # the gap between the label and the value
                break
            group.insert(0, c)
    # small, blurred text (a smaller HUD, a re-encoded upload) joins neighbouring digits. A digit is at most 0.9 times as
    # wide as it is tall, so a wider glyph is split at its thinnest columns, one piece per 0.6 of its height
    split = []
    for a, b in group:
        ys = np.where(ink[:, a:b].any(1))[0]
        gh = ys.max() - ys.min() + 1
        k = int(round((b - a) / gh / 0.6))
        if (b - a) / gh < 1.0 or k < 2:
            split.append((a, b))
            continue
        cuts, col = [a], ink[:, a:b].sum(0)
        for j in range(1, k):
            c = (b - a) * j / k
            lo, hi = int(c - 0.25 * (b - a) / k), int(c + 0.25 * (b - a) / k) + 1
            cuts.append(a + lo + int(np.argmin(col[lo:hi])))
        cuts.append(b)
        split += [(p, q) for p, q in zip(cuts, cuts[1:]) if q > p]
    group = split
    out, h = [], ink.shape[0]
    for a, b in group:
        ys = np.where(ink[:, a:b].any(1))[0]
        g = strength[ys.min():ys.max() + 1, a:b]
        img = np.asarray(Image.fromarray(g).resize((GW, GH), Image.BILINEAR))   # grey: small blurred digits differ
        out.append((img, (ys.max() - ys.min() + 1) / h, a))                       # in grey more than in black and white
    return out


class _Shapes:
    """Glyph shapes seen so far; a glyph joins the most alike shape, or starts a new one."""

    def __init__(self, same=SAME):
        self.shapes, self.count, self.same = [], [], same

    def id(self, g, learn=True):
        best = None
        v = g.ravel() / max(1e-6, float(np.linalg.norm(g)))
        for k, s in enumerate(self.shapes):
            sim = float(v @ s.ravel()) / max(1e-6, float(np.linalg.norm(s)))
            if sim >= self.same and (best is None or sim > best[0]):
                best = (sim, k)
        if best is None and not learn:
            return -1
        if best is None:
            self.shapes.append(g.astype(np.float32).copy())
            self.count.append(0)
            best = (1.0, len(self.shapes) - 1)
        k = best[1]
        self.count[k] += 1
        if learn and self.count[k] <= 50:      # the shape is the mean of its first Kill Count glyphs (the Accuracy
                                               # row's slashes and brackets, cut into pieces, would blur it)
            self.shapes[k] += (g - self.shapes[k]) / self.count[k]
        return k


def _runs(readings, min_len=3):
    """Stable readings: (reading, first frame, last frame) for each stretch at least min_len frames long."""
    runs = []
    for i, r in enumerate(readings):
        if runs and runs[-1][0] == r:
            runs[-1][2] = i
        else:
            runs.append([r, i, i])
    return [tuple(x) for x in runs if x[2] - x[1] + 1 >= min_len and x[0]]


def _learn_digits(runs):
    """Which shape is which digit, from the Kill Count's stable readings counting up. Returns {shape: digit} or None."""
    zero, succ = collections.Counter(), collections.Counter()
    for (a, _, _), (b, _, _) in zip(runs, runs[1:]):
        if len(b) == len(a) and a[:-1] != b[:-1] or len(b) == len(a) + 1:
            zero[b[-1]] += 1                                   # the tens place changed (or a digit was added): b ends in 0
        if len(b) >= len(a):
            succ[(a[-1], b[-1])] += 1                          # usually one more kill: the last digit's next shape
    if not zero:
        return None
    nxt = {}
    for (x, y), n in succ.most_common():
        if x != y and x not in nxt and y not in nxt.values():
            nxt[x] = y
    z = zero.most_common(1)[0][0]
    digits, s = {z: 0}, z
    for d in range(1, 10):
        s = nxt.get(s)
        if s is None or s in digits:
            return None
        digits[s] = d
    return digits if nxt.get(s) == z else None


def _number(glyphs, ids, digits):
    """Digits read left to right, skipping short glyphs (commas, dots); None if a tall glyph is not a digit."""
    v = ""
    for (img, hshare, _), k in zip(glyphs, ids):
        if hshare < 0.5:
            continue
        if k not in digits:
            return None
        v += str(digits[k])
    return int(v) if v else None


def _count(kill_g, acc_g, same, need):
    """The Kill Count's stable values at one likeness: (acc_read, digits, values, checked), or None when the digits are
    not learned or fewer than `need` of the steps are +1 kill. The shapes are learned from the Kill Count alone; the
    Accuracy row's glyphs are only matched against them (-1: not a digit shape, such as the slash and the bracket)."""
    shapes = _Shapes(same)
    kills_read = [(kg, tuple(shapes.id(g[0]) for g in kg if g[1] >= 0.5)) for kg in kill_g]
    acc_read = [(ag, tuple(shapes.id(g[0], learn=False) for g in ag)) for ag in acc_g]
    runs = _runs([r for _, r in kills_read])
    digits = _learn_digits(runs)
    if digits is None:
        return None
    # in a blurred recording one digit can leave more than one shape: the others join the most alike digit
    unit = lambda s: s.ravel() / max(1e-6, float(np.linalg.norm(s)))
    for k, s in enumerate(shapes.shapes):
        if k not in digits:
            sim, best = max((float(unit(s) @ unit(shapes.shapes[d])), d) for d in list(digits))
            if sim >= 0.9:
                digits[k] = digits[best]
    # the Kill Count, stable readings only
    values = []
    for reading, a, b in runs:
        v = int("".join(str(digits[k]) for k in reading)) if all(k in digits for k in reading) else None
        if v is not None:
            values.append((v, a, b))
    steps = [b[0] - a[0] for a, b in zip(values, values[1:])]
    if len(steps) < 3:
        return None
    checked = sum(1 for s in steps if s == 1) / len(steps)
    if checked < need:
        return None
    return acc_read, digits, values, checked


def read(video, progress=None):
    """The HUD over the whole recording: dict(kills=[frame, ...] one entry per kill, shots=[frame, ...] one per shot,
    hits=[frame, ...] one per hit, final=dict(kills, hits, shots), checked=share of steps that were +1 kill), or None
    when there is no readable box."""
    lay = layout(video)
    if lay is None:
        return None
    rows, x0, x1, starts = lay
    compact = len(rows) == 5
    if compact and (starts[1] is None or starts[2] is None):
        return None
    (kill_row, ks), (acc_row, as_) = ((rows[1], starts[1]), (rows[2], starts[2])) if compact else ((rows[1], None), (rows[3], None))
    pad = 3
    kill_g, acc_g = [], []
    for i, f in enumerate(_frames(video)):
        kill_g.append(_value_glyphs(f[max(0, kill_row[0] - pad):kill_row[1] + pad, x0:x1], ks))
        acc_g.append(_value_glyphs(f[max(0, acc_row[0] - pad):acc_row[1] + pad, x0:x1], as_))
        if progress and i % 600 == 0:
            progress("reading the HUD", i, 0)
    # a very blurred upload can split one digit into two shapes at the usual likeness, and the digits are not learned;
    # then looser likenesses are tried, trusting only a Kill Count that counts up by one at almost every step (95%)
    for same, need in ((SAME, 0.8), (0.96, 0.95), (0.95, 0.95), (0.94, 0.95), (0.93, 0.95)):
        got = _count(kill_g, acc_g, same, need)
        if got:
            break
    else:
        return None
    acc_read, digits, values, checked = got
    # a lone misread between two readings that follow on (0, 9, 1: a 1 caught mid-change, 5 frames) is dropped; it
    # looked like a restart, which split the run and lost the kills before it (ww5t 2040 read 203 kills of 204). Only
    # a short one: a real restart's 0 between two 1s (1w2tes) stays up for a second or more
    keep = [i in (0, len(values) - 1) or values[i][2] - values[i][1] > 10
            or not ((not 0 <= values[i][0] - values[i - 1][0] <= 3) and 0 <= values[i + 1][0] - values[i - 1][0] <= 3)
            for i in range(len(values))]
    values = [v for v, k in zip(values, keep) if k]
    # a drop is a restart (or the end screen): the run that counts is the stretch between drops with the most kills
    cuts = [0] + [i for i in range(1, len(values)) if values[i][0] < values[i - 1][0]] + [len(values)]
    a0, a1 = max(zip(cuts, cuts[1:]), key=lambda c: (values[c[1] - 1][0] - values[c[0]][0], c[0]))
    values = values[a0:a1]
    since, until = values[0][1], values[-1][2]
    kills = []
    for (v0, _, _), (v1, a, _) in zip(values, values[1:]):
        if 0 < v1 - v0 <= 3:                                   # a bigger jump is a misread
            kills += [a] * (v1 - v0)
    # Accuracy: hits / shots. The "/" and the "(" are the tall glyphs that are not digits: hits before the first,
    # shots between them.
    acc = []
    for i, (ag, ids) in enumerate(acc_read):
        if not since <= i <= until:
            acc.append(None)
            continue
        # hits, then the "/", then shots, then the "(" (both tall glyphs that are not digits); commas and dots are short
        parts, cur = [], ""
        for (img, hshare, _), k in zip(ag, ids):
            if hshare < 0.5:
                continue
            if k in digits:
                cur += str(digits[k])
            else:
                parts.append(cur)
                cur = ""
        if len(parts) >= 2 and parts[0] and parts[1] and int(parts[0]) <= int(parts[1]):
            acc.append((int(parts[0]), int(parts[1])))
        elif len(parts) >= 2 and not parts[0] and not parts[1]:
            acc.append((0, 0))                                 # "--/-- ( %)": no shot yet
        else:
            acc.append(None)
    acc_runs = _runs(acc)
    shots, hits = [], []
    for (p0, _, _), (p1, a, _) in zip(acc_runs, acc_runs[1:]):
        if 0 <= p1[0] - p0[0] <= 50 and 0 < p1[1] - p0[1] <= 50:
            shots += [a] * (p1[1] - p0[1])
            hits += [a] * (p1[0] - p0[0])
    # the totals: the kills counted, and the fullest Accuracy reading (the HUD resets to 0 when the run ends)
    top = max((r[0] for r in acc_runs), key=lambda x: x[1], default=None)
    final = dict(kills=len(kills), hits=top[0] if top else None, shots=top[1] if top else None)
    return dict(kills=kills, shots=shots, hits=hits, final=final, checked=round(checked, 3),
                start=values[0][0], digits=len(digits))


# ---- Aim Lab ----------------------------------------------------------------------------------------------------------
# Aim Lab shows three boxes at the top centre: POINTS, TIME and ACCURACY. A hit adds points and a miss takes some off,
# so the POINTS number gives every hit and miss. The digits are learned from the TIME box, which counts down one second
# at a time (read backwards it counts up, as the Kill Count does). Shares of a 16:9 frame:
AIM_BAND = (0.30, 40 / 720, 0.565, 63 / 720)      # the value line of the POINTS and TIME boxes
AIM_SIZE = (678, 46)                              # scaled to twice 720p
AIM_POINTS, AIM_TIME = (26, 356), (368, 656)      # the two values' columns in the scaled band


def _aim_glyphs(band, cols):
    """The white value's glyphs in one box: (grey image GW x GH, height share, width share), colons left out."""
    b = band[:, cols[0]:cols[1]].astype(np.float32)
    d = b - np.median(b)
    top = float(np.percentile(d, 99.5))
    if top < 40:
        return []
    ink = d > 0.5 * top
    strength = np.clip(d / top, 0, 1).astype(np.float32)
    out, h = [], ink.shape[0]
    for a, c in _spans(ink.any(0)):
        ys = np.where(ink[:, a:c].any(1))[0]
        gh = ys.max() - ys.min() + 1
        if len(_spans(ink[:, a:c].any(1))) >= 2 and c - a <= 0.5 * gh:
            continue                                            # the colon: two dots
        img = np.asarray(Image.fromarray(strength[ys.min():ys.max() + 1, a:c]).resize((GW, GH), Image.BILINEAR))
        out.append((img, gh / h, (c - a) / gh))
    return out


def read_aimlab(video, progress=None):
    """Aim Lab's HUD: dict(kills=[frame, ...], hits, shots, final, checked, game="aimlab") as read() gives, with every
    hit counted as a kill (one-hit targets; review.py checks that against the video), or None."""
    pts_g, time_g = [], []
    for i, f in enumerate(_frames(video, box=AIM_BAND, size=AIM_SIZE, scaler="bicubic")):
        pts_g.append(_aim_glyphs(f, AIM_POINTS))
        time_g.append(_aim_glyphs(f, AIM_TIME))
        if progress and i % 600 == 0:
            progress("reading the HUD", i, 0)
    for same in (SAME, 0.96, 0.95):
        shapes = _Shapes(same)
        runs = _runs([tuple(shapes.id(g[0]) for g in tg if g[1] >= 0.5) if len(tg) == 4 else () for tg in time_g])
        digits = _learn_digits(runs[::-1])
        if digits is None:
            continue
        secs = [int("".join(str(digits[k]) for k in r)) for r, _, _ in runs if all(k in digits for k in r)]
        secs = [60 * (v // 100) + v % 100 for v in secs]
        steps = [a - b for a, b in zip(secs, secs[1:])]
        if len(steps) >= 10 and sum(1 for x in steps if x == 1) >= 0.95 * len(steps):
            break
    else:
        return None
    # POINTS: each glyph is the most alike digit shape (a minus sign is short and wide)
    unit = lambda x: x.ravel() / max(1e-6, float(np.linalg.norm(x)))
    protos = [(unit(shapes.shapes[k]), d) for k, d in digits.items()]
    vals = []
    for pg in pts_g:
        v, sign = "", 1
        for img, hshare, wshare in pg:
            if hshare < 0.5:
                if not v and wshare > 1.2:
                    sign = -1
                continue
            sim, d = max((float(unit(img) @ u), d) for u, d in protos)
            if sim < 0.9:
                v = ""
                break
            v += str(d)
        vals.append((sign * int(v),) if v else ())            # a tuple: a reading of 0 counts too
    runs = _runs(vals)
    changes = [(b[0][0] - a[0][0], b[1]) for a, b in zip(runs, runs[1:])]
    ups = collections.Counter(d for d, _ in changes if d > 0)
    if not ups:
        return None
    hit = ups.most_common(1)[0][0]
    downs = collections.Counter(d for d, _ in changes if d < 0)
    miss = downs.most_common(1)[0][0] if downs else None
    hits, misses, ok = [], [], 0
    for d, f in changes:
        best = min(((abs(d - a * hit - b * (miss or 0)), a, b) for a in range(4) for b in range(4 if miss else 1)
                    if a + b), default=None)
        if best and best[0] <= max(1, 0.15 * hit):              # a jump of two hits, or a hit and a miss, in one step
            hits += [f] * best[1]
            misses += [f] * best[2]
            ok += 1
    checked = ok / max(1, len(changes))
    if len(hits) < 10 or checked < 0.85:
        return None
    return dict(kills=hits, hits=hits, shots=sorted(hits + misses),
                final=dict(kills=len(hits), hits=len(hits), shots=len(hits) + len(misses)),
                checked=round(checked, 3), game="aimlab", points=runs[-1][0][0])


if __name__ == "__main__":
    import sys
    import time
    t = time.time()
    r = read(Path(sys.argv[1]))
    if r is None:
        print("no readable session HUD")
    else:
        print(f"{len(r['kills'])} kills, {len(r['shots'])} shots, {len(r['hits'])} hits; final {r['final']}; "
              f"start {r['start']}; +1 steps {r['checked']:.0%}; {time.time() - t:.1f} s")
