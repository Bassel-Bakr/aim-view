"""Training crops from the moments just before kills, where the model is weakest: a target held under the crosshair.

The key-frame dataset (build_data.py) almost never catches a kill. Here, for each static VOD with a stats file:
1. the video's clock is lined up with the stats file's: the first seconds are decoded, tracked with the model, and the
   first kills matched (old_review.match_times votes the offset, to the frame);
2. about 10 kills spread over the run are picked, and only the third of a second before each is decoded (seeking to
   the key frame before it);
3. those frames are labelled with the model's detections. The killed target keeps its label even where the model lost
   it under the crosshair: it does not move, so its place follows the camera's turn (the tracks' frame-to-frame shift)
   from where it was last seen. Those are the labels the model needs to learn.
Each frame gives one 256 x 256 crop round the crosshair (shifted a little at random), saved like build_data.py's
(rgb, fixed, tmask, boxes) plus "hidden" (1 when the killed target's label came from following the camera).
Splits follow build_data.split_of (scenario folders), so the end-to-end test VODs never reach training.
Incremental: a VOD already in the manifest (same file and size) is skipped.
Usage: python python/model/build_kills.py [--out test_out/vod_model/data_kills] [--per-folder 1] [--kills 10]
       [--vods <recordings' folder>] [--also <file of VOD hashes>] [--threads 4]
"""
import argparse
import hashlib
import json
import math
import random
import subprocess
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
import build_data  # noqa: E402
import infer  # noqa: E402
import old_review  # noqa: E402
import aimview_tools  # noqa: E402
import local_config  # noqa: E402

CROP = 256
BACK = (1, 3, 6, 10, 15, 22, 30)          # frames before the kill used, at 120 fps (scaled for other rates)
BACK_FPS = 120                             # the frame rate BACK's numbers are for
LABELLER = "small_v2"                      # never marks KovaaK's crosshair; the models trained on these crops did
GPU = threading.Lock()                     # one detector call at a time across the VODs' threads
BATCH = 16                                 # frames the detector takes at once
MIN_KILLS = 8                              # a run with fewer kills is left out
# the clock offset: from the first kills (up to FIRST_KILLS of those in the first FIRST_KILLS_S seconds, else the
# first FALLBACK_KILLS when fewer than MIN_FIRST), the video decoded up to FIRST_MARGIN_S past the last of them
FIRST_KILLS_S, FIRST_KILLS, MIN_FIRST, FALLBACK_KILLS, FIRST_MARGIN_S = 10, 6, 3, 4, 3.0
MIN_CONFIRMED = 2                          # first kills lined up at least (and half of them)
START_MARGIN_S, END_MARGIN_S = 1.0, 0.5    # a kill used is this far inside the video
LEAD_S = 0.1                               # decoded before the earliest frame used
LOST_S = 0.25                              # the killed target's track ends at most this long before the kill
AFTER_FRAMES = 3                           # decoded past the kill
LAST_SEEN_FRAMES = 2                       # a track's last sighting counts up to this far past the kill
KILLED_DEG = 1.5                           # the killed target ends (and is placed) this near the crosshair
LATE_DEG_PER_S = 4.0                       # a track that ends before the kill counts as this much farther per second
SIZE_MATCH_PX = 2                          # a box this near the track's place gives the target's size
MIN_RADIUS_PX = 1.5                        # the smallest radius of a target's disc in the mask and of a lost target
RING_IN, RING_OUT = 1.6, 2.6               # the wall round a lost target: a ring of 1.6 to 2.6 times its radius
WINDOW_RADII = 3                           # the window looked at: 3 radii (and 2 px) round it
MIN_VISIBLE, MIN_RING = 3, 6               # pixels of the target and of the ring that must show
MIN_CONTRAST = 40                          # the target's middle differs from the ring by this much at least
OTHER_MIN_PX = 2.0                         # another box: farther than this (or half the target's width) from it
CONFIDENT = 0.6                            # without a target count, the other boxes scoring this or more
JITTER_PX = 48                             # a crop's corner moves up to this far at random
HASH_CHARS = 10                            # a crop's name starts with this much of its VOD path's md5
FOLDER_CHARS, REASON_CHARS = 40, 120       # the printed folder's width; an error's reason kept in the manifest
SPLITS = ("train", "val", "test")


def decode(video, start, end):
    """Frames from `start` to `end` seconds, RGB 1280 x 720 (ffmpeg's own conversion, as in training and in the
    review)."""
    begin = max(0.0, start)
    frame_bytes = old_review.W * old_review.H * 3
    decoded = subprocess.run(["ffmpeg", "-v", "error", "-ss", f"{begin:.4f}", "-i", str(video), "-t",
                              f"{end - begin:.4f}", "-vf",
                              f"scale={old_review.W}:{old_review.H}:flags=area,format=rgb24", "-f", "rawvideo", "-"],
                             capture_output=True)
    count = len(decoded.stdout) // frame_bytes
    return np.frombuffer(decoded.stdout[:count * frame_bytes], np.uint8).reshape(count, old_review.H, old_review.W, 3)


def read_frames(stream, buffer, size):
    """Fills the buffer's frames from the stream; returns how many came whole."""
    count = 0
    while count < BATCH:
        view, got = memoryview(buffer[count].reshape(-1)), 0
        while got < size and (more := stream.readinto(view[got:])):
            got += more
        if got < size:
            break
        count += 1
    return count


def detect_stream(detector, video, end, fixed):
    """The model's boxes for every frame from the start to `end` seconds, decoded and detected 16 frames at a time (the
    frames are not kept: a captured pipe of 10 s of 720p RGB is 3.3 GB and took 32 s)."""
    size = old_review.W * old_review.H * 3
    process = subprocess.Popen(["ffmpeg", "-v", "error", "-i", str(video), "-t", f"{end:.4f}", "-vf",
                                f"scale={old_review.W}:{old_review.H}:flags=area,format=rgb24", "-f", "rawvideo", "-"],
                               stdout=subprocess.PIPE, bufsize=0)
    buffer, out = np.empty((BATCH, old_review.H, old_review.W, 3), np.uint8), []
    try:
        while True:
            count = read_frames(process.stdout, buffer, size)
            if count:
                out += detect(detector, buffer[:count], fixed)
            if count < BATCH:
                return out
    finally:
        process.stdout.close()
        process.wait()


def detect(detector, frames, fixed):
    """The model's boxes per frame (cx, cy, w, h, score in pixels), the overlay masked out as in the review."""
    out = []
    for i in range(0, len(frames), BATCH):
        with GPU:
            found = detector.batch(frames[i:i + BATCH], fixed)
        for boxes in found:
            keep = [box for box in boxes if old_review.MASK[min(old_review.H - 1, max(0, int(box[1]))),
                                                            min(old_review.W - 1, max(0, int(box[0])))]]
            out.append(np.array(keep, np.float32).reshape(-1, 5))
    return out


def tracks_of(boxes, start):
    """Link per-frame boxes into tracks (old_review.link), frames numbered from `start`."""
    rows = [[(*old_review.to_deg(float(box[0]), float(box[1])), int(round(math.pi / 4 * box[2] * box[3])))
             for box in frame] for frame in boxes]
    frames = old_review.link(rows)
    for k, frame in enumerate(frames):
        frame["i"] = start + k
    return frames


def to_px(x_deg, y_deg):
    """A place in degrees from the crosshair as frame pixels (1280 x 720): old_review.to_px's formula."""
    x =old_review.CX + old_review.K * math.tan(math.radians(x_deg))
    return x, old_review.CY - math.tan(math.radians(y_deg)) * math.hypot(old_review.K, x - old_review.CX)


def kill_times(stats):
    """The stats file's kill times, in seconds from the challenge's start."""
    meta, rows = old_review.load_stats(stats)
    start = old_review.datetime.strptime(meta["Challenge Start"], "%H:%M:%S.%f")
    return [(old_review.datetime.strptime(row[1], "%H:%M:%S.%f") - start).total_seconds() for row in rows]


def clock_offset(detector, video, kills, timing, fixed):
    """The video's clock against the stats file's, from the first kills (old_review.match_times votes it, to the
    frame): (offset, None), or (None, why not). timing: the video's frame rate and duration."""
    fps, duration = timing
    first = [kill for kill in kills if kill < FIRST_KILLS_S][:FIRST_KILLS]
    if len(first) < MIN_FIRST:
        first = kills[:FALLBACK_KILLS]
    end = min(duration, first[-1] + FIRST_MARGIN_S)
    tracks = dict(fps=fps, frames=tracks_of(detect_stream(detector, video, end, fixed), 0))
    _, info = old_review.match_times(tracks, first, [1] * len(first))
    if info.get("offset") is None or info.get("confirmed", 0) < max(MIN_CONFIRMED, (len(first) + 1) // 2):
        return None, f"no clock offset ({info.get('confirmed')} of {len(first)} first kills lined up)"
    return info["offset"], None


def killed_target(places, kill_frame, fps, lost_frames):
    """The killed target: the track nearest the crosshair that ends in the last quarter second (a track that ends
    earlier counts as farther), or None."""
    best = None
    for track, seen in places.items():
        last = max(i for i in seen if i <= kill_frame + LAST_SEEN_FRAMES)
        if last < kill_frame - lost_frames:
            continue
        distance = math.hypot(*seen[last])
        cost = distance + LATE_DEG_PER_S * (kill_frame - min(last, kill_frame)) / fps
        if distance < KILLED_DEG and (best is None or cost < best[0]):
            best = (cost, track, last)
    return None if best is None else best[1]


def followed(seen, frames, kill_frame):
    """The target's place in each frame up to the kill: where the model saw it, else where it was last seen moved by
    the camera's turn (the tracks' frame-to-frame shift)."""
    shift = {frame["i"]: frame.get("shift") or [0.0, 0.0] for frame in frames}
    place = {}
    x, y = seen[min(seen)]
    for i in range(min(seen), kill_frame):
        if i in seen:
            x, y = seen[i]
        else:
            x, y = x + shift.get(i, [0, 0])[0], y + shift.get(i, [0, 0])[1]
        place[i] = (x, y)
    return place


def target_size(seen, boxes, first_frame):
    """The target's width and height (px): the median of the model's boxes where it saw it, or None."""
    sizes = []
    for i, (x_deg, y_deg) in seen.items():
        frame_boxes = boxes[i - first_frame]
        if len(frame_boxes):
            cx, cy = to_px(x_deg, y_deg)
            j = int(np.argmin(np.hypot(frame_boxes[:, 0] - cx, frame_boxes[:, 1] - cy)))
            if math.hypot(frame_boxes[j, 0] - cx, frame_boxes[j, 1] - cy) < SIZE_MATCH_PX:
                sizes.append(frame_boxes[j, 2:4])
    return np.median(np.array(sizes), 0) if sizes else None


def shows_beside_crosshair(rgb, fixed, cx, cy, width):
    """Where the labeller lost the target: does some of it show beside the crosshair, unlike the wall round it? A
    label on the crosshair alone (the target gone, or wholly covered) taught small_v4 to v6 the crosshair as a
    target."""
    radius = max(MIN_RADIUS_PX, 0.5 * width)
    reach = int(WINDOW_RADII * radius) + 2
    top, bottom = max(0, int(cy) - reach), min(old_review.H, int(cy) + reach + 1)
    left, right = max(0, int(cx) - reach), min(old_review.W, int(cx) + reach + 1)
    yy, xx = np.mgrid[top:bottom, left:right]
    distance2 = (xx - cx) ** 2 + (yy - cy) ** 2
    free = fixed[top:bottom, left:right] == 0
    visible = (distance2 <= radius ** 2) & free
    ring = (distance2 >= (RING_IN * radius) ** 2) & (distance2 <= (RING_OUT * radius) ** 2) & free
    if visible.sum() < MIN_VISIBLE or ring.sum() < MIN_RING:
        return False
    pixels = rgb[top:bottom, left:right].astype(np.float32)
    return np.linalg.norm(np.median(pixels[visible], 0) - np.median(pixels[ring], 0)) >= MIN_CONTRAST


def other_labels(frame_boxes, cx, cy, width, count):
    """The other targets: at most the scenario's count minus the killed one, the model's most confident. Everything
    else is background: labelling every detection taught wall seams as targets (small_v4)."""
    rest = sorted((box for box in frame_boxes if math.hypot(box[0] - cx, box[1] - cy) > max(OTHER_MIN_PX, 0.5 * width)),
                  key=lambda box: -box[4])
    return [box[:4] for box in (rest[:max(0, count - 1)] if count else [box for box in rest if box[4] >= CONFIDENT])]


def save_crop(path, rgb, fixed, labels, rnd, hidden):
    """One crop round the crosshair (shifted a little at random) with its labels, as build_data.py saves them."""
    x0 = int(np.clip(old_review.CX - CROP // 2 + rnd.randint(-JITTER_PX, JITTER_PX), 0, old_review.W - CROP))
    y0 = int(np.clip(old_review.CY - CROP // 2 + rnd.randint(-JITTER_PX, JITTER_PX), 0, old_review.H - CROP))
    boxes = np.array([[box[0] - x0, box[1] - y0, box[2], box[3]] for box in labels
                      if x0 <= box[0] < x0 + CROP and y0 <= box[1] < y0 + CROP], np.float32).reshape(-1, 4)
    yy, xx = np.mgrid[0:CROP, 0:CROP]
    target_mask = np.zeros((CROP, CROP), np.uint8)
    for box in boxes:
        target_mask[(xx - box[0]) ** 2 + (yy - box[1]) ** 2 <= max(MIN_RADIUS_PX, 0.5 * box[2]) ** 2] = 1
    np.savez_compressed(path, rgb=rgb[y0:y0 + CROP, x0:x0 + CROP], fixed=fixed[y0:y0 + CROP, x0:x0 + CROP],
                        tmask=target_mask, boxes=boxes, hidden=np.uint8(hidden))


def kill_crops(detector, video, kill_frame, fps, fixed, plan):
    """One kill's crops, a frame at each of `back` before it (plan: back, lead and lost frames, the target count,
    the random generator, and the path of a crop at a frame back). Returns the crops and those whose target the model
    lost."""
    back, lead, lost, count, rnd, crop_path = plan
    first = kill_frame - lead
    frames = decode(video, first / fps, (kill_frame + AFTER_FRAMES) / fps)
    if len(frames) < lead:
        return 0, 0
    boxes = detect(detector, frames, fixed)
    tracks = tracks_of(boxes, first)
    places = {}
    for frame in tracks:
        for track, x, y in frame["t"]:
            places.setdefault(track, {})[frame["i"]] = (x, y)
    track = killed_target(places, kill_frame, fps, lost)
    if track is None:
        return 0, 0
    place = followed(places[track], tracks, kill_frame)
    if kill_frame - 1 not in place or math.hypot(*place[kill_frame - 1]) > KILLED_DEG:
        return 0, 0
    size = target_size(places[track], boxes, first)
    if size is None:
        return 0, 0
    width, height = size
    written = hidden = 0
    for frames_back in back:
        i = kill_frame - frames_back
        if i - first < 0 or i not in place:
            continue
        seen = i in places[track]
        cx, cy = to_px(*place[i])
        if not seen and not shows_beside_crosshair(frames[i - first], fixed, cx, cy, width):
            continue
        labels = [np.array([cx, cy, width, height], np.float32)] + other_labels(boxes[i - first], cx, cy, width, count)
        save_crop(crop_path(frames_back), frames[i - first], fixed, labels, rnd, not seen)
        written += 1
        hidden += not seen
    return written, hidden


def one(job, detector):
    """Lines up one VOD's clock with its stats file and writes the crops of up to KILLS of its kills; returns its
    manifest row (kept when it wrote a crop, else with the reason)."""
    folder, video, stats, split, out, seed = job
    count = COUNTS.get(folder.lower())
    row = dict(folder=folder, file=Path(video).name, size=Path(video).stat().st_size, split=split, kept=False, crops=0)
    fps, duration = old_review.probe(video)
    kills = kill_times(stats)
    if len(kills) < MIN_KILLS:
        return dict(row, reason="too few kills")
    fixed = old_review.fixed_map(list(old_review._frames(video, keyframes=True))).astype(np.uint8)
    # 1. the offset, from the first kills (the offsets seen so far are 0.4 to 2.2 s)
    offset, why_not = clock_offset(detector, video, kills, (fps, duration), fixed)
    if offset is None:
        return dict(row, reason=why_not)
    row["offset"] = round(offset, 4)
    # 2. the kills used: spread over the run, inside the video
    usable = [k for k, kill in enumerate(kills) if START_MARGIN_S < kill + offset < duration - END_MARGIN_S]
    rnd = random.Random(seed)
    picks = sorted(rnd.sample(usable, min(KILLS, len(usable))))
    back = [max(1, round(frames * fps / BACK_FPS)) for frames in BACK]
    lead, lost = max(back) + int(LEAD_S * fps), int(LOST_S * fps)
    stem = hashlib.md5(video.encode()).hexdigest()[:HASH_CHARS]
    written = hidden = 0
    # 3. each kill's frames, labelled with the model's detections and the killed target followed with the camera
    for k in picks:
        kill_frame = int(round((kills[k] + offset) * fps))
        plan = (back, lead, lost, count, rnd,
                lambda frames_back, k=k: Path(out) / split / f"{stem}_k{k:03d}_{frames_back:02d}.npz")
        crops, lost_by_model = kill_crops(detector, video, kill_frame, fps, fixed, plan)
        written += crops
        hidden += lost_by_model
    return dict(row, kept=written > 0, crops=written, hidden=hidden, kills=len(picks))


KILLS = 10                                  # kills used per VOD (--kills)
COUNTS = {}                                 # targets alive at once per scenario (build_data.target_counts)


def jobs_of(lib, args, static, also):
    """Each static scenario folder's newest recordings with a stats file (and those --also names) as jobs."""
    jobs = []
    for folder in sorted(path for path in Path(args.vods).iterdir() if path.is_dir()):
        if folder.name.lower() not in static:
            continue
        with_stats = []
        for video in sorted(folder.glob("*.mp4"), key=lambda path: -path.stat().st_mtime):
            name = aimview_tools.NAME.match(video.name)
            stats = lib.stats_for(name["scenario"], name["stamp"]) if name else None
            if stats:
                with_stats.append((video, stats))
        chosen = with_stats[:args.per_folder] + [pair for pair in with_stats[args.per_folder:]
                                                 if hashlib.md5(str(pair[0]).encode()).hexdigest()[:HASH_CHARS] in also]
        jobs += [(folder.name, str(video), str(stats), build_data.split_of(folder.name), args.out, i)
                 for i, (video, stats) in enumerate(chosen)]
    return jobs


def done_and_todo(jobs, manifest):
    """The manifest's rows of the jobs done before (same file and size), and the jobs left to do."""
    old = {}
    if manifest.exists():
        for line in manifest.read_text(encoding="utf-8").splitlines():
            if line.strip():
                row = json.loads(line)
                old[(row["folder"], row["file"])] = row
    rows, todo = [], []
    for job in jobs:
        row = old.get((job[0], Path(job[1]).name))
        if row and row.get("size") == Path(job[1]).stat().st_size:
            rows.append(row)
        else:
            todo.append(job)
    return rows, todo


def main():
    """Labels the VODs not done before on --threads threads, rewriting the manifest after each, and prints each
    split's counts."""
    global KILLS, COUNTS
    parser = argparse.ArgumentParser()
    parser.add_argument("--vods", default=aimview_tools.VODS_DEFAULT)
    parser.add_argument("--out", default=str(local_config.folder("data") / "vod_model" / "data_kills"))
    parser.add_argument("--per-folder", type=int, default=1)
    parser.add_argument("--also", help="a file of VOD hashes to include whatever --per-folder says (new runs)")
    parser.add_argument("--kills", type=int, default=10)
    parser.add_argument("--threads", type=int, default=4)
    args = parser.parse_args()
    KILLS = args.kills
    for split in SPLITS:
        (Path(args.out) / split).mkdir(parents=True, exist_ok=True)
    lib = aimview_tools.Library(args.vods, aimview_tools.STATS_DEFAULT)
    lib.load_stats_index()
    static = build_data.static_scenarios()
    COUNTS = build_data.target_counts()
    also = set(Path(args.also).read_text().split()) if args.also else set()
    jobs = jobs_of(lib, args, static, also)
    manifest = Path(args.out) / "manifest.jsonl"
    rows, todo = done_and_todo(jobs, manifest)
    print(f"{len(jobs)} VODs: {len(rows)} done before, {len(todo)} to do", flush=True)
    detector = infer.TorchDetector(str(HERE / "exports" / f"detector_{LABELLER}.pt"))
    started = time.time()
    with ThreadPoolExecutor(args.threads) as pool:
        for k, row in enumerate(pool.map(lambda job: _safe(job, detector), todo)):
            rows.append(row)
            with open(manifest, "w", encoding="utf-8") as file:
                file.writelines(json.dumps(done) + "\n"
                                for done in sorted(rows, key=lambda done: (done["split"], done["folder"], done["file"])))
            print(f"[{k + 1}/{len(todo)}] {row['split']:5s} {'keep' if row['kept'] else 'drop'} "
                  f"{row['folder'][:FOLDER_CHARS]:40s} {row.get('reason', '')} {row['crops']} crops, "
                  f"{row.get('hidden', 0)} hidden ({time.time() - started:.0f} s)", flush=True)
    for split in SPLITS:
        split_rows = [row for row in rows if row["split"] == split]
        print(f"{split}: {sum(row['kept'] for row in split_rows)} of {len(split_rows)} VODs, "
              f"{sum(row['crops'] for row in split_rows)} crops, "
              f"{sum(row.get('hidden', 0) for row in split_rows)} with the killed target lost by the model")


def _safe(job, detector):
    """one's row, or a dropped row with the error's reason when it raises."""
    try:
        return one(job, detector)
    except Exception as error:                              # one bad VOD does not stop the build
        return dict(folder=job[0], file=Path(job[1]).name, size=Path(job[1]).stat().st_size, split=job[3], kept=False,
                    crops=0, reason=f"{type(error).__name__}: {error}"[:REASON_CHARS])


if __name__ == "__main__":
    main()
