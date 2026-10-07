"""Find a recording's overlay areas (HUD boxes, clocks, a webcam) for the review app's "Exclude areas", and say what each
one is.

Finding: anything that stays put on screen while the view moves stands out from the wall behind it in nearly every
frame (old_review.contrast), the way the crosshair and HUD do in old_review.fixed_map. The run's key frames are used (or 90
frames spread over it, when it has few); pixels that stand out in 80% of them or more are fixed, and fixed pixels a few pixels apart are grouped into
one area. A webcam is found by its border: its content changes, but its edge against the game stays. The crosshair,
the fixed spot at the centre, is never an area.

Naming, in two steps (the user's plan, 2026-10-02):
1. Rules: KovaaK's session box where hud.layout finds it; Aim Lab's POINTS, TIME and ACCURACY boxes where their
   values are; a big area whose content changes is a webcam; the top centre is the timer; a top corner is the clock;
   the bottom centre is the scenario name; a bottom corner with many rows is the settings box; a tiny text in a bottom
   corner is the version; anything else is Other.
2. Learning: every area the user saves in the editor becomes an example (examples.jsonl: the area's features and the
   kind the user gave it; a found area the user removed is an example of "not an area"). Once there are examples,
   each found area takes the kind most of its 5 nearest examples have, when they agree (3 or more) and are near.
"""
import json
import math
import subprocess
import sys
import threading
from pathlib import Path

import numpy as np
from scipy import ndimage

import hud

sys.path.insert(0, str(Path(__file__).resolve().parent / "model"))
import old_review  # noqa: E402

N_SAMPLES = 90                       # frames sampled over the run when it has few key frames
FIXED = 0.8                          # a pixel standing out in this share of them is fixed
GAP = 5                              # fixed pixels this close (px at 1280 x 720) join one area
NONE = "none"                        # an area the user removed: not an area
_lock = threading.Lock()


def sample(video, n=N_SAMPLES, min_keys=24):
    """Frames spread over the run, YUV 4:2:0 at 1280 x 720 (review's format): the key frames when there are min_keys
    or more (33 on a 66 s KovOBS run, decoded in 0.1 s), else n frames from decoding the whole run (9 s on a 1440p
    120 fps AV1 run)."""
    keys = list(old_review._frames(str(video), keyframes=True))
    if len(keys) >= min_keys:
        return keys
    fps, dur = old_review.probe(str(video))
    p = subprocess.Popen(["ffmpeg", "-v", "error", "-i", str(video), "-vf",
                          f"fps={n / max(1.0, dur - 1):.5f},scale={old_review.W}:{old_review.H}:flags=area,format=yuv420p",
                          "-f", "rawvideo", "-"], stdout=subprocess.PIPE, bufsize=0)
    return list(old_review._read(p, old_review.FRAME))


def features(box, stand, change):
    """What an area looks like, for the learner: where it is and how big (shares of the frame), how much of it is
    fixed, how much it changes over the run, how many text rows it has."""
    x0, y0, x1, y1 = (int(round(v * s)) for v, s in zip(box, (old_review.W, old_review.H, old_review.W, old_review.H)))
    st, ch = stand[y0:y1, x0:x1], change[y0:y1, x0:x1]
    fixed = st >= FIXED
    rows = len(review_rows(fixed[:, 6:-6] if fixed.shape[1] > 16 else fixed))   # inside a box's border
    return [round(float(v), 4) for v in ((box[0] + box[2]) / 2, (box[1] + box[3]) / 2, box[2] - box[0], box[3] - box[1],
                                         fixed.mean() if fixed.size else 0, ch.mean() / 40 if ch.size else 0,
                                         min(rows, 12) / 12)]


def review_rows(fixed):
    """Text rows in an area: runs of rows with fixed pixels, at least 3 px tall."""
    on = fixed.sum(1) > 1
    out, cur = [], None
    for i, v in enumerate(on):
        if v and cur is None:
            cur = i
        elif not v and cur is not None:
            if i - cur >= 3:
                out.append((cur, i))
            cur = None
    if cur is not None and len(on) - cur >= 3:
        out.append((cur, len(on)))
    return out


def rule_kind(box, feat, session_box, aim):
    """Step 1: the kind from where the area is and what it does."""
    cx, cy, w, h, fixed, change, rows = feat
    iou = lambda a, b: max(0.0, min(a[2], b[2]) - max(a[0], b[0])) * max(0.0, min(a[3], b[3]) - max(a[1], b[1])) / \
        max(1e-9, (a[2] - a[0]) * (a[3] - a[1]))
    if session_box and iou(box, session_box) > 0.5:
        return "Session stats"
    if aim:
        for b, kind in aim:
            if iou(box, b) > 0.3:
                return kind
    if w * h > 0.015 and fixed < 0.2 and rows * 12 <= 2:   # big, its content does not stay put (a hand, moving or
        return "Webcam"                                     # still: only its border is fixed), and not rows of text
    if cy < 0.15 and 0.35 < cx < 0.65:
        return "Timer"
    if cy < 0.18 and (cx > 0.8 or cx < 0.2):
        return "Clock"
    if cy > 0.85 and 0.3 < cx < 0.7:
        return "Scenario name"
    if cy > 0.75 and (cx < 0.35 or cx > 0.65) and rows * 12 >= 3:
        return "Settings"
    if cy > 0.9 and w * h < 0.002 and (cx < 0.1 or cx > 0.9):
        return "Version"
    return "Other"


def analyse(video, with_maps=False):
    """The found areas: [dict(box=[x0, y0, x1, y1] shares, feat=[...], rule=kind)]; with_maps: also the stand-out and
    change maps they came from (to describe any area the user draws)."""
    fr = sample(video)
    stand = np.mean([old_review.contrast(f) > old_review.DIFF for f in fr], axis=0)
    ys = [np.frombuffer(f, np.uint8)[:old_review.W * old_review.H].reshape(old_review.H, old_review.W).astype(np.int16) for f in fr]
    change = np.mean([np.abs(a - b) for a, b in zip(ys, ys[1:])], axis=0)
    fixed = stand >= FIXED
    if fixed.mean() > 0.15:                             # the view hardly moved (a probe run): the room itself stays
        return ([], stand, change) if with_maps else []  # put, and overlays cannot be told from it
    # KovaaK's session box (hud.layout) and Aim Lab's boxes, where they are
    session_box, aim = None, None
    lay = hud.layout(video)
    if lay:
        rows, x0, x1, _ = lay
        sx, sy = hud.BOX[2] / hud.BW, hud.BOX[3] / hud.BH
        pad = 8 / old_review.W                              # the box's own border, round the rows hud.layout reads
        session_box = [max(0.0, x0 * sx - pad), max(0.0, (rows[0][0] - 8) * sy - pad), x1 * sx + pad,
                       (rows[-1][1] + 8) * sy + pad]
    grown = ndimage.binary_dilation(ndimage.binary_closing(fixed, iterations=GAP), iterations=GAP // 2)
    if session_box:                                     # cut out, so a clock beside it is an area of its own
        a, b, c, d = (int(round(v * s)) for v, s in zip(session_box, (old_review.W, old_review.H, old_review.W, old_review.H)))
        grown[max(0, b - 4):d + 4, max(0, a - 4):c + 4] = False
    lab, n = ndimage.label(grown)
    band = hud.AIM_BAND
    aim_cols = [(hud.AIM_POINTS, "Session stats"), (hud.AIM_TIME, "Timer")]
    yb0, yb1 = int(band[1] * old_review.H), int(band[3] * old_review.H)
    sxa = (band[2] - band[0]) / hud.AIM_SIZE[0]
    if fixed[yb0:yb1].any():
        aim = []
        for (c0, c1), kind in aim_cols:
            b = [band[0] + c0 * sxa, band[1], band[0] + c1 * sxa, band[3]]
            xa, xb = int(b[0] * old_review.W), int(b[2] * old_review.W)
            if stand[yb0:yb1, xa:xb].max() >= FIXED:
                aim.append((b, kind))
        aim = aim if len(aim) == 2 else None
        if aim:                                         # ACCURACY: the third box, as wide as POINTS, after TIME
            w = aim[0][0][2] - aim[0][0][0]
            aim.append(([aim[1][0][2] + 0.005, band[1], aim[1][0][2] + 0.005 + w, band[3]], "Session stats"))
    boxes = []
    for s in ndimage.find_objects(lab):
        y0, y1, x0, x1 = s[0].start, s[0].stop, s[1].start, s[1].stop
        if (x1 - x0) * (y1 - y0) < 80 or min(x1 - x0, y1 - y0) < 7 or not fixed[y0:y1, x0:x1].any():
            continue                                    # too small, or a sliver of a box's border
        if x0 <= old_review.CX <= x1 and y0 <= old_review.CY <= y1 and (x1 - x0) < 120:
            continue                                    # the crosshair: never an area
        boxes.append([x0 / old_review.W, y0 / old_review.H, x1 / old_review.W, y1 / old_review.H])
    if session_box:                                     # the session box as found by hud.layout, whole
        boxes = [b for b in boxes if _inside(b, session_box) < 0.6] + [session_box]
    # an area mostly inside a bigger one (text inside a box, a webcam's details) is part of it
    boxes = [b for b in boxes if b is session_box or
             not any(o is not b and _area(o) > _area(b) and _inside(b, o) > 0.8 for o in boxes)]
    out = []
    for box in boxes:
        feat = features(box, stand, change)
        kind = "Zoomed crosshair" if zoomed(box, ys) else rule_kind(box, feat, session_box, aim)
        out.append(dict(box=[round(v, 4) for v in box], feat=feat, rule=kind))
    return (out, stand, change) if with_maps else out


def zoomed(box, ys, size=16):
    """A magnified copy of the screen round the crosshair (a crosshair zoom): over the sampled frames, the area's
    picture follows the centre's, scaled down by some zoom. True when one zoom correlates 0.8 or more."""
    x0, y0, x1, y1 = (int(round(v * s)) for v, s in zip(box, (old_review.W, old_review.H, old_review.W, old_review.H)))
    w, h = x1 - x0, y1 - y0
    if w < 24 or h < 24 or len(ys) < 10:
        return False
    from PIL import Image
    small = lambda a: np.asarray(Image.fromarray(a.astype(np.uint8)).resize((size, size), Image.BILINEAR), np.float32)
    area = np.stack([small(y[y0:y1, x0:x1]) for y in ys]).ravel()
    area = (area - area.mean()) / (area.std() + 1e-6)
    cx, cy = int(old_review.CX), int(old_review.CY)
    for zoom in (1.5, 2, 3, 4, 6, 8):
        hw, hh = max(4, int(w / zoom / 2)), max(4, int(h / zoom / 2))
        if cx - hw < 0 or cy - hh < 0:
            continue
        mid = np.stack([small(y[cy - hh:cy + hh, cx - hw:cx + hw]) for y in ys]).ravel()
        mid = (mid - mid.mean()) / (mid.std() + 1e-6)
        if float((area * mid).mean()) >= 0.8:
            return True
    return False


def _area(b):
    return (b[2] - b[0]) * (b[3] - b[1])


def _inside(b, o):
    """The share of box b inside box o."""
    inter = max(0.0, min(b[2], o[2]) - max(b[0], o[0])) * max(0.0, min(b[3], o[3]) - max(b[1], o[1]))
    return inter / max(1e-9, _area(b))


# ---- step 2: learning from the user's saved areas ---------------------------------------------------------------------
def load_examples(path):
    if not Path(path).exists():
        return []
    return [json.loads(line) for line in Path(path).read_text(encoding="utf-8").splitlines() if line.strip()]


def learn(path, rec, found, saved, maps=None):
    """The user saved `saved` areas for recording `rec`, after `found` were proposed (or found now). With the
    recording's maps (cached by find()), every saved area is an example of its kind, drawn by hand or not, and each
    found area that lies in no saved one an example of "none" (the user removed it). Without them, each found area
    takes the kind of the saved area that fits it best. Replaces the recording's earlier examples."""
    ex = []
    used = set()
    if maps is not None:
        stand, change = maps
        for b in saved:
            ex.append(dict(rec=rec, feat=features(b[:4], stand, change), kind=b[4] if len(b) > 4 else "other"))
        for a in found:
            if not any(_inside(a["box"], b[:4]) > 0.5 for b in saved):
                ex.append(dict(rec=rec, feat=a["feat"], kind=NONE))
        found = []
    for a in found:
        bx = a["box"]
        best = None
        for j, s in enumerate(saved):                   # areas can overlap: the one that fits it best, not the
            inter = max(0.0, min(bx[2], s[2]) - max(bx[0], s[0])) * max(0.0, min(bx[3], s[3]) - max(bx[1], s[1]))
            share = inter / max(1e-9, (bx[2] - bx[0]) * (bx[3] - bx[1]))     # biggest that holds it
            if share > 0.5 and (best is None or iou(bx, s[:4]) > best[0]):
                best = (iou(bx, s[:4]), j)
        if best:
            used.add(best[1])
        ex.append(dict(rec=rec, feat=a["feat"], kind=saved[best[1]][4] if best and len(saved[best[1]]) > 4 else
                       ("Other" if best else NONE)))
    with _lock:
        keep = [e for e in load_examples(path) if e["rec"] not in (rec, "kovobs:" + rec)]   # the user's labels
                                                                                         # replace the layout's
        Path(path).parent.mkdir(parents=True, exist_ok=True)
        with open(path, "w", encoding="utf-8") as fh:
            for e in keep + ex:
                fh.write(json.dumps(e) + "\n")
    return len(ex)


def predict(found, examples, k=5):
    """Step 2: each found area's kind from its k nearest examples (other recordings' saved areas), when 3 or more
    agree and they are near; else the rule's kind. "none": the user removes such areas, so they are left out."""
    if len(examples) < k:
        return [dict(a, kind=a["rule"], by="rule") for a in found]
    X = np.array([e["feat"] for e in examples], np.float32)
    y = [e["kind"] for e in examples]
    out = []
    for a in found:
        d = np.linalg.norm(X - np.array(a["feat"], np.float32), axis=1)
        idx = np.argsort(d)[:k]
        votes = {}
        for i in idx:
            votes[y[i]] = votes.get(y[i], 0) + 1
        kind, n = max(votes.items(), key=lambda kv: kv[1])
        if n >= 3 and float(np.median(d[idx])) < 0.08:
            out.append(dict(a, kind=kind, by="learned"))
        else:
            out.append(dict(a, kind=a["rule"], by="rule"))
    return [a for a in out if a["kind"] != NONE]


def iou(a, b):
    inter = max(0.0, min(a[2], b[2]) - max(a[0], b[0])) * max(0.0, min(a[3], b[3]) - max(a[1], b[1]))
    return inter / max(1e-9, _area(a) + _area(b) - inter)


def same_layout(found, other):
    """How alike two recordings' overlays are: each found area's best overlap with the other's, averaged both ways
    (1: the same areas in the same places)."""
    if not found or not other:
        return 0.0
    one = lambda xs, ys: sum(max(iou(x["box"], y["box"]) for y in ys) for x in xs) / len(xs)
    return min(one(found, other), one(other, found))


def find(video, cache, examples_path, labelled=()):
    """The areas to propose: found (cached per recording in cache/areas.json). When the found areas sit where those of
    a recording the user already labelled do (same_layout 0.5 or more), the user's own areas for that recording, as
    they drew and named them: most recordings share one layout, and redoing it each time was the user's complaint
    (2026-10-02). Else the found areas, named by the learner or the rules. labelled: (recording, its found areas, its
    saved areas). Returns ([[x0, y0, x1, y1, kind], ...], found, the recording copied from or None)."""
    p = Path(cache) / "areas.json"
    found = json.load(open(p)) if p.exists() else None
    if found is None:
        found, stand, change = analyse(video, with_maps=True)
        Path(cache).mkdir(parents=True, exist_ok=True)
        json.dump(found, open(p, "w"))
        np.savez_compressed(Path(cache) / "areas_maps.npz", stand=np.round(stand * 255).astype(np.uint8),
                            change=np.clip(change, 0, 255).astype(np.uint8))
    best = max(((same_layout(found, f), rec, saved) for rec, f, saved in labelled), default=(0, None, None),
               key=lambda t: t[0])
    if best[0] >= 0.5:
        return [list(b[:4]) + [b[4] if len(b) > 4 else "Other"] for b in best[2]], found, best[1]
    named = predict(found, load_examples(examples_path))
    find.by = {k: sum(1 for a in named if a["by"] == k) for k in ("learned", "rule")}
    return [a["box"] + [a["kind"]] for a in named], found, None


def maps(video, cache):
    """The recording's stand-out and change maps (cache/areas_maps.npz), made by analysing it when missing."""
    p = Path(cache) / "areas_maps.npz"
    if not p.exists():
        found, stand, change = analyse(video, with_maps=True)
        Path(cache).mkdir(parents=True, exist_ok=True)
        np.savez_compressed(p, stand=np.round(stand * 255).astype(np.uint8),
                            change=np.clip(change, 0, 255).astype(np.uint8))
        if not (Path(cache) / "areas.json").exists():
            json.dump(found, open(Path(cache) / "areas.json", "w"))
    z = np.load(p)
    return z["stand"].astype(np.float32) / 255, z["change"].astype(np.float32)


def check(path):
    """Leave one recording out: each example's kind predicted from the other recordings' examples. Returns (share the
    learner names, share of those right, count, confusions {(truth, guess): n}); "?" is not sure (the rules decide)."""
    ex = load_examples(path)
    right, sure, wrong = 0, 0, {}
    for rec in {e["rec"] for e in ex}:
        rest = [e for e in ex if e["rec"] != rec]
        for e in (e for e in ex if e["rec"] == rec):
            p = predict([dict(box=None, feat=e["feat"], rule="?")], rest)
            k = p[0]["kind"] if p else NONE
            sure += k != "?"
            if k == e["kind"]:
                right += 1
            elif k != "?":
                wrong[(e["kind"], k)] = wrong.get((e["kind"], k), 0) + 1
    return sure / max(1, len(ex)), right / max(1, sure), len(ex), wrong


if __name__ == "__main__":
    import argparse
    import sys
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    import aimview_tools
    ap = argparse.ArgumentParser(description="bootstrap: learn the user's KovOBS layout from N of their recordings "
                                             "(one per scenario); check: leave-one-recording-out accuracy")
    ap.add_argument("what", choices=("bootstrap", "check"))
    ap.add_argument("--n", type=int, default=30)
    a = ap.parse_args()
    if a.what == "bootstrap":
        lib = aimview_tools.Library()
        seen, picked = set(), []
        for v in lib.list():
            sc = v["id"].split("/")[0]
            if v.get("uploaded") or sc in seen or "Probe" in sc:      # probes: the view hardly moves
                continue
            seen.add(sc)
            picked.append(v["id"])
            if len(picked) >= a.n:
                break
        for i, vid in enumerate(picked):
            found = find(str(lib.resolve(vid)), lib.cache_dir(vid), aimview_tools.AREA_EXAMPLES)[1]  # noqa
            n = learn(aimview_tools.AREA_EXAMPLES, "kovobs:" + vid, found, old_review.OVERLAY_SHARES)
            print(f"[{i + 1}/{len(picked)}] {vid[:60]}: {n} examples", flush=True)
    sure, acc, n, wrong = check(aimview_tools.AREA_EXAMPLES)
    print(f"leave one recording out, {n} examples: the learner names {sure:.0%}, {acc:.0%} of those right; "
          f"wrong (truth, guess): {wrong}")
