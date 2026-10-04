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
KOVOBS = r"E:\OBS\KovOBS"
SKIP = {"voxTS Voltaic Easy - 111 - 2026.08.13-01.17.08.mp4"}     # a recording of another game, not KovaaK's
STATIC = list(eval_vods.DEFAULT) + glob.glob(rf"{KOVOBS}\1wall 2targets xsmall - valorant\*558.46*.mp4") + \
    [p for x in ("889.26", "886.15", "849.91") for p in glob.glob(rf"{KOVOBS}\1wall 6targets extra small\*{x}*.mp4")]


def stats_of(v):
    try:
        return eval_vods.stats_for(v)
    except ValueError:                                  # a file not named the KovOBS way
        return None


def picks():
    kinds = old_review.scenario_kinds()
    out = {"static": [(v, str(stats_of(v))) for v in STATIC if stats_of(v)],
           "dynamic": [], "switching": [], "tracking": []}
    for d in sorted(Path(KOVOBS).iterdir()):
        k = kinds.get(d.name.lower())
        if not d.is_dir() or k not in out or k == "static" or build_data.split_of(d.name) != "test" or len(out[k]) >= PER[k]:
            continue
        for v in sorted(d.glob("*.mp4"), key=lambda p: -p.stat().st_mtime):
            st = stats_of(str(v))
            if v.name not in SKIP and st:
                out[k].append((str(v), str(st)))
                break
    return out


def core_numbers(program, kind, tracks, video, stats, limit, keep=None):
    """A recording's numbers from the core's review of its tracks, as the app's report works them out. Clicking kinds:
    (kind, kills matched, the stats file's kills, flicks measured), with the stats file. Tracking: (kind, the review's
    time on the bot, the stats file's accuracy), without the stats file, over the scenario's time limit `limit`. With
    `keep`, the whole report is written there too."""
    meta, rows = old_review.load_stats(stats)
    tracking = kind == "tracking"
    request = dict(tracks=tracks, statsText="" if tracking else Path(stats).read_bytes().decode("utf-8", "replace"),
                   video=Path(video).name, stats="" if tracking else Path(stats).name, hud=None, run=None,
                   tracking=tracking, limit=limit if tracking else None, camera=[], countdown=[], faint=None)
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


def main():
    lib = eval_vods.library()
    program = eval_video_alone.review_program()
    pick = picks()
    facts = old_review.scenario_facts()
    counts = old_review.target_counts()
    res = {}
    os.makedirs("test_out/vod_model/eval", exist_ok=True)
    models, reports = parse_args(sys.argv[1:])
    for arg in models:
        name, path = arg.split("=", 1)
        model = str(eval_vods.u8in(path))
        cache = f"test_out/vod_model/eval/moving_{name}_native.pkl"
        tracks = pickle.load(open(cache, "rb")) if os.path.exists(cache) else {}
        for vs in pick.values():
            for v, _ in vs:
                if v not in tracks:
                    cap = counts.get(Path(v).stem.rsplit(" - ", 2)[0].lower())
                    tracks[v] = lib.review_video(v, model, cap=cap)["tracks"]
                    pickle.dump(tracks, open(cache, "wb"))
        out = {}
        for kind, vs in pick.items():
            for v, st in vs:
                limit = facts.get(Path(v).stem.rsplit(" - ", 2)[0].lower(), (None, None))[1]
                keep = reports / name / f"{Path(v).stem}.json" if reports else None
                out[v] = core_numbers(program, kind, tracks[v], v, st, limit, keep)
        res[name] = out
    names = list(res)
    print("recording".ljust(44), "  ".join(n.rjust(14) for n in names))
    tot = {n: {} for n in names}
    err = {n: [] for n in names}
    for kind, vs in pick.items():
        for v, _ in vs:
            cells = []
            for n in names:
                r = res[n][v]
                if kind == "tracking":
                    cells.append(f"{r[1]:.2f} vs {r[2]:.2f}")
                    err[n].append(r[1] - r[2])
                else:
                    cells.append(f"{r[1]}/{r[2]} {r[3]}f")
                    t = tot[n].setdefault("static" if kind == "static" else "moving", [0, 0, 0])
                    t[0] += r[1]; t[1] += r[2]; t[2] += r[3]
            print(f"{kind[:4]} {Path(v).stem[:39].encode('ascii', 'replace').decode():39s}",
                  "  ".join(c.rjust(14) for c in cells))
    for n in names:
        e, s, m = np.array(err[n]), tot[n].get("static", [0, 0, 0]), tot[n].get("moving", [0, 0, 0])
        print(f"{n}: static kills {s[0]}/{s[1]}, flicks {s[2]}; dynamic and switching kills {m[0]}/{m[1]}, flicks {m[2]}; "
              f"tracking on target minus accuracy: mean {e.mean():+.3f}, mean abs {np.abs(e).mean():.3f}")


if __name__ == "__main__":
    main()
