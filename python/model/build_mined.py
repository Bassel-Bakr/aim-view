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
(old_review.match) end, or in a tracking run one held near the crosshair a fifth of the time (a cloud, a name tag or a wall
seam the model boxes steadily is neither).
- kill: a kill of a dynamic or switching run, matched as the core matches them (old_review.match: the clock offset voted
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
import old_review  # noqa: E402

WIDTH, HEIGHT, CROP = old_review.W, old_review.H, 256
MODEL = HERE / "exports" / "detector_full_v3_u8in.onnx"
THRESHOLD = 0.3                              # full_v3's, in its settings file
RULES = ("kill", "gap", "false_static", "false_lone")
CODE = dict(kill="k", gap="g", false_static="s", false_lone="l")
PER_KILL = 4                                 # crops per kill at most
PER_RUN = dict(kill=80, gap=40, false_static=12, false_lone=20)   # crops per recording and rule at most
SPLITS = ("train", "val", "test")

# A placed box must show (disc, shows): its middle is a disc half its smaller side across (at least MIN_RADIUS_PX), the
# wall round it a ring of RING_IN to RING_OUT times that radius, with MIN_VISIBLE and MIN_RING of their pixels off the
# fixed map, in a window MIN_WINDOW_PX a side at least.
MIN_RADIUS_PX, RING_IN, RING_OUT = 1.5, 1.6, 2.6
MIN_VISIBLE, MIN_RING, MIN_WINDOW_PX = 3, 6, 3
MIN_CONTRAST = 40                            # the middle differs from the wall by this at least
PLAIN_WITHIN, PLAIN_SHARE = 40, 0.8          # a plain wall: this share of the ring within this of its median
SAME_COLOR = 45                              # the middle within this of the color the target had where it was seen

# Run: the run's window, its clock and its verified chains
AREA_MARGIN_PX = 24                          # false_lone's boxes are this far from the excluded areas at least
MATCHED_SHARE = 0.8                          # the kills' clock counts when this share of the stats file's kills matched
CLOCK_FRAMES = 2.5                           # and KovaaK's countdown ends within this many frames of it,
CONFIRMED_SHARE = 0.8                        # or this share of the matched kills is confirmed
EDGE_S = 0.5                                 # the window starts and ends this far inside the run
HELD_MIN_S, HELD_DEG, HELD_SIZE, HELD_SHARE = 0.5, 1.0, 0.75, 0.2   # tracking: a target held near the crosshair
SPOT_DEG = 0.2                               # a place this near a crosshair spot is on it
SAME_SPOT_DEG = 0.2                          # false_static: a chain this near a spot found before joins it
STEADY_MIN_S, STEADY_HALF_S, STEADY_SHARE = 0.1, 0.125, 0.8         # a chain seen steadily round a frame
EDGE_PX = 8                                  # a placed box's center is this far inside the frame

# kill: the third of a second before the kill, up to 2 frames before it
KILL_WINDOW_PER_S = 3                        # the window: a second over this
KILL_END_FRAMES = 1                          # frames before the kill the window stops short of (kill frame - 1)
AHEAD_MIN_FRAMES, AHEAD_S = 2, 0.05          # past the last sighting, by its own speed
BRIDGE_MIN_FRAMES, BRIDGE_S = 2, 0.06        # between two sightings
SEEN_SHARE = 0.8                             # seen in this share of the frames before it was lost
SIZE_BEFORE, SIZE_AFTER = 5, 3               # sightings the size is the median of
SPEED_FRAMES, MIN_SPEED_POINTS = 4, 2        # its own speed: over the sightings in the 4 frames before the last
CLOCK_SLACK_FRAMES = 2                       # the speed leads it onto the crosshair within this many frames of the kill
ONTO_SLACK_DEG = 0.3
FREE_MIN_DEG, FREE_SIZE = 0.3, 0.6           # no box of the review within max(0.3 deg, 0.6 sizes) of a placed box

# gap: missed for 1 or 2 frames, seen steadily either side
GAP_FRAMES = (1, 2)
SIDE_MIN_FRAMES, SIDE_PER_S = 4, 30          # seen 4 frames in a row either side (1/30 s at higher frame rates)
KILL_CLEAR_FRAMES = 5                        # not within this many frames of a kill
GAP_STEADY_SHARE, GAP_STEADY_HALF_S = 0.9, 0.25
SIZE_RATIO_MIN, SIZE_RATIO_MAX = 0.7, 1.43   # its size either side agrees
TINY_AREA = 1e-6
SPEED_SPAN = 3                               # frames its speed either side is measured over
AGREE_MIN_DEG, AGREE_SIZE = 0.15, 0.35       # its speed either side leads it to the other side within this

# false_static: a box that stays put on screen while the view turns
LINK_DEG = 0.08                              # boxes linked frame to frame by their place on screen within this
STATIC_MIN_FRAMES, STATIC_MIN_S = 8, 0.1
STILL_DEG, STILL_NEAR_DEG, NEAR_CROSSHAIR_DEG = 0.1, 0.03, 1.5   # stays within this (the second near the crosshair)
MIN_TURN_DEG = 1.0                           # while the view turns this much
TURNING_DEG = 0.02                           # the camera turning at that frame
CLEAR_DEG, CLEAR_FRAMES = 2.0, 5             # no other box within 2 deg in the 5 frames either side
SPOT_SELF_DEG = 0.15                         # a box this near the spot is the spot's own
PER_SPOT = 3                                 # frames per spot on screen at most

# false_lone: a box in a single frame
LONE_FRAMES = 3                              # nothing near it in the 3 frames either side
LONE_MIN_DEG = 2.0                           # 2 deg or more from the crosshair
STEADY_VIEW_DEG = 0.5                        # the camera's reading and the tracks' shift under this a frame

# decoding and lining up
JOIN_FRAMES = 8                              # frames this close are decoded in one window
MAX_WINDOWS = 120                            # ffmpeg's expressions stay short
LINE_UP_TURN_DEG = 0.03                      # frames where the camera turns, so a frame off shows
LINE_UP_DEG = 0.15                           # a review box found again within this
LINE_UP_MIN_BOXES, LINE_UP_SHARE, LINE_UP_MARGIN = 20, 0.8, 0.05

# the image tests and the crops
OVERSAMPLE = 2                               # the image tests drop some places: sample twice the cap first
MIN_KEY_FRAMES = 3
SAME_CENTER, SAME_SLACK_PX, SAME_AREA_MIN, SAME_AREA_MAX = 0.3, 1, 0.6, 1.67   # the export's own box of a placed one
TOUCH_PX = 2                                 # another box touching a placed one
STATIC_PATCH_MARGIN_PX = 3
SAME_IMAGE = 10                              # false_static: the image the same (mean difference under this)
LONE_CLEAR_PX, LONE_CLEAR_SIZES = 24.0, 4    # false_lone: no box of the export within this (or 4 box sizes)
LONE_PATCH_MIN_PX, LONE_PATCH_MARGIN_PX = 4, 2
NEAR_REACH_PX, FAR_REACH_PX = 48, 64         # the patches looked for a match in the frames 1 and 3 away
LIKE_PATCH = 20                              # a patch matching to this mean difference is like it
EXPORT_MATCH_PX = 2.0                        # the export's box of a review box
TAKE_OUT_MIN_PX = 3.0                        # a box taken out removes the review's boxes within this (or half its size)
JITTER_PX = 48                               # a crop's corner moves up to this far at random
REASON_CHARS = 160
FOLDER_CHARS = 38


def px_size(x, y, width_deg, height_deg):
    """A box's width and height in pixels from its size in degrees at (x, y) (track.rs keep's inverse, to first
    order)."""
    left, _ = old_review.to_px(x - width_deg / 2, y)
    right, _ = old_review.to_px(x + width_deg / 2, y)
    _, top = old_review.to_px(x, y + height_deg / 2)
    _, bottom = old_review.to_px(x, y - height_deg / 2)
    return right - left, bottom - top


def disc(rgb, fixed, cx, cy, width, height):
    """The median color of a target's middle (a disc half its smaller side across, the fixed pixels left out), of the
    wall round it (a ring 1.6 to 2.6 times that radius), and the share of the ring within 40 of the ring's median, or
    None where too little of either shows."""
    radius = max(MIN_RADIUS_PX, 0.5 * min(width, height))
    reach = int(math.ceil(RING_OUT * radius)) + 1
    top, bottom = max(0, int(cy) - reach), min(HEIGHT, int(cy) + reach + 1)
    left, right = max(0, int(cx) - reach), min(WIDTH, int(cx) + reach + 1)
    if bottom - top < MIN_WINDOW_PX or right - left < MIN_WINDOW_PX:
        return None
    yy, xx = np.mgrid[top:bottom, left:right]
    distance2 = (xx - cx) ** 2 + (yy - cy) ** 2
    free = fixed[top:bottom, left:right] == 0
    visible = (distance2 <= radius * radius) & free
    ring = (distance2 >= (RING_IN * radius) ** 2) & (distance2 <= (RING_OUT * radius) ** 2) & free
    if visible.sum() < MIN_VISIBLE or ring.sum() < MIN_RING:
        return None
    pixels = rgb[top:bottom, left:right].astype(np.float32)
    outer = np.median(pixels[ring], 0)
    return (np.median(pixels[visible], 0), outer,
            float(np.mean(np.linalg.norm(pixels[ring] - outer, axis=1) < PLAIN_WITHIN)))


def shows(rgb, fixed, cx, cy, width, height, ref=None):
    """Does a target show at (cx, cy), alone: its middle unlike the wall round it (by 40 or more, as build_kills.py
    checks), the wall round it plain (8 in 10 of its pixels within 40 of its median: no other target touching it),
    and its middle within 45 of the color `ref` the target had where it was seen? Returns its middle's color, or
    None."""
    colors = disc(rgb, fixed, cx, cy, width, height)
    if colors is None or np.linalg.norm(colors[0] - colors[1]) < MIN_CONTRAST or colors[2] < PLAIN_SHARE:
        return None
    if ref is not None and np.linalg.norm(colors[0] - ref) > SAME_COLOR:
        return None
    return colors[0]


def patch(rgb, cx, cy, half):
    x, y = int(round(cx)), int(round(cy))
    if x - half < 0 or y - half < 0 or x + half + 1 > WIDTH or y + half + 1 > HEIGHT:
        return None
    return rgb[y - half:y + half + 1, x - half:x + half + 1].astype(np.float32)


def best_match(template, rgb, cx, cy, half, reach):
    """The smallest mean absolute difference between the patch `template` and the patches of rgb centered within
    `reach` px of (cx, cy) (every second pixel), or None where none fits in the frame."""
    x, y = int(round(cx)), int(round(cy))
    region = rgb[max(0, y - reach - half):min(HEIGHT, y + reach + half + 1),
                 max(0, x - reach - half):min(WIDTH, x + reach + half + 1)].astype(np.float32)
    side = 2 * half + 1
    if region.shape[0] < side or region.shape[1] < side:
        return None
    windows = np.lib.stride_tricks.sliding_window_view(region, (side, side, 3))[::2, ::2, 0]
    return float(np.abs(windows - template).mean(axis=(2, 3, 4)).min())


class Run:
    """One recording's review: its tracks as places in degrees, their chains (the tracks appearances() joins: one
    target picked up again), the camera's turn, the run's frames."""

    def __init__(self, folder, kind, stats):
        self.tracks = json.loads((folder / "tracks.json").read_text(encoding="utf-8"))
        readings = json.loads((folder / "readings.json").read_text(encoding="utf-8"))
        self.fps, frames = self.tracks["fps"], self.tracks["frames"]
        self.frame_count = len(frames)
        self._read_boxes(frames)
        self._read_camera(readings, frames)
        self.mask = old_review.mask_of(self.tracks.get("areas") or old_review.OVERLAY_SHARES)
        self.far = ndimage.binary_erosion(self.mask, iterations=AREA_MARGIN_PX)   # 24 px or more from the areas
        self._join_chains()
        self.spots = old_review.crosshair_spots(frames)      # where the model marks the crosshair itself
        self.kind, self.stats = kind, stats
        self.meta, self.rows = old_review.load_stats(stats)
        self.flicks, self.info = [], {}
        on = [i for i, counting in enumerate(readings.get("countdown") or []) if counting]
        countdown = (on[-1] + 1) / self.fps if on else None
        start = self._line_up_kills(countdown)
        if start is None and countdown is not None:
            start, self.start_from = countdown, "countdown"
        self._set_window(start)
        self.kill_frames = [flick["stats_frame"] for flick in self.flicks]
        self._verify()

    def _read_boxes(self, frames):
        self.boxes = []                              # per frame: (track, x, y, wd, hd, score)
        self.places = {}
        for frame in frames:
            sizes = frame.get("wh") or [(0.0, 0.0)] * len(frame["t"])
            scores = frame.get("s") or [1.0] * len(frame["t"])
            self.boxes.append([(track, x, y, width, height, score)
                               for (track, x, y), (width, height), score in zip(frame["t"], sizes, scores)])
            for track, x, y in frame["t"]:
                self.places.setdefault(track, {})[frame["i"]] = (x, y)

    def _read_camera(self, readings, frames):
        camera = np.zeros((self.frame_count, 2))
        self.camera_ok = np.zeros(self.frame_count, bool)
        for i, reading in enumerate((readings.get("camera") or [])[:self.frame_count]):
            if reading is not None:
                camera[i], self.camera_ok[i] = reading[:2], True
        self.camera = camera
        # the tracks' own shift
        self.track_shift = np.array([frame.get("shift") or (0.0, 0.0) for frame in frames], float).reshape(-1, 2)
        self.turn_sum = np.cumsum(camera, axis=0)

    def _join_chains(self):
        _, self.follows = old_review.appearances(self.tracks)
        before = {after: track for track, after in self.follows.items()}
        self.root, self.chain = {}, collections.defaultdict(set)
        for track, seen in self.places.items():
            root = track
            while root in before:
                root = before[root]
            self.root[track] = root
            self.chain[root] |= set(seen)
        self.span = {root: (min(frames), max(frames)) for root, frames in self.chain.items()}

    def _line_up_kills(self, countdown):
        """The run's start by the kills' clock (old_review.match), or None; sets the flicks, the match's info and
        whether the clock is sure."""
        self.start_from, self.clock = None, False
        if self.kind == "tracking" or not self.rows:
            return None
        self.flicks, self.info = old_review.match(self.tracks, self.stats)
        info = self.info
        if info.get("offset") is None or info["matched"] < MATCHED_SHARE * len(self.rows):
            self.flicks = []
            return None
        start, self.start_from = float(info["offset"]), "kills"
        # the kills' clock is trusted only where KovaaK's countdown ends on the same frame (within 2), or where 8 in
        # 10 kills are confirmed (the target last seen at the crosshair within 2 frames of the kill): the vote can
        # line the kills up with the crosshair's own boxes, which the model marks once the target is gone (Pasu
        # Voltaic Reload Easy: 7 frames late)
        self.clock = (countdown is not None and abs(start - countdown) * self.fps <= CLOCK_FRAMES) or \
            info["confirmed"] >= CONFIRMED_SHARE * info["matched"]
        if countdown is not None:
            self.info["countdown"] = countdown
        return start

    def _set_window(self, start):
        length = old_review.stats_length(self.stats)
        self.window = None
        if start is not None and length:
            low = int(round((start + EDGE_S) * self.fps))
            high = min(self.frame_count - 1, int(round((start + length - EDGE_S) * self.fps)))
            if high - low > self.fps:
                self.window = (low, high)

    def _verify(self):
        """The chains that are targets for sure: a clicking run's killed ones, a tracking run's held near the
        crosshair a fifth of the time or more (a cloud, a name tag or a wall seam the model boxes steadily is not)."""
        self.verified = set()
        at = {(i, x, y): track for i, boxes in enumerate(self.boxes) for track, x, y, *_ in boxes}
        for flick in self.flicks:
            for i, x, y in flick["traj"]:
                if (i, x, y) in at and not self.on_spot(x, y):
                    self.verified.add(self.root[at[(i, x, y)]])
        if self.kind != "tracking":
            return
        points = collections.defaultdict(list)
        for boxes in self.boxes:
            for track, x, y, width, height, _ in boxes:
                points[self.root[track]].append((x, y, width, height))
        for root, seen in points.items():
            seen = np.array(seen)
            size = float(np.median(np.maximum(seen[:, 2], seen[:, 3])))
            if len(seen) >= HELD_MIN_S * self.fps and \
                    np.mean(np.hypot(seen[:, 0], seen[:, 1]) <= max(HELD_DEG, HELD_SIZE * size)) >= HELD_SHARE:
                self.verified.add(root)

    def inside(self, i):
        return self.window is not None and self.window[0] <= i <= self.window[1]

    def turned(self, start, end):
        """The camera's turn from frame start to frame end (degrees), or None where a reading is missing."""
        low, high = min(start, end), max(start, end)
        if high > low and not self.camera_ok[low + 1:high + 1].all():
            return None
        return self.turn_sum[end] - self.turn_sum[start]

    def box_of(self, i, x, y):
        """The review's box at (x, y) in frame i: (wd, hd, score)."""
        for _, box_x, box_y, width, height, score in self.boxes[i]:
            if box_x == x and box_y == y:
                return width, height, score
        return None

    def on_spot(self, x, y):
        return any(math.hypot(x - spot_x, y - spot_y) < SPOT_DEG for spot_x, spot_y in self.spots)

    def near(self, i, x, y, radius):
        return any(math.hypot(box_x - x, box_y - y) < radius for _, box_x, box_y, *_ in self.boxes[i])

    def trusted(self, track, i, x, y):
        """Is the review's box of the track at (x, y) in frame i a target to keep as a label: a verified chain's, seen
        steadily, and not on a spot where the model marks the crosshair?"""
        return self.root[track] in self.verified and not self.on_spot(x, y) and self.steady(track, i)

    def steady(self, track, i, share=STEADY_SHARE, half=None):
        """Is the track's target seen steadily round frame i: its chain lasts 0.1 s or more and is seen in `share` of
        the frames it lives within `half` frames (0.125 s) of i? A box the model gives now and then (a wall seam, a
        name tag) is not."""
        root = self.root[track]
        first, last = self.span[root]
        if last - first + 1 < round(STEADY_MIN_S * self.fps):
            return False
        half = half or int(round(STEADY_HALF_S * self.fps))
        low, high = max(first, i - half), min(last, i + half)
        return sum(k in self.chain[root] for k in range(low, high + 1)) >= share * (high - low + 1)

    def placeable(self, x, y):
        cx, cy = old_review.to_px(x, y)
        return EDGE_PX <= cx < WIDTH - EDGE_PX and EDGE_PX <= cy < HEIGHT - EDGE_PX and self.mask[int(cy), int(cx)]


def size_of(run, places, frames):
    """A target's box (wd, hd in degrees; w, h in pixels): the median over the given frames of its track's places."""
    got = []
    for i in frames:
        box = run.box_of(i, *places[i])
        if box and box[0] > 0:
            got.append((box[0], box[1], *px_size(places[i][0], places[i][1], box[0], box[1])))
    return tuple(np.median(np.array(got), 0)) if got else None


def free_place(run, i, x, y, size):
    """No box of the review there or near, and the place where a box can be."""
    width_deg, height_deg = size[0], size[1]
    return not run.near(i, x, y, max(FREE_MIN_DEG, FREE_SIZE * max(width_deg, height_deg))) and run.placeable(x, y)


def onto_crosshair(run, places, last, speed, size, kill_frame):
    """Does the target's own speed over the world, from its last sighting, lead it onto the crosshair at the kill
    (the clock: within 2 frames)?"""
    radius = 0.5 * max(size[0], size[1])
    onto = False
    for frame in range(kill_frame - CLOCK_SLACK_FRAMES, kill_frame + CLOCK_SLACK_FRAMES + 1):
        turn = run.turned(last, min(frame, run.frame_count - 1))
        if turn is not None and math.hypot(*(np.array(places[last]) + turn + speed * (frame - last))) \
                <= radius + ONTO_SLACK_DEG:
            onto = True
    return onto


def kill_place(run, flick, places, i, size_frames):
    """A frame's place for the killed target where the model lost it: between two sightings, or past the last one by
    its own speed. size_frames: the sightings its size is measured on. Returns (x, y, size, last sighting, why) or
    None."""
    seen = sorted(places)
    last = max(j for j in seen if j < i)
    after = [j for j in seen if j > i]
    next_seen = after[0] if after else None
    bridged = next_seen is not None and next_seen - last - 1 <= max(BRIDGE_MIN_FRAMES, int(round(BRIDGE_S * run.fps)))
    size = size_of(run, places, size_frames(seen, i, after, bridged))
    if size is None:
        return None
    turn = run.turned(last, i)
    if turn is None:
        return None
    if bridged:
        if run.turned(last, next_seen) is None:
            return None
        part = (i - last) / (next_seen - last)
        world_last = np.array(places[last]) - run.turn_sum[last]
        world_next = np.array(places[next_seen]) - run.turn_sum[next_seen]
        x, y = world_last + part * (world_next - world_last) + run.turn_sum[i]
        return x, y, size, last, f"kill {flick['n']}: between frames {last} and {next_seen}, where the model saw it"
    if next_seen is None and i - last <= max(AHEAD_MIN_FRAMES, int(round(AHEAD_S * run.fps))):
        back = [j for j in seen if last - SPEED_FRAMES <= j < last]
        if len(back) < MIN_SPEED_POINTS or run.turned(back[0], last) is None:
            return None
        speed = ((np.array(places[last]) - run.turn_sum[last]) -
                 (np.array(places[back[0]]) - run.turn_sum[back[0]])) / (last - back[0])
        if not onto_crosshair(run, places, last, speed, size, flick["stats_frame"]):
            return None
        x, y = np.array(places[last]) + turn + speed * (i - last)
        return x, y, size, last, f"kill {flick['n']}: {i - last} frames past where the model last saw it, by its own speed"
    return None


def kill_size_frames(seen, i, after, bridged):
    """The sightings a lost target's size is the median of: the last 5 before the frame, and 3 after it between two."""
    return [j for j in seen if j <= i][-SIZE_BEFORE:] + (after[:SIZE_AFTER] if bridged else [])


def flick_places(run, flick, window):
    """One kill's places for the killed target in the frames before the kill where the model lost it."""
    kill_frame = flick["stats_frame"]
    places = {i: (x, y) for i, x, y in flick["traj"] if not run.on_spot(x, y)}
    if not places or not run.inside(kill_frame - window) or not run.inside(kill_frame):
        return []
    seen = sorted(places)
    first = max(kill_frame - window, seen[0])
    last = max([j for j in seen if j < kill_frame - KILL_END_FRAMES], default=-1)
    if last < first or sum(1 for j in seen if first <= j <= last) < SEEN_SHARE * (last - first + 1):
        return []                                         # not seen steadily before it was lost
    got = []
    for i in range(max(kill_frame - window, seen[0] + 1), kill_frame - KILL_END_FRAMES):
        if i in places:
            continue
        place = kill_place(run, flick, places, i, kill_size_frames)
        if place is None:
            continue
        x, y, (width_deg, height_deg, width, height), last_seen, why = place
        if not free_place(run, i, x, y, (width_deg, height_deg)):
            continue
        got.append((i, float(x), float(y), width_deg, height_deg, width, height, last_seen, *places[last_seen], why))
    return got


def kill_places(run):
    """The kill rule's places: (frame, x, y, wd, hd, w, h, ref frame, ref x, ref y, why)."""
    out = []
    if not run.clock:
        return out
    window = int(round(run.fps / KILL_WINDOW_PER_S))
    for flick in run.flicks:
        got = flick_places(run, flick, window)
        if len(got) > PER_KILL:                               # spread over the third of a second
            got = [got[round(k * (len(got) - 1) / (PER_KILL - 1))] for k in range(PER_KILL)]
        out += got
    return out


def gap_ends(run, before, after, side):
    """A gap's two sides: the frames either side, when the target was missed for 1 or 2 frames inside the run and
    seen steadily either side, not near a kill or a crosshair spot; else None."""
    seen_before, seen_after = run.places[before], run.places[after]
    end, start = max(seen_before), min(seen_after)
    if start - end - 1 not in GAP_FRAMES or not run.inside(end - side) or not run.inside(start + side):
        return None
    if any(end - k not in seen_before for k in range(side)) or any(start + k not in seen_after for k in range(side)):
        return None
    if any(abs(k - end) <= KILL_CLEAR_FRAMES or abs(k - start) <= KILL_CLEAR_FRAMES for k in run.kill_frames):
        return None
    if run.root[before] not in run.verified or not run.steady(before, end, share=GAP_STEADY_SHARE,
                                                              half=int(round(GAP_STEADY_HALF_S * run.fps))):
        return None                                       # not known for a target, or seen now and then
    if run.on_spot(*seen_before[end]) or run.on_spot(*seen_after[start]) or run.turned(end - side, start + side) is None:
        return None
    return end, start


def gap_size(run, before, after, ends, side):
    """The target's size either side of a gap, averaged, when the two agree; else None."""
    end, start = ends
    size_before = size_of(run, run.places[before], range(end - side + 1, end + 1))
    size_after = size_of(run, run.places[after], range(start, start + side))
    if size_before is None or size_after is None or not SIZE_RATIO_MIN <= size_before[2] * size_before[3] / max(
            TINY_AREA, size_after[2] * size_after[3]) <= SIZE_RATIO_MAX:
        return None
    return [(a + b) / 2 for a, b in zip(size_before, size_after)]


def gap_places(run):
    """The gap rule's places, as kill_places gives them."""
    out = []
    side = max(SIDE_MIN_FRAMES, int(round(run.fps / SIDE_PER_S)))
    for before, after in run.follows.items():
        ends = gap_ends(run, before, after, side)
        if ends is None:
            continue
        size = gap_size(run, before, after, ends, side)
        if size is None:
            continue
        end, start = ends
        seen_before, seen_after = run.places[before], run.places[after]

        def world(seen, frame):                           # a place over the world
            return np.array(seen[frame]) - run.turn_sum[frame]
        speed_before = (world(seen_before, end) - world(seen_before, end - SPEED_SPAN)) / SPEED_SPAN
        speed_after = (world(seen_after, start + SPEED_SPAN) - world(seen_after, start)) / SPEED_SPAN
        width_deg, height_deg, width, height = size
        tolerance = max(AGREE_MIN_DEG, AGREE_SIZE * max(width_deg, height_deg))
        if np.hypot(*(world(seen_before, end) + speed_before * (start - end) - world(seen_after, start))) > tolerance \
                or np.hypot(*(world(seen_after, start) - speed_after * (start - end) - world(seen_before, end))) \
                > tolerance:
            continue
        for i in range(end + 1, start):
            part = (i - end) / (start - end)
            x, y = world(seen_before, end) + part * (world(seen_after, start) - world(seen_before, end)) + \
                run.turn_sum[i]
            if not free_place(run, i, x, y, (width_deg, height_deg)):
                continue
            out.append((i, float(x), float(y), width_deg, height_deg, width, height, end, *seen_before[end],
                        f"gap: missed for {start - end - 1} frame{'s' if start - end - 1 > 1 else ''} between "
                        f"frames {end} and {start}"))
    return out


def screen_chains(run):
    """The run's boxes linked frame to frame by their place on screen alone."""
    low, high = run.window
    chains, open_chains = [], {}
    for i in range(low, high + 1):
        now = {}
        for _, x, y, width, height, _ in run.boxes[i]:
            best = None
            for key, chain in open_chains.items():
                distance = math.hypot(x - chain[-1][1], y - chain[-1][2])
                if distance < LINK_DEG and key not in now and (best is None or distance < best[0]):
                    best = (distance, key)
            if best:
                open_chains[best[1]].append((i, x, y, width, height))
                now[best[1]] = open_chains[best[1]]
            else:
                key = (i, x, y)
                now[key] = [(i, x, y, width, height)]
                chains.append(now[key])
        open_chains = now
    return chains


def static_spots(run, chains):
    """The spots on screen where a chain stays put while the view turns (by the camera's reading and by the other
    targets), each with its frames: (frame, x, y, wd, hd, first, last, turn, chain length)."""
    need = max(STATIC_MIN_FRAMES, int(round(STATIC_MIN_S * run.fps)))
    tracks_turn = np.cumsum(run.track_shift, axis=0)
    spots = []
    for chain in chains:
        if len(chain) < need or not run.camera_ok[chain[0][0] + 1:chain[-1][0] + 1].all():
            continue
        points = np.array([(x, y) for _, x, y, _, _ in chain])
        middle = np.median(points, 0)
        if not (run.kind == "static" or run.on_spot(*middle)):
            continue
        spread = float(np.hypot(*(points - middle).T).max())
        if spread > (STILL_NEAR_DEG if math.hypot(*middle) < NEAR_CROSSHAIR_DEG else STILL_DEG):
            continue
        frames = [i for i, *_ in chain]
        moved = np.hypot(*(run.turn_sum[frames] - run.turn_sum[frames[0]]).T).max()
        moved_tracks = np.hypot(*(tracks_turn[frames] - tracks_turn[frames[0]]).T).max()
        if moved < MIN_TURN_DEG or moved_tracks < MIN_TURN_DEG:   # the camera and the other targets agree it turned
            continue
        spot = next((spot for spot in spots if math.hypot(*(spot["at"] - middle)) < SAME_SPOT_DEG), None)
        if spot is None:
            spot = dict(at=middle, frames=[])
            spots.append(spot)
        spot["frames"] += [(i, x, y, width, height, frames[0], frames[-1], float(moved), len(chain))
                           for i, x, y, width, height in chain]
    return spots


def spot_places(run, spot):
    """A spot's frames to take its box out at: the camera turning there, no other box near, up to 3 spread out."""
    ok = []
    for i, x, y, width, height, first, last, moved, length in spot["frames"]:
        if i in (first, last) or abs(run.camera[i]).max() < TURNING_DEG:     # the camera turning at that frame
            continue
        if any(math.hypot(box_x - x, box_y - y) < CLEAR_DEG
               for j in range(max(0, i - CLEAR_FRAMES), min(run.frame_count, i + CLEAR_FRAMES + 1))
               for _, box_x, box_y, *_ in run.boxes[j]
               if math.hypot(box_x - spot["at"][0], box_y - spot["at"][1]) >= SPOT_SELF_DEG):
            continue
        ok.append((i, x, y, width, height, first, last, f"false_static: put on screen for {length} frames while the "
                   f"camera turned {moved:.1f} deg" + (" (a spot where the model marks the crosshair)"
                                                     if run.on_spot(x, y) else "")))
    if len(ok) > PER_SPOT:
        ok = [ok[round(k * (len(ok) - 1) / (PER_SPOT - 1))] for k in range(PER_SPOT)]
    return ok


def static_places(run):
    """The false_static rule: (frame, x, y, wd, hd, first frame, last frame, why), up to 3 frames per spot on screen.
    Only in a static scenario (no target can move: a box that stays put on screen while the view turns is no target) or
    on a spot where the model marks the crosshair (old_review.crosshair_spots): in a moving one, every target strafing
    alike stays put on screen while the player follows one of them."""
    out = []
    for spot in static_spots(run, screen_chains(run)):
        out += spot_places(run, spot)
    return out


def lone_and_steady(run, track, i, x, y):
    """No box within 2 deg of the lone one (on screen and after the camera's turn) in the 3 frames before and after,
    and a steady view: the tracks' shift small too, and every other box continuing a track (in a fast flick the
    camera's reading fails and every target starts a new track each frame)."""
    clear = True
    for j in range(i - LONE_FRAMES, i + LONE_FRAMES + 1):
        turn = run.turn_sum[j] - run.turn_sum[i]
        if j != i and (run.near(j, x, y, CLEAR_DEG) or run.near(j, x + turn[0], y + turn[1], CLEAR_DEG)):
            clear = False
        before = {box[0] for box in run.boxes[j - 1]}
        if math.hypot(*run.track_shift[j]) > STEADY_VIEW_DEG or any(box[0] not in before for box in run.boxes[j]
                                                                    if box[0] != track):
            clear = False
    return clear


def lone_places(run, single):
    """The false_lone rule's candidates (the image test comes after decoding): (frame, x, y, wd, hd, why)."""
    out = []
    for track in single:
        (i, (x, y)), = run.places[track].items()
        if not run.inside(i - LONE_FRAMES) or not run.inside(i + LONE_FRAMES) or math.hypot(x, y) < LONE_MIN_DEG \
                or not run.placeable(x, y):
            continue
        cx, cy = old_review.to_px(x, y)
        if not run.far[int(cy), int(cx)]:                    # a target going into an excluded area shows by bits
            continue
        if not run.camera_ok[i - 2:i + 4].all() or abs(run.camera[i - 2:i + 4]).max() > STEADY_VIEW_DEG:
            continue
        box = run.box_of(i, x, y)
        if box is None or box[0] <= 0:
            continue
        if lone_and_steady(run, track, i, x, y):
            out.append((i, x, y, box[0], box[1], "false_lone: a box in one frame, nothing near it in the 3 frames "
                        "either side"))
    return out


def balanced_sum(terms):
    """The terms' sum as ffmpeg's expressions take it: halves in brackets, a balanced tree (ffmpeg refuses a flat sum
    of 100 terms or more, its parser's depth limit; service/src/video.rs does the same)."""
    if len(terms) == 1:
        return terms[0]
    half = len(terms) // 2
    return f"({balanced_sum(terms[:half])})+({balanced_sum(terms[half:])})"


def decode(video, frames):
    """The frames (sorted indices) as RGB 1280 x 720 (ffmpeg's area scaling, as everywhere): one pass from the start, so
    the numbering is the review's; frames between them in the same windows are decoded and dropped."""
    ranges = []
    for i in frames:
        if ranges and i - ranges[-1][1] <= JOIN_FRAMES:
            ranges[-1][1] = i
        else:
            ranges.append([i, i])
    while len(ranges) > MAX_WINDOWS:                  # ffmpeg's expressions stay short
        k = min(range(len(ranges) - 1), key=lambda k: ranges[k + 1][0] - ranges[k][1])
        ranges[k][1] = ranges.pop(k + 1)[1]
    select = balanced_sum([f"between(n,{first},{last})" for first, last in ranges])
    size = WIDTH * HEIGHT * 3
    process = subprocess.Popen(["ffmpeg", "-v", "error", "-i", str(video), "-vf",
                                f"select='{select}',scale={WIDTH}:{HEIGHT}:flags=area,format=rgb24", "-fps_mode",
                                "passthrough", "-f", "rawvideo", "-"], stdout=subprocess.PIPE, bufsize=0)
    want, out = set(frames), {}
    order = (i for first, last in ranges for i in range(first, last + 1))
    try:
        for i in order:
            buffer = bytearray(size)
            view, got = memoryview(buffer), 0
            while got < size and (more := process.stdout.readinto(view[got:])):
                got += more
            if got < size:
                break
            if i in want:
                out[i] = np.frombuffer(buffer, np.uint8).reshape(HEIGHT, WIDTH, 3)
    finally:
        process.stdout.close()
        process.wait()
    return out


def lined_up(run, boxes):
    """How well the export run again on the decoded frames finds the review's boxes, at frame offsets -1, 0 and 1:
    the share of the review's boxes with one within 0.15 deg (frames where the camera turns, so a frame off shows)."""
    out = {}
    for offset in (-1, 0, 1):
        hit = total = 0
        for i, found in boxes.items():
            j = i + offset
            if not 0 <= j < run.frame_count or not run.boxes[j] or abs(run.camera[i]).max() < LINE_UP_TURN_DEG:
                continue
            got = [old_review.to_deg(box[0], box[1]) for box in found]
            for _, x, y, *_ in run.boxes[j]:
                total += 1
                hit += any(math.hypot(x - got_x, y - got_y) < LINE_UP_DEG for got_x, got_y in got)
        out[offset] = (hit / total if total else None, total)
    return out


def rule_places(run, kind, rnd):
    """Every rule's places in the run, each rule's sampled down to twice its cap (the image tests drop some)."""
    joined = set(run.follows) | set(run.follows.values())
    single = [track for track, seen in run.places.items() if len(seen) == 1 and track not in joined]
    places = dict(kill=kill_places(run) if kind in ("dynamic", "switching") else [],
                  gap=gap_places(run), false_static=static_places(run), false_lone=lone_places(run, single))
    for rule, cap in PER_RUN.items():
        if len(places[rule]) > OVERSAMPLE * cap:
            places[rule] = sorted(rnd.sample(places[rule], OVERSAMPLE * cap))
    return places


def frames_needed(places):
    """The frames the image tests read: each place's, where its target was seen, and false_lone's neighbors."""
    need = set()
    for rule, found in places.items():
        for place in found:
            need.add(place[0])
            if rule in ("kill", "gap"):
                need.add(place[7])
            if rule == "false_static":
                need |= {place[5], place[6]}
            if rule == "false_lone":
                need |= set(range(place[0] - LONE_FRAMES, place[0] + LONE_FRAMES + 1))
    return need


def placed_tests(run, places, decoded, drop):
    """The kill and gap places that pass the image tests, by frame: [(rule, cx, cy, w, h, why)]."""
    frames, boxes, fixed = decoded
    fixes = collections.defaultdict(list)
    for rule in ("kill", "gap"):
        for i, x, y, _, _, width, height, seen_at, seen_x, seen_y, why in places[rule]:
            if i not in frames or seen_at not in frames:
                continue
            cx, cy = old_review.to_px(x, y)

            # the export run again on the CPU can find the target itself (a score near the threshold): its box,
            # about where and as big as the placed one, confirms the place and is left out of the boxes round it
            def same(box):
                return math.hypot(box[0] - cx, box[1] - cy) < SAME_CENTER * max(width, height) + SAME_SLACK_PX and \
                    SAME_AREA_MIN < box[2] * box[3] / (width * height) < SAME_AREA_MAX

            def touches(other):
                return abs(other[0] - cx) < (other[2] + width) / 2 + TOUCH_PX and \
                    abs(other[1] - cy) < (other[3] + height) / 2 + TOUCH_PX
            cpu = [box for box in boxes[i] if same(box)]
            others = [box[:4] for box in boxes[i] if not same(box)] + [
                (*old_review.to_px(box_x, box_y), *px_size(box_x, box_y, box_wd, box_hd))
                for _, box_x, box_y, box_wd, box_hd, _ in run.boxes[i]]
            if any(touches(other) for other in others):
                drop[f"{rule}: another box there or touching"] += 1
                continue                             # another box of the model there or touching it (a bot's head
                #                                      on its body, two targets merged): which is which is not clear
            if cpu:
                why += " (the export on the CPU finds it at " + f"{max(box[4] for box in cpu):.2f})"
            ref = shows(frames[seen_at], fixed, *old_review.to_px(seen_x, seen_y), width, height)
            if ref is None or shows(frames[i], fixed, cx, cy, width, height, ref) is None:
                drop[f"{rule}: does not show" if ref is not None else f"{rule}: not clear where seen"] += 1
                continue
            fixes[i].append((rule, cx, cy, width, height, why))
    return fixes


def static_tests(places, frames, drop, falses):
    """The false_static places whose image there is the same as where the box was first and last seen, while the view
    turned: nothing in the world (a target behind it, or come by) shows there."""
    for i, x, y, width_deg, height_deg, first, last, why in places["false_static"]:
        if not {i, first, last} <= set(frames):
            continue
        cx, cy = old_review.to_px(x, y)
        width, height = px_size(x, y, width_deg, height_deg)
        half = int(math.ceil(0.5 * max(width, height))) + STATIC_PATCH_MARGIN_PX
        template = patch(frames[i], cx, cy, half)
        same = [patch(frames[j], cx, cy, half) for j in (first, last)]
        if template is None or any(other is None or np.abs(other - template).mean() >= SAME_IMAGE for other in same):
            drop["false_static: the image there changes"] += 1
            continue
        falses[i].append(("false_static", cx, cy, width, height, why))


def lone_tests(run, places, decoded, drop, falses):
    """The false_lone places with no box of the export near in the frames either side (the excluded areas too), and
    nothing like the box's patch there: a flash, not a target the model saw once."""
    frames, boxes, _ = decoded
    for i, x, y, width_deg, height_deg, why in places["false_lone"]:
        if not set(range(i - LONE_FRAMES, i + LONE_FRAMES + 1)) <= set(frames):
            continue
        cx, cy = old_review.to_px(x, y)
        width, height = px_size(x, y, width_deg, height_deg)
        clear = True
        for j in (i - 3, i - 2, i - 1, i + 1, i + 2, i + 3):   # the export on the CPU, the excluded areas too
            turn = run.turn_sum[j] - run.turn_sum[i]
            for place_x, place_y in ((cx, cy), old_review.to_px(x + turn[0], y + turn[1])):
                if any(math.hypot(box[0] - place_x, box[1] - place_y) < max(LONE_CLEAR_PX,
                                                                             LONE_CLEAR_SIZES * max(width, height))
                       for box in boxes[j]):
                    clear = False
        if not clear:
            drop["false_lone: the export finds a box near"] += 1
            continue
        half = max(LONE_PATCH_MIN_PX, int(math.ceil(0.5 * max(width, height))) + LONE_PATCH_MARGIN_PX)
        template = patch(frames[i], cx, cy, half)
        if template is None:
            continue
        diffs = []
        for j, reach in ((i - 3, FAR_REACH_PX), (i - 1, NEAR_REACH_PX), (i + 1, NEAR_REACH_PX), (i + 3, FAR_REACH_PX)):
            turn = run.turn_sum[j] - run.turn_sum[i]
            diffs.append(best_match(template, frames[j], *old_review.to_px(x + turn[0], y + turn[1]), half, reach))
        if any(diff is None or diff < LIKE_PATCH for diff in diffs):
            drop["false_lone: like a patch beside"] += 1
            continue                                 # something like it is there in the frames either side
        falses[i].append(("false_lone", cx, cy, width, height,
                          why + f" (the image differs by {min(diffs):.0f} at best)"))


def frame_labels(run, i, found, fixes, falses):
    """A frame's labels: the review's boxes with the export's own place and size, the false ones taken out and the
    placed ones added, each as (box, score, trusted)."""
    base = []
    for track, x, y, width_deg, height_deg, score in run.boxes[i]:
        cx, cy = old_review.to_px(x, y)
        matches = [box for box in found if math.hypot(box[0] - cx, box[1] - cy) < EXPORT_MATCH_PX]
        box = (min(matches, key=lambda box: math.hypot(box[0] - cx, box[1] - cy)) if matches
               else (cx, cy, *px_size(x, y, width_deg, height_deg), score))
        base.append((tuple(float(value) for value in box[:5]), run.trusted(track, i, x, y)))
    for _, false_x, false_y, false_w, false_h, _ in falses:
        base = [(box, ok) for box, ok in base
                if math.hypot(box[0] - false_x, box[1] - false_y) >= max(TAKE_OUT_MIN_PX, 0.5 * max(false_w, false_h))]
    return [(box[:4], box[4], ok) for box, ok in base] + [((cx, cy, width, height), -1.0, True)
                                                          for _, cx, cy, width, height, _ in fixes]


def write_crops(run, decoded, mined, job, rnd, folder):
    """The crops: one round each placed or taken-out box (shifted up to 48 px at random), its labels the frame's,
    left out when another box in it is not trusted or the frame has more boxes than targets. Returns how many, and
    the counts by rule and by why left out."""
    frames, boxes, fixed = decoded
    fixes, falses = mined
    written = 0
    got = collections.Counter()
    for i in sorted(set(fixes) | set(falses)):
        labels = frame_labels(run, i, boxes[i], fixes[i], falses[i])
        if job["cap"] and len(labels) > job["cap"]:
            got["over"] += 1                         # more boxes than the scenario has targets: one is not a target
            continue
        for rule, cx, cy, width, height, why in fixes[i] + falses[i]:
            if got[rule] >= PER_RUN[rule]:
                continue
            x0 = int(np.clip(cx - CROP // 2 + rnd.randint(-JITTER_PX, JITTER_PX), 0, WIDTH - CROP))
            y0 = int(np.clip(cy - CROP // 2 + rnd.randint(-JITTER_PX, JITTER_PX), 0, HEIGHT - CROP))
            inside = [label for label in labels if x0 <= label[0][0] < x0 + CROP and y0 <= label[0][1] < y0 + CROP]
            if not all(ok for *_, ok in inside):
                got["untrusted"] += 1                # another box in it is not known for a target: its label
                continue                             # is not to be trusted
            crop_boxes = np.array([(box[0] - x0, box[1] - y0, box[2], box[3]) for box, _, _ in inside],
                                  np.float32).reshape(-1, 4)
            target_mask = np.zeros((CROP, CROP), np.uint8)
            yy, xx = np.ogrid[0:CROP, 0:CROP]
            for box_x, box_y, box_w, box_h in crop_boxes:
                target_mask[((xx - box_x) / max(1.0, box_w / 2)) ** 2 + ((yy - box_y) / max(1.0, box_h / 2)) ** 2
                            <= 1] = 1
            np.savez_compressed(folder / f"{job['stem']}_{i:05d}_{CODE[rule]}{got[rule]:02d}.npz",
                                rgb=frames[i][y0:y0 + CROP, x0:x0 + CROP], fixed=fixed[y0:y0 + CROP, x0:x0 + CROP],
                                tmask=target_mask, boxes=crop_boxes,
                                scores=np.array([score for _, score, _ in inside], np.float32),
                                mined=np.str_(rule), fix=np.array([cx - x0, cy - y0, width, height], np.float32),
                                frame=np.int32(i), why=np.str_(why))
            got[rule] += 1
            written += 1
    return written, got


def mine(job, folder, detector, out):
    """One reviewed recording's crops. Returns its manifest row (without the review's fields)."""
    video, stats, kind, split, stem = job["video"], job["stats"], job["kind"], job["split"], job["stem"]
    row = dict(kept=False, crops=0, rules={rule: 0 for rule in RULES})
    run = Run(folder, kind, stats)
    row.update(info={key: (round(float(value), 4) if isinstance(value, (float, np.floating)) else value)
                     for key, value in run.info.items()}, start_from=run.start_from)
    if run.window is None:
        return dict(row, reason="no run window (no kills lined up and no countdown)")
    rnd = random.Random(int(stem, 16))
    places = rule_places(run, kind, rnd)
    need = frames_needed(places)
    row["candidates"] = {rule: len(found) for rule, found in places.items()}
    if not need:
        return dict(row, reason="nothing to mine")
    started = time.time()
    frames = decode(video, sorted(need))
    yuvs = build_data.keyframes(video, "yuv420p")
    if len(yuvs) < MIN_KEY_FRAMES:
        return dict(row, reason="too few key frames")
    fixed = old_review.fixed_map(yuvs).astype(np.uint8)
    row["decode_s"] = round(time.time() - started, 1)
    boxes = {i: detector(frames[i], fixed, THRESHOLD) for i in sorted(frames)}
    line = lined_up(run, boxes)
    row["lined_up"] = {str(offset): [None if share is None else round(share, 3), total]
                       for offset, (share, total) in line.items()}
    share, total = line[0]
    if total >= LINE_UP_MIN_BOXES and (share is None or share < LINE_UP_SHARE
                                       or share < max((line[-1][0] or 0), (line[1][0] or 0)) + LINE_UP_MARGIN):
        return dict(row, reason="the decoded frames do not line up with the review's")
    # the rules' image tests
    drop = collections.Counter()                     # why the image tests dropped a place
    decoded = (frames, boxes, fixed)
    fixes = placed_tests(run, places, decoded, drop)  # frame: [(rule, cx, cy, w, h, why)] placed
    falses = collections.defaultdict(list)           # frame: [(rule, cx, cy, w, h, why)] taken out
    static_tests(places, frames, drop, falses)
    lone_tests(run, places, decoded, drop, falses)
    written, got = write_crops(run, decoded, (fixes, falses), job, rnd, Path(out) / split)
    row["rules"] = {rule: got[rule] for rule in RULES}
    row["dropped"] = dict(drop, untrusted=got["untrusted"], over=got["over"])
    return dict(row, kept=written > 0, crops=written)


def jobs_of(lib, per_folder, vods):
    """The recordings to mine, in order: dynamic and switching folders in turn, then tracking, then static (each kind's
    folders in a fixed shuffle)."""
    kinds, counts, facts = old_review.scenario_kinds(), old_review.target_counts(), old_review.scenario_facts()
    skip = build_data.check_runs(lib)
    by_folder = collections.defaultdict(list)
    for recording in lib.recordings:                 # newest first
        files = lib.by_id[recording["id"]]
        if not files["stats_file"] or not files["video"]:
            continue
        video = Path(files["video"])
        if Path(vods) not in video.parents:
            continue
        folder = video.parent.name
        kind = kinds.get(folder.lower())
        if kind is None or (folder, video.name) in skip or video.name in eval_moving.SKIP:
            continue
        by_folder[(kind, folder)].append(dict(video=str(video), stats=files["stats_file"], kind=kind, folder=folder,
                                              file=video.name, split=build_data.split_of(folder),
                                              cap=counts.get(folder.lower()),
                                              limit=facts.get(folder.lower(), (None, None))[1],
                                              stem=hashlib.md5(str(video).encode()).hexdigest()[:10]))
    order = {}
    for kind in ("dynamic", "switching", "tracking", "static"):
        folders = sorted(folder for folder_kind, folder in by_folder if folder_kind == kind)
        random.Random(0).shuffle(folders)
        order[kind] = [job for folder in folders for job in by_folder[(kind, folder)][:per_folder]]
    dynamic, switching = order["dynamic"], order["switching"]
    mixed = [job for pair in zip(dynamic, switching) for job in pair] + dynamic[len(switching):] + \
        switching[len(dynamic):]
    return mixed + order["tracking"] + order["static"]


class Build:
    """build's two stages at once: a thread reviews the recordings in turn (the GPU), and `workers` threads mine the
    reviewed ones (the CPU: decoding, the export)."""

    def __init__(self, args, lib, todo, rows, out):
        self.args, self.lib, self.todo, self.rows, self.out = args, lib, todo, rows, out
        self.manifest = out / "manifest.jsonl"
        self.detector = infer.OnnxDetector(MODEL, threads=4)
        self.reviewed = queue.Queue(maxsize=args.workers + 1)
        self.spent = 0.0
        self.lock = threading.Lock()
        self.started = None

    def review(self, job):
        """A recording's review folder and the review's seconds (reviewed before: its time counts all the same), or
        None to skip it."""
        folder = self.out / "reviews" / job["stem"]
        seconds = 0.0
        if (folder / "job.json").exists():
            seconds = json.loads((folder / "job.json").read_text(encoding="utf-8")).get("seconds", 0.0)
        elif self.args.reviewed_only:
            return None
        elif not ((folder / "tracks.json").exists() and (folder / "readings.json").exists()):
            try:
                result = self.lib.review_video(job["video"], str(MODEL), out=folder, stats=job["stats"],
                                               kind=job["kind"], limit=job["limit"], cap=job["cap"], quiet=True)
                seconds = result["seconds"]
                (folder / "job.json").write_text(json.dumps(dict(job, seconds=seconds)), encoding="utf-8")
            except Exception as error:               # one bad recording does not stop the build
                return job, None, f"review: {type(error).__name__}: {error}"[:REASON_CHARS]
        return job, folder, seconds

    def reviewer(self):
        for job in self.todo:
            if self.spent >= self.args.budget:
                break
            item = self.review(job)
            if item is None:
                continue
            if item[1] is not None:
                self.spent += item[2]
            self.reviewed.put(item)
        for _ in range(self.args.workers):
            self.reviewed.put(None)

    def miner(self):
        while (item := self.reviewed.get()) is not None:
            job, folder, seconds = item
            row = dict(folder=job["folder"], file=job["file"], size=job["size"], split=job["split"], kind=job["kind"],
                       stem=job["stem"], kept=False, crops=0)
            if folder is None:
                row["reason"] = seconds
            else:
                started = time.time()
                try:
                    row.update(mine(job, folder, self.detector, self.out), review_s=round(seconds, 1))
                except Exception as error:
                    row.update(reason=f"{type(error).__name__}: {error}"[:REASON_CHARS], review_s=round(seconds, 1))
                row["mine_s"] = round(time.time() - started, 1)
            with self.lock:
                self.rows.append(row)
                with open(self.manifest, "w", encoding="utf-8") as manifest:
                    manifest.writelines(json.dumps(done) + "\n" for done in
                                        sorted(self.rows, key=lambda done: (done["split"], done["folder"], done["file"])))
                rules = row.get("rules") or {}
                print(f"[{len(self.rows)}] {row['split']:5s} {row['kind'][:6]:6s} {row['folder'][:FOLDER_CHARS]:38s} "
                      f"{' '.join(f'{CODE[rule]}{rules.get(rule, 0)}' for rule in RULES)} {row.get('reason', '')} "
                      f"(review {self.spent:.0f} s, {time.time() - self.started:.0f} s)", flush=True)

    def run(self):
        self.started = time.time()
        reviewer = threading.Thread(target=self.reviewer)
        reviewer.start()
        with ThreadPoolExecutor(self.args.workers) as pool:
            for future in [pool.submit(self.miner) for _ in range(self.args.workers)]:
                future.result()
        reviewer.join()


def build(args):
    out = Path(args.out)
    for split in SPLITS:
        (out / split).mkdir(parents=True, exist_ok=True)
    lib = aimview_tools.Library(args.vods)
    jobs = jobs_of(lib, args.per_folder, args.vods)
    manifest = out / "manifest.jsonl"
    old = {}
    if manifest.exists():
        for line in manifest.read_text(encoding="utf-8").splitlines():
            if line.strip():
                row = json.loads(line)
                old[(row["folder"], row["file"])] = row
    rows, todo = [], []
    for job in jobs:
        job["size"] = Path(job["video"]).stat().st_size
        row = old.pop((job["folder"], job["file"]), None)
        if row and row.get("size") == job["size"]:
            rows.append(row)
        else:
            todo.append(job)
    rows += list(old.values())                       # rows from other runs (another --per-folder) stay
    print(f"{len(jobs)} recordings: {len(jobs) - len(todo)} done before, {len(todo)} to do; "
          f"review budget {args.budget:.0f} s", flush=True)
    if args.limit:
        todo = todo[:args.limit]
    stages = Build(args, lib, todo, rows, out)
    stages.run()
    done = [row for row in rows if "mine_s" in row or row.get("kept")]
    print(f"reviewed for {stages.spent:.0f} s; {len(done)} recordings mined")
    for split in SPLITS:
        split_rows = [row for row in rows if row["split"] == split]
        print(f"{split}: {sum(row['kept'] for row in split_rows)} of {len(split_rows)} recordings, "
              f"{sum(row['crops'] for row in split_rows)} crops: " +
              ", ".join(f"{rule} {sum((row.get('rules') or {}).get(rule, 0) for row in split_rows)}" for rule in RULES))


def pick(args):
    """--pick: crops spread over the rules (an equal share each, as far as a rule has crops) and over the recordings
    (one from each in turn), copied with picks.jsonl."""
    data, out = Path(args.out), Path(args.check)
    if out.exists() and any(out.iterdir()):
        sys.exit(f"{out} is not empty")
    recordings = {}
    for line in open(data / "manifest.jsonl", encoding="utf-8"):
        row = json.loads(line)
        recordings[row.get("stem")] = row
    by_rule = collections.defaultdict(lambda: collections.defaultdict(list))
    for file in sorted(data.glob("*/*.npz")):
        stem, _, tag = file.stem.split("_")
        rule = next(rule for rule, code in CODE.items() if tag.startswith(code))
        by_rule[rule][stem].append(file)
    rnd = random.Random(args.seed)
    quota, left = {}, args.pick

    def crops_of(rule):
        return sum(map(len, by_rule[rule].values()))
    for k, rule in enumerate(sorted(by_rule, key=crops_of)):
        quota[rule] = min(crops_of(rule), left // (len(by_rule) - k))
        left -= quota[rule]
    picked = []
    for rule, files_by_stem in by_rule.items():
        queues = []
        for stem in sorted(files_by_stem):
            files = list(files_by_stem[stem])
            rnd.shuffle(files)
            queues.append(files)
        rnd.shuffle(queues)
        got = 0
        while got < quota[rule] and any(queues):
            for files in queues:
                if files and got < quota[rule]:
                    picked.append((rule, files.pop(0)))
                    got += 1
    write_picks(picked, out, recordings)


def write_picks(picked, out, recordings):
    """The picked crops copied into their split folders of `out`, with picks.jsonl, and a line saying how many."""
    out.mkdir(parents=True, exist_ok=True)
    with open(out / "picks.jsonl", "w", encoding="utf-8") as picks:
        for rule, file in picked:
            (out / file.parent.name).mkdir(exist_ok=True)
            shutil.copy2(file, out / file.parent.name / file.name)
            crop = np.load(file)
            recording = recordings[file.stem.split("_")[0]]
            picks.write(json.dumps(dict(file=f"{file.parent.name}/{file.name}", folder=recording["folder"],
                                        video=recording["file"], kind=recording["kind"], rule=rule,
                                        frame=int(crop["frame"]), boxes=len(crop["boxes"]),
                                        fix=[round(float(value), 1) for value in crop["fix"]],
                                        why=str(crop["why"]))) + "\n")
    counts = collections.Counter(rule for rule, _ in picked)
    print(f"{len(picked)} crops from {len({file.stem.split('_')[0] for _, file in picked})} recordings into {out}: " +
          ", ".join(f"{rule} {counts[rule]}" for rule in RULES))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--vods", default=r"E:\OBS\KovOBS")
    parser.add_argument("--out", default="test_out/vod_model/data_mined")
    parser.add_argument("--per-folder", type=int, default=1, help="the newest recordings with a stats file per folder")
    parser.add_argument("--budget", type=float, default=3600, help="seconds of native review, then stop")
    parser.add_argument("--workers", type=int, default=2, help="recordings mined at once (CPU: decoding, the export)")
    parser.add_argument("--limit", type=int, help="at most this many new recordings (a trial)")
    parser.add_argument("--reviewed-only", action="store_true", help="only the recordings reviewed before (no new "
                        "review)")
    parser.add_argument("--pick", type=int, help="pick this many crops for a check by eye (with --check)")
    parser.add_argument("--check", help="the check folder --pick copies into")
    parser.add_argument("--seed", type=int, default=0)
    args = parser.parse_args()
    if args.pick:
        if not args.check:
            sys.exit("--pick needs --check <folder>")
        return pick(args)
    build(args)


if __name__ == "__main__":
    main()
