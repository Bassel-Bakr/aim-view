"""Crops from where a detector's kills disagree with KovaaK's stats files, for a check by eye on the Crops page
(crop_check/README.md) and then training: the gate's video-alone check, turned into training data.

Each recording (with a stats file, outside the gate's own runs) is reviewed as the app reviews it (aimview-tool review,
the excluded areas eval_video_alone.py uses), kept in <out>/reviews/<model>/<stem>/. The core finds its kills from the
video alone (eval_video_alone.core_review) and they are matched to the stats file's (within 3 frames), as the gate
does. Two rules give crops 256 x 256 round the crosshair (shifted up to 48 px at random):
- false_kill: a kill the video gave and the stats file does not have. On the gate's small-target runs the box nearest
  the crosshair is nearly always the player's own crosshair, boxed, but elsewhere it is often a real target whose kill
  the video timed wrong, so nothing is crossed out in advance.
- missed_kill: a kill of the stats file the video did not find, at its frame: the target under or near the crosshair
  that the detector lost.
- kill (--rules kill): a kill of the stats file, BEFORE_KILL frames before it: the target at or near the crosshair, for
  a teacher to box (teacher_label.py) where the detector finds it in pieces (robots).
The crops carry the review's boxes, for the user to judge and fix. Saved like build_mined.py's (rgb, fixed, tmask: each
box's pill, boxes, scores, mined, frame, why) in the split folder
build_data.split_of gives the scenario folder, with a manifest.jsonl for make_page.py.

Usage: python python/model/build_kill_feedback.py <out> <model> [--kind static] [--match xsmall,extra small,...]
       [--recordings 20] [--per-rule 4] [--rules false_kill,missed_kill,kill] [--seed 0]
"""
import argparse
import hashlib
import json
import random
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
import aimview_tools  # noqa: E402
import build_data  # noqa: E402
import build_disagreements  # noqa: E402
import build_mined  # noqa: E402
import eval_moving  # noqa: E402
import eval_video_alone  # noqa: E402
import eval_vods  # noqa: E402
import old_review  # noqa: E402
import teacher_label  # noqa: E402

CROP, JITTER_PX = build_mined.CROP, build_mined.JITTER_PX
WIDTH, HEIGHT = build_mined.WIDTH, build_mined.HEIGHT
TOLERANCE = 3                   # frames, as the gate matches kills
BEFORE_KILL = (2, 6)            # the kill rule's frames before a kill: at the crosshair, and still on the way to it


def gate_videos(lib):
    """The gate's own recordings, by file name: never mined, so the gate stays a test of them."""
    names = {Path(run["id"]).name for run in json.loads(eval_video_alone.RUNS.read_text(encoding="utf-8"))}
    names |= {Path(video).name for videos in eval_moving.picks(lib).values() for video, _ in videos}
    return names | {Path(video).name for video in eval_vods.DEFAULT}


def recordings(lib, kind, words, count, rnd):
    """Up to `count` recordings of the kind with a stats file, outside the gate, whose scenario has one of the words:
    the newest of each scenario first, the scenarios in random order."""
    kinds, gate = lib.scenario_kinds(), gate_videos(lib)
    newest = {}
    for row in lib.list():                              # newest first
        more = lib.by_id[row["id"]]
        scenario = row["scenario"]
        if (more["stats_file"] and more["video"] and Path(more["video"]).name not in gate
                and kinds.get(scenario.lower()) == kind and any(word in scenario.lower() for word in words)):
            newest.setdefault(scenario, (Path(more["video"]), Path(more["stats_file"])))
    picked = list(newest.values())
    rnd.shuffle(picked)
    return picked[:count]


def kills(lib, video, stats, args):
    """The review's frames, the false kills and the missed kills (frames), or None when the stats file gives no clock
    offset."""
    folder = args.out / "reviews" / args.model / video.stem
    if not (folder / "tracks.json").is_file():
        lib.review_video(str(video), args.model, str(folder), stats=str(stats), areas=eval_video_alone.AREAS,
                         quiet=True)
    with_stats = eval_video_alone.core_review(args.program, folder, video, stats)
    offset = with_stats["summary"]["info"].get("offset")
    if with_stats.get("mode") == "track" or offset is None:
        return None
    frames = json.loads((folder / "tracks.json").read_bytes())["frames"]
    fps = with_stats["fps"]
    truth = eval_video_alone.truth_frames(stats, offset, fps, len(frames))
    alone = eval_video_alone.core_review(args.program, folder, video, None)
    got = [path[-1][0] for path in alone["paths"].values() if path]
    first, last = eval_video_alone.challenge(stats, offset, fps)
    got = [frame for frame in got if first - TOLERANCE <= frame <= last + TOLERANCE]
    _, missed, false = eval_video_alone.match(truth, got, TOLERANCE)
    return frames, false, missed, truth


def save(path, rgb, fixed, labels, rule, i, why, rnd):
    """One crop round the crosshair, labelled with the review's boxes inside it."""
    cx, cy = old_review.to_px(0, 0)
    x0 = int(np.clip(cx - CROP // 2 + rnd.randint(-JITTER_PX, JITTER_PX), 0, WIDTH - CROP))
    y0 = int(np.clip(cy - CROP // 2 + rnd.randint(-JITTER_PX, JITTER_PX), 0, HEIGHT - CROP))
    inside = [box for box in labels if x0 <= box[0] < x0 + CROP and y0 <= box[1] < y0 + CROP]
    boxes = np.array([(box[0] - x0, box[1] - y0, box[2], box[3]) for box in inside], np.float32).reshape(-1, 4)
    np.savez_compressed(path, rgb=rgb[y0:y0 + CROP, x0:x0 + CROP], fixed=fixed[y0:y0 + CROP, x0:x0 + CROP],
                        tmask=teacher_label.pill_mask(boxes, (CROP, CROP)), boxes=boxes,
                        scores=np.array([box[4] for box in inside], np.float32), mined=np.str_(rule),
                        frame=np.int32(i), why=np.str_(why))


def crops_of(lib, video, stats, args, rnd):
    """One recording's crops; its manifest row."""
    found = kills(lib, video, stats, args)
    if found is None:
        return dict(video=str(video), crops=0, reason="the stats file gives no clock offset")
    frames, false, missed, truth = found
    rules = []
    if "false_kill" in args.rules:
        rules += [("false_kill", frame) for frame in rnd.sample(false, min(args.per_rule, len(false)))]
    if "missed_kill" in args.rules:
        rules += [("missed_kill", frame) for frame in rnd.sample(missed, min(args.per_rule, len(missed)))]
    if "kill" in args.rules:
        rules += [("kill", kill - rnd.choice(BEFORE_KILL)) for kill in rnd.sample(truth, min(args.per_rule, len(truth)))]
    stem = hashlib.md5(video.name.encode()).hexdigest()[:10]
    folder = args.out / build_data.split_of(video.parent.name)
    folder.mkdir(parents=True, exist_ok=True)
    decoded = build_mined.decode(video, sorted({frame for _, frame in rules}))
    fixed = old_review.fixed_map(build_data.keyframes(video, "yuv420p")).astype(np.uint8)
    written = 0
    for n, (rule, i) in enumerate(rules):
        if i not in decoded:
            continue
        labels = build_disagreements.boxes_px(frames[i])
        why = {"false_kill": "the video gave a kill here and the stats file has none: is the box at the crosshair a target?",
               "missed_kill": "the stats file has a kill here and the video found none",
               "kill": "a few frames before a kill of the stats file: the target at or near the crosshair"}[rule]
        save(folder / f"{stem}_{i:05d}_{rule[0]}{n:02d}.npz", decoded[i], fixed, labels, rule, i, why, rnd)
        written += 1
    print(f"{video.name}: {len(false)} false kills, {len(missed)} missed; {written} crops", flush=True)
    return dict(stem=stem, folder=video.parent.name, kind=args.kind, video=str(video), crops=written,
                false=len(false), missed=len(missed))


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("out", type=Path)
    parser.add_argument("model")
    parser.add_argument("--kind", default="static")
    parser.add_argument("--match", default="xsmall,extra small,small,micro,tile,pressure",
                        help="comma-separated words, one of which the scenario's name must have")
    parser.add_argument("--recordings", type=int, default=20)
    parser.add_argument("--per-rule", type=int, default=4, help="crops of each rule per recording at most")
    parser.add_argument("--rules", default="false_kill,missed_kill", help="comma-separated: false_kill, missed_kill, kill")
    parser.add_argument("--seed", type=int, default=0)
    args = parser.parse_args()
    rnd = random.Random(args.seed)
    lib = aimview_tools.Library()
    args.program = eval_video_alone.review_program()
    picked = recordings(lib, args.kind, [word.strip().lower() for word in args.match.split(",")], args.recordings, rnd)
    rows = [crops_of(lib, video, stats, args, rnd) for video, stats in picked]
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "manifest.jsonl").write_text("".join(json.dumps(row) + "\n" for row in rows))
    print(f"{sum(row['crops'] for row in rows)} crops from {len(rows)} recordings in {args.out}")


if __name__ == "__main__":
    main()
