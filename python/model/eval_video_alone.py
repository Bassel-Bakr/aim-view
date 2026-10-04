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


def model_of(arg):
    """The model's name and its _u8in export."""
    p = Path(arg)
    if p.suffix in (".pt", ".onnx"):
        f = eval_vods.u8in(p)
        return re.match(r"detector_(.+)_u8in\.onnx$", f.name)[1], f
    f = HERE / "exports" / f"detector_{arg}_u8in.onnx"
    if not f.is_file():
        sys.exit(f"no {f}: export the model first (python/model/export.py --u8in)")
    return arg, f


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
    with tempfile.TemporaryDirectory() as tmp:
        f = Path(tmp) / "request.json"
        f.write_text(json.dumps(request))
        out = json.loads(subprocess.run([str(program), str(f)], capture_output=True, check=True).stdout)
    if "error" in out:
        raise RuntimeError(out["error"])
    return out["report"]


def micros(s):
    h, m, rest = s.split(":")
    sec, frac = rest.split(".")
    return ((int(h) * 60 + int(m)) * 60 + int(sec)) * 1_000_000 + int(frac.ljust(6, "0")[:6])


def truth_frames(stats, offset, fps, n):
    """Every kill in the stats file on the video's clock (frames), inside the video. A bot that died without a hit (a
    timer bot such as flick pressure's "Dumbbell") is no kill the player made."""
    meta, rows = old_review.load_stats(str(stats))
    t0 = micros(meta["Challenge Start"])
    kf = [round(((micros(r[1]) - t0) / 1e6 + offset) * fps) for r in rows if not (len(r) > 6 and r[6].strip() == "0")]
    return sorted(k for k in kf if 0 <= k < n)


def challenge(stats, offset, fps):
    """The challenge on the video's clock (frames): from its start (the stats file's Challenge Start) to its end (the
    time in the stats file's name, to the second, plus a second)."""
    meta, _ = old_review.load_stats(str(stats))
    t0 = micros(meta["Challenge Start"]) / 1e6
    hh, mm, ss = re.search(r"-(\d\d)\.(\d\d)\.(\d\d) Stats\.csv$", stats.name).groups()
    t1 = int(hh) * 3600 + int(mm) * 60 + int(ss)
    if t1 < t0 - 3600:                                  # the challenge ran past midnight
        t1 += 86400
    return round(offset * fps), round((offset + t1 - t0 + 1) * fps)


def match(truth, got, tol):
    """Each stats kill paired with a video kill at most `tol` frames from it, one to one, in time order (greedy, which
    is optimal for intervals): the pairs, the stats kills missed and the video kills that are no kill."""
    got = sorted(got)
    used, pairs, j = [False] * len(got), [], 0
    for t in truth:
        while j < len(got) and got[j] - t < -tol:
            j += 1
        k = j
        while k < len(got) and got[k] - t <= tol:
            if not used[k]:
                used[k] = True
                pairs.append((t, got[k]))
                break
            k += 1
    hit = {p[0] for p in pairs}
    return pairs, [t for t in truth if t not in hit], [g for g, u in zip(got, used) if not u]


def file_hash(p):
    return hashlib.sha256(Path(p).read_bytes()).hexdigest()


def track_all(runs, name, model, retrack):
    """Tracks the runs not tracked yet with this model file (all of them with retrack). Each run's model.json, written
    when its tracking is done, says which file tracked it."""
    cache = EVAL / "video_alone" / name
    digest = file_hash(model)

    def tracked(folder):
        stamp = folder / "model.json"
        return (all((folder / f).exists() for f in TRACKED) and stamp.exists()
                and json.loads(stamp.read_text())["sha256"] == digest)

    todo = [r for r in runs if retrack or not tracked(cache / slug(r["id"]))]
    if not todo:
        return cache
    lib = aimview_tools.Library()
    for i, r in enumerate(todo, 1):
        t, folder = time.time(), cache / slug(r["id"])
        lib.review_video(str(lib.resolve(r["id"])), str(model), str(folder), stats=str(r["stats_file"]), areas=AREAS,
                         quiet=True)
        (folder / "model.json").write_text(json.dumps(dict(model=str(model), sha256=digest)))
        print(f"tracked {i}/{len(todo)} in {time.time() - t:5.1f} s: {r['id']}", flush=True)
    return cache


def score(runs, cache, program, tol):
    """Each run's kills found, missed and false, and the runs left out with the reason."""
    out, left_out = {}, {}
    for r in runs:
        folder, video, stats = cache / slug(r["id"]), Path(r["id"]), r["stats_file"]
        with_stats = core_review(program, folder, video, stats)
        offset = with_stats["summary"]["info"].get("offset")
        if with_stats.get("mode") == "track" or offset is None:
            left_out[r["id"]] = "the stats review gives no clock offset"
            continue
        fps, n = with_stats["fps"], len(json.loads((folder / "tracks.json").read_bytes())["frames"])
        truth = truth_frames(stats, offset, fps, n)
        alone = core_review(program, folder, video, None)
        # every kill the finder gave (each flick's path ends on its kill frame), also those the measures leave out
        got = [p[-1][0] for p in alone["paths"].values() if p]
        w0, w1 = challenge(stats, offset, fps)
        got = [g for g in got if w0 - tol <= g <= w1 + tol]
        pairs, missed, false = match(truth, got, tol)
        out[r["id"]] = dict(set=r["set"], kind=r["kind"], truth=len(truth), video=len(got), found=len(pairs),
                            missed=missed, false=false, fps=fps, source=alone["summary"]["info"].get("source"))
    return out, left_out


def totals(per_run):
    """Stats kills, video kills and kills found, with recall and precision, by set, kind and both."""
    groups = defaultdict(lambda: [0, 0, 0])
    for x in per_run.values():
        for g in ("all", x["set"], x["kind"], f'{x["set"]}/{x["kind"]}'):
            groups[g][0] += x["truth"]
            groups[g][1] += x["video"]
            groups[g][2] += x["found"]
    order = ["all", *SETS, *KINDS, *(f"{s}/{k}" for s in SETS for k in KINDS)]
    return {g: dict(truth=t, video=v, found=f, recall=f / max(1, t), precision=f / max(1, v))
            for g in order if g in groups for t, v, f in [groups[g]]}


def new_path(name):
    p, n = EVAL / f"video_alone_{name}.json", 2
    while p.exists():
        p, n = EVAL / f"video_alone_{name}_{n}.json", n + 1
    return p


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("model", nargs="?", default=infer.BEST)
    ap.add_argument("--retrack", action="store_true", help="track every run again")
    ap.add_argument("--tolerance", type=int, default=3, help="frames a video kill may be from its stats kill [3]")
    ap.add_argument("--quiet", action="store_true", help="the totals only, not each run")
    a = ap.parse_args()
    name, model = model_of(a.model)
    runs = json.loads(RUNS.read_text(encoding="utf-8"))
    for r in runs:
        r["stats_file"] = Path(aimview_tools.STATS_DEFAULT) / r["stats"]
        if not r["stats_file"].is_file():
            sys.exit(f"no stats file {r['stats_file']}")
    cache = track_all(runs, name, model, a.retrack)
    per_run, left_out = score(runs, cache, review_program(), a.tolerance)
    groups = totals(per_run)
    if not a.quiet:
        for rid, x in per_run.items():
            print(f'{x["set"]:4s} {x["kind"][:6]:6s} {Path(rid).stem[:58]:58s} truth {x["truth"]:4d} '
                  f'video {x["video"]:4d} found {x["found"]:4d} missed {len(x["missed"]):3d} false {len(x["false"]):3d}')
    for rid, why in left_out.items():
        print(f"left out: {Path(rid).stem}: {why}")
    for g, x in groups.items():
        print(f'{g:16s} truth {x["truth"]:5d} video {x["video"]:5d} found {x["found"]:5d}  recall {x["recall"]:.3f}  '
              f'precision {x["precision"]:.3f}')
    out = new_path(name)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(dict(model=name, file=str(model), tolerance=a.tolerance, groups=groups, runs=per_run,
                                   left_out=left_out), indent=1))
    print("written", out.relative_to(ROOT))


if __name__ == "__main__":
    main()
