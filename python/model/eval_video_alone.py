"""The video-alone kill finder against the stats files (REPRODUCE.md step 3).

Without a stats file and without a HUD it can read, the review finds a clicking run's kills from the video alone
(src/matching.rs: `match_video`). This check scores that finder on the runs in video_alone_runs.json: 48 KovOBS
recordings of clicking scenarios (22 static, 18 dynamic, 8 switching), one run per scenario, each with its stats file
(by its name in KovaaK's stats folder). The runs are recordings from June 2026 on, with 10 kills or more, a time limit
of 2 minutes or less and (static and dynamic) a median of 3 shots a kill or fewer. Within each kind, 40% of the
scenarios are held out ("held", 19 runs); the finder's rules were tuned on the others ("dev", 29 runs).

For each run:
1. The model tracks the run in the app's native review (the review service's aimview-tool, through
   python/aimview_tools.py), with KovOBS's areas and no target cap, so every model is tracked the same way.
2. The core reviews the tracks twice (examples/review.rs, the request the app's report sends: service/src/report.rs):
   with the stats file, for the clock offset between the stats file and the video; and with no stats file and no HUD,
   which gives the video-alone kills.
3. The truth is every kill in the stats file on the video's clock (a bot that died without a hit is no kill). Only
   video kills in the challenge count: from its start in the stats file to the time in the stats file's name plus a
   second, with 3 frames either side. A kill is found when a video kill is within 3 frames of it (one to one).
Recall is kills found over stats kills; precision is kills found over video kills. A run whose stats review gives no
clock offset is left out (one today: 400ms strafing Reflex Micro++ Horizontalish valorant).

The tracks are kept per model in test_out/vod_model/eval/video_alone/<model>/<run>/ (tracks.json, readings.json,
hud.json, and the stats review's report.json), so a change to the finder is scored again without tracking. Runs are
tracked again with --retrack (after a change to the tracking), or when the model file is not the one the tracks were
made with. The result goes to test_out/vod_model/eval/video_alone_<model>.json (video_alone_<model>_2.json and on when
that exists): the totals by set and kind, and each run's counts with the frames of its missed and false kills.

Usage: python python/model/eval_video_alone.py [model] [--retrack] [--tolerance N] [--quiet]
model: a model's name (python/model/exports/detector_<name>_u8in.onnx) or a model file (the _u8in export beside it,
as eval_vods.py takes it) [infer.BEST].
"""
import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
import time
from collections import defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
import aimview_tools  # noqa: E402
import eval_vods  # noqa: E402
import infer  # noqa: E402
import old_review  # noqa: E402

RUNS = HERE / "video_alone_runs.json"
EVAL = ROOT / "test_out" / "vod_model" / "eval"
SETS = ("dev", "held")
KINDS = ("static", "dynamic", "switching")
# KovOBS's areas as the review service gives them (service/src/areas.rs: kovobs_areas): each kind by its id
AREAS = [[x0, y0, x1, y1, re.sub(r"[^a-z0-9]+", "_", kind.lower()).strip("_")]
         for x0, y0, x1, y1, kind in old_review.OVERLAY_SHARES]
TRACKED = ("tracks.json", "readings.json", "hud.json")
HITS_COLUMN = 6                     # a stats file's kill row: the bot's hits, "0" when it died without one
MICROS = 1_000_000
HOUR_S, DAY_S = 3600, 86400
STEM_CHARS = 58                     # the printed lines' columns


def model_of(arg):
    """The model's name and its _u8in export."""
    path = Path(arg)
    if path.suffix in (".pt", ".onnx"):
        export = eval_vods.u8in(path)
        return re.match(r"detector_(.+)_u8in\.onnx$", export.name)[1], export
    export = HERE / "exports" / f"detector_{arg}_u8in.onnx"
    if not export.is_file():
        sys.exit(f"no {export}: export the model first (python/model/export.py --u8in)")
    return arg, export


def slug(run_id):
    return re.sub(r"[^\w.-]", "_", Path(run_id).stem)


def review_program():
    """The core's review of one request (examples/review.rs), built first with cargo's quick profile."""
    subprocess.run(["cargo", "build", "-q", "--profile", "quick", "--manifest-path", str(ROOT / "Cargo.toml"),
                    "-p", "aimview", "--example", "review"], check=True)
    return ROOT / "target" / "quick" / "examples" / ("review.exe" if os.name == "nt" else "review")


def core_review(program, folder, video, stats):
    """The report the app works out from the run's folder (service/src/report.rs: work_out) with no run marks, facts
    or faint cut-off: with the stats file, or (stats None) with neither the stats file nor the HUD."""
    readings = json.loads((folder / "readings.json").read_bytes())
    request = dict(tracks=json.loads((folder / "tracks.json").read_bytes()),
                   statsText=stats.read_bytes().decode("utf-8", "replace") if stats else "", video=video.name,
                   stats=stats.name if stats else "",
                   hud=json.loads((folder / "hud.json").read_bytes()) if stats else None,
                   run=None, tracking=False, limit=None, camera=readings["camera"], countdown=readings["countdown"],
                   faint=None)
    return request_report(program, request)


def request_report(program, request):
    """The report the core works out for a review request (src/review.rs `ReviewRequest`, as JSON), with `program`
    (`review_program`)."""
    with tempfile.TemporaryDirectory() as temporary:
        request_file = Path(temporary) / "request.json"
        request_file.write_text(json.dumps(request))
        answer = json.loads(subprocess.run([str(program), str(request_file)], capture_output=True, check=True).stdout)
    if "error" in answer:
        raise RuntimeError(answer["error"])
    return answer["report"]


def micros(clock):
    """A stats file's clock time (hh:mm:ss.ffffff) in microseconds."""
    hours, minutes, rest = clock.split(":")
    seconds, fraction = rest.split(".")
    return ((int(hours) * 60 + int(minutes)) * 60 + int(seconds)) * MICROS + int(fraction.ljust(6, "0")[:6])


def truth_frames(stats, offset, fps, frame_count):
    """Every kill in the stats file on the video's clock (frames), inside the video. A bot that died without a hit (a
    timer bot such as flick pressure's "Dumbbell") is no kill the player made."""
    meta, rows = old_review.load_stats(str(stats))
    start = micros(meta["Challenge Start"])
    kill_frames = [round(((micros(row[1]) - start) / MICROS + offset) * fps) for row in rows
                   if not (len(row) > HITS_COLUMN and row[HITS_COLUMN].strip() == "0")]
    return sorted(frame for frame in kill_frames if 0 <= frame < frame_count)


def challenge(stats, offset, fps):
    """The challenge on the video's clock (frames): from its start (the stats file's Challenge Start) to its end (the
    time in the stats file's name, to the second, plus a second)."""
    meta, _ = old_review.load_stats(str(stats))
    start = micros(meta["Challenge Start"]) / MICROS
    hours, minutes, seconds = re.search(r"-(\d\d)\.(\d\d)\.(\d\d) Stats\.csv$", stats.name).groups()
    end = int(hours) * HOUR_S + int(minutes) * 60 + int(seconds)
    if end < start - HOUR_S:                            # the challenge ran past midnight
        end += DAY_S
    return round(offset * fps), round((offset + end - start + 1) * fps)


def match(truth, got, tolerance):
    """Each stats kill paired with a video kill at most `tolerance` frames from it, one to one, in time order (greedy,
    which is optimal for intervals): the pairs, the stats kills missed and the video kills that are no kill."""
    got = sorted(got)
    used, pairs, j = [False] * len(got), [], 0
    for kill in truth:
        while j < len(got) and got[j] - kill < -tolerance:
            j += 1
        k = j
        while k < len(got) and got[k] - kill <= tolerance:
            if not used[k]:
                used[k] = True
                pairs.append((kill, got[k]))
                break
            k += 1
    found = {pair[0] for pair in pairs}
    return pairs, [kill for kill in truth if kill not in found], [frame for frame, taken in zip(got, used) if not taken]


def file_hash(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def track_all(runs, name, model, retrack):
    """Tracks the runs not tracked yet with this model file (all of them with retrack). Each run's model.json, written
    when its tracking is done, says which file tracked it."""
    cache = EVAL / "video_alone" / name
    digest = file_hash(model)

    def tracked(folder):
        stamp = folder / "model.json"
        return (all((folder / file).exists() for file in TRACKED) and stamp.exists()
                and json.loads(stamp.read_text())["sha256"] == digest)

    todo = [run for run in runs if retrack or not tracked(cache / slug(run["id"]))]
    if not todo:
        return cache
    lib = aimview_tools.Library()
    for i, run in enumerate(todo, 1):
        started, folder = time.time(), cache / slug(run["id"])
        lib.review_video(str(lib.resolve(run["id"])), str(model), str(folder), stats=str(run["stats_file"]),
                         areas=AREAS, quiet=True)
        (folder / "model.json").write_text(json.dumps(dict(model=str(model), sha256=digest)))
        print(f"tracked {i}/{len(todo)} in {time.time() - started:5.1f} s: {run['id']}", flush=True)
    return cache


def score(runs, cache, program, tolerance):
    """Each run's kills found, missed and false, and the runs left out with the reason."""
    out, left_out = {}, {}
    for run in runs:
        folder, video, stats = cache / slug(run["id"]), Path(run["id"]), run["stats_file"]
        with_stats = core_review(program, folder, video, stats)
        offset = with_stats["summary"]["info"].get("offset")
        if with_stats.get("mode") == "track" or offset is None:
            left_out[run["id"]] = "the stats review gives no clock offset"
            continue
        fps = with_stats["fps"]
        frame_count = len(json.loads((folder / "tracks.json").read_bytes())["frames"])
        truth = truth_frames(stats, offset, fps, frame_count)
        alone = core_review(program, folder, video, None)
        # every kill the finder gave (each flick's path ends on its kill frame), also those the measures leave out
        got = [path[-1][0] for path in alone["paths"].values() if path]
        first, last = challenge(stats, offset, fps)
        got = [frame for frame in got if first - tolerance <= frame <= last + tolerance]
        pairs, missed, false = match(truth, got, tolerance)
        out[run["id"]] = dict(set=run["set"], kind=run["kind"], truth=len(truth), video=len(got), found=len(pairs),
                              missed=missed, false=false, fps=fps, source=alone["summary"]["info"].get("source"))
    return out, left_out


def totals(per_run):
    """Stats kills, video kills and kills found, with recall and precision, by set, kind and both."""
    groups = defaultdict(lambda: [0, 0, 0])
    for result in per_run.values():
        for group in ("all", result["set"], result["kind"], f'{result["set"]}/{result["kind"]}'):
            groups[group][0] += result["truth"]
            groups[group][1] += result["video"]
            groups[group][2] += result["found"]
    order = ["all", *SETS, *KINDS, *(f"{kill_set}/{kind}" for kill_set in SETS for kind in KINDS)]
    return {group: dict(truth=truth, video=video, found=found, recall=found / max(1, truth),
                        precision=found / max(1, video))
            for group in order if group in groups for truth, video, found in [groups[group]]}


def new_path(name):
    path, number = EVAL / f"video_alone_{name}.json", 2
    while path.exists():
        path, number = EVAL / f"video_alone_{name}_{number}.json", number + 1
    return path


def print_results(per_run, left_out, groups, quiet):
    if not quiet:
        for run_id, result in per_run.items():
            print(f'{result["set"]:4s} {result["kind"][:6]:6s} {Path(run_id).stem[:STEM_CHARS]:58s} truth '
                  f'{result["truth"]:4d} video {result["video"]:4d} found {result["found"]:4d} missed '
                  f'{len(result["missed"]):3d} false {len(result["false"]):3d}')
    for run_id, why in left_out.items():
        print(f"left out: {Path(run_id).stem}: {why}")
    for group, result in groups.items():
        print(f'{group:16s} truth {result["truth"]:5d} video {result["video"]:5d} found {result["found"]:5d}  recall '
              f'{result["recall"]:.3f}  precision {result["precision"]:.3f}')


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("model", nargs="?", default=infer.BEST)
    parser.add_argument("--retrack", action="store_true", help="track every run again")
    parser.add_argument("--tolerance", type=int, default=3, help="frames a video kill may be from its stats kill [3]")
    parser.add_argument("--quiet", action="store_true", help="the totals only, not each run")
    args = parser.parse_args()
    name, model = model_of(args.model)
    runs = json.loads(RUNS.read_text(encoding="utf-8"))
    for run in runs:
        run["stats_file"] = Path(aimview_tools.STATS_DEFAULT) / run["stats"]
        if not run["stats_file"].is_file():
            sys.exit(f"no stats file {run['stats_file']}")
    cache = track_all(runs, name, model, args.retrack)
    per_run, left_out = score(runs, cache, review_program(), args.tolerance)
    groups = totals(per_run)
    print_results(per_run, left_out, groups, args.quiet)
    out = new_path(name)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(dict(model=name, file=str(model), tolerance=args.tolerance, groups=groups, runs=per_run,
                                   left_out=left_out), indent=1))
    print("written", out.relative_to(ROOT))


if __name__ == "__main__":
    main()
