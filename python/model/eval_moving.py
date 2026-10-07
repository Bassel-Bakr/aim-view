"""End-to-end check of detectors on whole recordings of every scenario kind, against the stats files.

- static: the four eval_vods.py recordings, the valorant run and three 1wall 6targets extra small runs (a regression
  check; the last four scenarios were trained on);
- dynamic, switching, tracking: the newest recording with a stats file of the first 6 folders of each kind (12 for
  tracking) in the test split (build_data.split_of), never trained on.

Clicking kinds (static, dynamic, switching): kills matched to a target and flicks measured. Tracking: the review's
time on the target against the stats file's accuracy, hits over hits and misses: the game's own measure of the same
thing. All from the core's review of the tracks (`core_numbers`: examples/review.rs, as the app's report works them out).
The tracks come from the app's native review (the review service's aimview-tool, through python/aimview_tools.py: the
model's _u8in export, see eval_vods.u8in, and the app's areas for each recording). They are cached per model name in
test_out/vod_model/eval/moving_<name>_native.pkl.
Usage: python python/model/eval_moving.py name=model [name=model ...] [--reports <folder>]
--reports also writes the core's whole report of each run to <folder>/<name>/<video>.json: run it before and after a
change to the core's tracking or clicking review and compare the folders (`diff -rq`; BENCH.md).
"""
import glob
import json
import os
import pickle
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import build_data  # noqa: E402
import eval_video_alone  # noqa: E402
import eval_vods  # noqa: E402
import old_review  # noqa: E402

PER = {"dynamic": 6, "switching": 6, "tracking": 12}
KOVOBS = str(eval_vods.VODS)
SKIP = {"voxTS Voltaic Easy - 111 - 2026.08.13-01.17.08.mp4"}     # a recording of another game, not KovaaK's
STATIC = list(eval_vods.DEFAULT) + glob.glob(rf"{KOVOBS}\1wall 2targets xsmall - valorant\*558.46*.mp4") + \
    [path for score in ("889.26", "886.15", "849.91")
     for path in glob.glob(rf"{KOVOBS}\1wall 6targets extra small\*{score}*.mp4")]
NAME_CHARS, CELL_CHARS = 39, 14     # the table's columns


def stats_of(video):
    try:
        return eval_vods.stats_for(video)
    except ValueError:                                  # a file not named the KovOBS way
        return None


def picks(lib):
    kinds = lib.scenario_kinds()
    out = {"static": [(video, str(stats_of(video))) for video in STATIC if stats_of(video)],
           "dynamic": [], "switching": [], "tracking": []}
    for folder in sorted(Path(KOVOBS).iterdir()):
        kind = kinds.get(folder.name.lower())
        if not folder.is_dir() or kind not in out or kind == "static" or build_data.split_of(folder.name) != "test" \
                or len(out[kind]) >= PER[kind]:
            continue
        for video in sorted(folder.glob("*.mp4"), key=lambda path: -path.stat().st_mtime):
            stats = stats_of(str(video))
            if video.name not in SKIP and stats:
                out[kind].append((str(video), str(stats)))
                break
    return out


def core_numbers(program, kind, tracks, video, stats, limit, keep=None, hitbox=None):
    """A recording's numbers from the core's review of its tracks, as the app's report works them out. Clicking kinds:
    (kind, kills matched, the stats file's kills, flicks measured), with the stats file. Tracking: (kind, the review's
    time on the bot, the stats file's accuracy), without the stats file, over the scenario's time limit `limit`, and
    with `hitbox` (the scenario's facts' hitbox) on the bot inside that shape. With `keep`, the whole report is written
    there too."""
    meta, rows = old_review.load_stats(stats)
    tracking = kind == "tracking"
    request = dict(tracks=tracks, statsText="" if tracking else Path(stats).read_bytes().decode("utf-8", "replace"),
                   video=Path(video).name, stats="" if tracking else Path(stats).name, hud=None, run=None,
                   tracking=tracking, limit=limit if tracking else None, camera=[], countdown=[], faint=None,
                   hitbox=hitbox if tracking else None)
    report = eval_video_alone.request_report(program, request)
    if keep:
        Path(keep).parent.mkdir(parents=True, exist_ok=True)
        Path(keep).write_text(json.dumps(report, indent=1))
    if tracking:
        hits, misses = float(meta.get("Hit Count", 0)), float(meta.get("Miss Count", 0))
        return kind, report["summary"]["on_target"] or 0.0, hits / max(1.0, hits + misses)
    return kind, report["summary"]["info"]["matched"], len(rows), report["summary"]["measured"]


def parse_args(args):
    """The name=model arguments, and the --reports folder (None without it)."""
    if "--reports" not in args:
        return args, None
    at = args.index("--reports")
    return args[:at] + args[at + 2:], Path(args[at + 1])


def scenario_of(video):
    return Path(video).stem.rsplit(" - ", 2)[0].lower()


def model_numbers(lib, program, pick, scenarios, name, path, reports):
    """One model's numbers on every recording; the recordings not in its track cache are tracked and added.
    scenarios: the scenarios' facts and target counts."""
    model = str(eval_vods.u8in(path))
    facts, counts = scenarios
    cache = f"test_out/vod_model/eval/moving_{name}_native.pkl"
    tracks = pickle.load(open(cache, "rb")) if os.path.exists(cache) else {}
    for kind, videos in pick.items():
        for video, _ in videos:
            if video not in tracks:
                tracks[video] = lib.review_video(video, model, kind=kind, cap=counts.get(scenario_of(video)))["tracks"]
                pickle.dump(tracks, open(cache, "wb"))
    out = {}
    for kind, videos in pick.items():
        for video, stats in videos:
            limit = facts.get(scenario_of(video), (None, None))[1]
            hitbox = lib.scenarios().get(scenario_of(video), {}).get("hitbox")
            keep = reports / name / f"{Path(video).stem}.json" if reports else None
            out[video] = core_numbers(program, kind, tracks[video], video, stats, limit, keep, hitbox)
    return out


def print_table(pick, results):
    """Each recording's numbers per model, then each model's totals."""
    names = list(results)
    print("recording".ljust(44), "  ".join(name.rjust(CELL_CHARS) for name in names))
    totals = {name: {} for name in names}
    gaps = {name: [] for name in names}
    for kind, videos in pick.items():
        for video, _ in videos:
            cells = []
            for name in names:
                numbers = results[name][video]
                if kind == "tracking":
                    cells.append(f"{numbers[1]:.2f} vs {numbers[2]:.2f}")
                    gaps[name].append(numbers[1] - numbers[2])
                else:
                    cells.append(f"{numbers[1]}/{numbers[2]} {numbers[3]}f")
                    total = totals[name].setdefault("static" if kind == "static" else "moving", [0, 0, 0])
                    total[0] += numbers[1]
                    total[1] += numbers[2]
                    total[2] += numbers[3]
            label = Path(video).stem[:NAME_CHARS].encode("ascii", "replace").decode()
            print(f"{kind[:4]} {label:39s}", "  ".join(cell.rjust(CELL_CHARS) for cell in cells))
    for name in names:
        gap = np.array(gaps[name])
        static, moving = totals[name].get("static", [0, 0, 0]), totals[name].get("moving", [0, 0, 0])
        print(f"{name}: static kills {static[0]}/{static[1]}, flicks {static[2]}; dynamic and switching kills "
              f"{moving[0]}/{moving[1]}, flicks {moving[2]}; tracking on target minus accuracy: mean {gap.mean():+.3f}, "
              f"mean abs {np.abs(gap).mean():.3f}")


def main():
    lib = eval_vods.library()
    program = eval_video_alone.review_program()
    pick = picks(lib)
    scenarios = lib.scenario_facts(), lib.target_counts()
    results = {}
    os.makedirs("test_out/vod_model/eval", exist_ok=True)
    models, reports = parse_args(sys.argv[1:])
    for arg in models:
        name, path = arg.split("=", 1)
        results[name] = model_numbers(lib, program, pick, scenarios, name, path, reports)
    print_table(pick, results)


if __name__ == "__main__":
    main()
