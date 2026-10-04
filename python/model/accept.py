"""The acceptance gate: may a newly exported detector reach the app? (REPRODUCE.md, "After a training run".)

It checks the model's _u8in export with its settings file (exports/detector_<name>_u8in.onnx and detector_<name>.json)
and compares every number with the current best model's (the model models.json marks as default, else infer.BEST),
both through the app's native review:

  contract     python/model/contract.py: every check must pass (its report: test_out/vod_model/accept/<name>/).
  moving       eval_moving.py's recordings of every kind, against their stats files: kills matched and flicks
               measured on static, dynamic and switching runs; on tracking runs, time on the bot minus the stats
               file's accuracy (the mean, and the mean size).
  report       eval_vods.py's four static recordings: the kills matched and flicks measured of the app's own report.
  video_alone  eval_video_alone.py's 48 runs: the video-alone kill finder's recall and precision, overall and per kind.

The limits (each against the best model's number, kind by kind):
  - kills matched may not drop, by a single kill (the user's rule);
  - flicks measured, recall and precision may be lower by at most 2 standard deviations of the best model's share over
    400 draws of its kills (seed 0). Both models are measured on the same recordings, so what makes one recording
    harder than another cancels out; what is left is chance at the level of the kill. That is the unit the check
    counts, as contract.py draws the hand-labelled crops themselves. Draws of the recordings (contract.py's unit for
    the val crops) would allow 6 to 7 points of flicks on dynamic and switching runs, where one run each (360 Tracking
    OW2 at 5 flicks of 10, Smoothbot Switch Robots at 41 of 56) sets the spread: no small margin.
  - tracking's mean size of the gap, and the mean's distance from 0, may be worse by at most 2 standard deviations of
    the best model's number over 400 draws of the tracking runs (seed 0): the stats file gives one accuracy per run,
    so the run is the finest unit there.

The tracks are reused where the scripts keep them (moving_<name>_native.pkl; video_alone/<name>/), and a cache made
before the export or a changed settings file stops the gate. The recordings not in them are tracked --jobs at a time:
each review is a process of its own, so the tracks do not depend on how many run together. The four report recordings
are reviewed again (eval_vods.py keeps nothing), unless this gate reviewed them with the same export, settings and
review program before.
Every review runs one copy of the review program, built once at the start (test_out/vod_model/accept/<name>/bin/), so
both models are reviewed by the same code even while the source changes. No script's result file is written.

On a pass, --list adds the model to models.json (so `bun run assets` ships it), unless it is there already. It never
edits models.json on a fail and never changes the default model: it prints the lines to change for that.
Writes python/model/reports/accept_<name>.json; exits 1 on a fail.
Usage: python python/model/accept.py <name> [--list] [--jobs 2]
"""
import argparse
import hashlib
import json
import os
import pickle
import shutil
import subprocess
import sys
import time
from datetime import date
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
import aimview_tools  # noqa: E402
import eval_moving  # noqa: E402
import eval_video_alone  # noqa: E402
import eval_vods  # noqa: E402
import infer  # noqa: E402

EXPORTS = HERE / "exports"
MODELS = HERE / "models.json"
REPORTS = HERE / "reports"
EVAL = ROOT / "test_out" / "vod_model" / "eval"
WORK = ROOT / "test_out" / "vod_model" / "accept"
DRAWS, SDS, SEED = 400, 2.0, 0
CLICKING = ("static", "dynamic", "switching")
TOLERANCE = 3                                   # eval_video_alone.py's frames between a video kill and its stats kill
PLAIN_THRESHOLD = 0.3                           # what the pipeline took without a settings file (with no score map)
NAME_CHARS = 60                                 # a recording's name in the progress lines
REVIEWS_AT_ONCE = 2                             # recordings tracked at once (each review a process of its own)


def say(*parts):
    print(*parts, flush=True)


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def scenario_of(video):
    return Path(video).stem.rsplit(" - ", 2)[0].lower()


# ---- the models ------------------------------------------------------------------------------------------------------
class Model:
    """A model's export and settings file, and what tells a cache made with them from one made before."""

    def __init__(self, name):
        self.name = name
        self.export = EXPORTS / f"detector_{name}_u8in.onnx"
        self.settings = EXPORTS / f"detector_{name}.json"
        for file in (self.export, self.settings):
            if not file.is_file():
                sys.exit(f"no {file.relative_to(ROOT)}: export the model first (python/model/export.py writes both)")
        settings = json.loads(self.settings.read_text())
        # without a settings file the pipeline took threshold 0.3 and no map, so a file holding just those changes
        # nothing a cache holds
        self.plain = settings.get("threshold") == PLAIN_THRESHOLD and settings.get("score_map") is None
        self.key = dict(export=sha(self.export), settings=sha(self.settings))

    def changed_after(self, when, export=True):
        """The files that changed after time `when` (a cache made then is stale)."""
        files = ([self.export] if export else []) + ([] if self.plain else [self.settings])
        return [str(file.relative_to(ROOT)) for file in files if file.stat().st_mtime > when]


def best_model():
    """The model models.json marks as default ("default": name, or an entry's "default": true), else infer.BEST."""
    info = json.loads(MODELS.read_text(encoding="utf-8"))
    if isinstance(info.get("default"), str):
        return info["default"]
    marked = [name for name, entry in info["models"].items() if entry.get("default") is True]
    return marked[0] if marked else infer.BEST


# ---- one review program for the whole run ----------------------------------------------------------------------------
def pin_programs(name):
    """Builds the review service's tool and the core's review example once, copies them into WORK/<name>/bin, and points
    aimview_tools at the copy: the reviews of both models run the same code. The video-alone scoring program."""
    say("building the review programs (cargo, release and quick profiles)")
    subprocess.run(["cargo", "build", "-q", "--release", "--manifest-path", str(ROOT / "Cargo.toml"),
                    "-p", "aimview-service", "--bin", "aimview-tool"], check=True)
    review_exe = eval_video_alone.review_program()
    bin_dir = WORK / name / "bin"
    bin_dir.mkdir(parents=True, exist_ok=True)
    tool = bin_dir / aimview_tools.TOOL.name
    shutil.copy2(aimview_tools.TOOL, tool)
    for dll in aimview_tools.TOOL.parent.glob("DirectML.dll"):      # the detector on the GPU (DirectML)
        shutil.copy2(dll, bin_dir / dll.name)
    scorer = bin_dir / review_exe.name
    shutil.copy2(review_exe, scorer)
    aimview_tools.command = lambda: [str(tool)]
    return scorer, dict(tool=sha(tool), review=sha(scorer))


# ---- the evaluations -------------------------------------------------------------------------------------------------
def contract(model):
    """contract.py on the model, its report kept in WORK/<name>/contract.json (the reports folder's is left alone)."""
    out = WORK / model.name / "contract.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    say(f"contract: {model.name}")
    code = subprocess.run([sys.executable, str(HERE / "contract.py"), model.name, "--report", str(out)]).returncode
    if code not in (0, 1) or not out.is_file():
        sys.exit(f"contract.py stopped (exit code {code})")
    report = json.loads(out.read_text())
    return dict(passed=report["passed"], report=str(out.relative_to(ROOT)),
                checks={key: check["passed"] for key, check in report["checks"].items()},
                failed=[f"{key}: {why}" for key, check in report["checks"].items() if not check["passed"]
                        for why in failures(check)])


def part_failures(check, where):
    """Why a contract check's own numbers fail it (its problems, its pairs or its value against full_v3's)."""
    out = []
    if check.get("problems"):
        out += check["problems"]
    if "more_than_reference" in check:
        if check["more_than_reference"] > check["allowed_gap"]:
            out.append(f"{where}pairs over {infer.BEST}'s: {check['more_than_reference']} of all (allowed "
                       f"{check['allowed_gap']})")
    elif "value" in check and "reference" in check and not check.get("passed", True):
        gap = check.get("allowed_gap", check.get("limit"))
        out.append(f"{where}{check['value']} against {infer.BEST}'s {check['reference']} (allowed gap {gap})"
                   if gap is not None else f"{where}{check['value']}")
    return out


def failures(check, where=""):
    """Why a contract check failed: each failing part with its numbers."""
    if not isinstance(check, dict):
        return []
    out = part_failures(check, where)
    for key, value in check.items():
        if isinstance(value, dict):
            out += failures(value, f"{where}{key} ")
        elif isinstance(value, list):
            for item in value:
                if isinstance(item, dict) and not item.get("passed", True):
                    label = item.get("video") or item.get("band")
                    allowed = item.get("allowed_gap", check.get("allowed_gap_one_recording"))
                    gap = f" (allowed gap {allowed})" if allowed is not None else ""
                    out.append(f"{where}{key} {label}: {item.get('value')} against {infer.BEST}'s "
                               f"{item.get('reference')}{gap}")
    return out


def moving(model, pick, lib, program, at_once):
    """eval_moving.py's numbers on every recording, from its track cache (the recordings not in it tracked and added,
    as eval_moving does, `at_once` at a time): {video: [kind, matched, stats kills, measured]} or, tracking, [kind, on
    target, accuracy]."""
    cache = EVAL / f"moving_{model.name}_native.pkl"
    tracks = {}
    if cache.exists():
        stale = model.changed_after(cache.stat().st_mtime)
        if stale:
            sys.exit(f"{cache.relative_to(ROOT)} is older than {', '.join(stale)}: move it to a retired/ folder and "
                     "run again to track with this model")
        tracks = pickle.load(open(cache, "rb"))
    facts, counts = lib.scenario_facts(), lib.target_counts()
    todo = [video for videos in pick.values() for video, _ in videos if video not in tracks]

    def review(video):
        return lib.review_video(video, str(model.export), cap=counts.get(scenario_of(video)), quiet=True)["tracks"]

    if todo:
        say(f"moving: tracking {len(todo)} recordings with {model.name}, {at_once} at a time")
    for at, video_tracks in aimview_tools.in_parallel([lambda video=video: review(video) for video in todo], at_once):
        say(f"moving: tracked {Path(todo[at]).stem[:NAME_CHARS]} with {model.name}")
        tracks[todo[at]] = video_tracks
        partial = cache.with_suffix(".pkl.tmp")
        pickle.dump(tracks, open(partial, "wb"))
        os.replace(partial, cache)
    fresh = [Path(video).name for video in todo]
    out = {}
    for kind, videos in pick.items():
        for video, stats in videos:
            limit = facts.get(scenario_of(video), (None, None))[1]
            out[Path(video).name] = list(eval_moving.core_numbers(program, kind, tracks[video], video, stats, limit))
    return out, fresh


def report_runs(model, lib, programs):
    """eval_vods.py's four recordings through the app's review with their stats files: {video: [matched, stats kills,
    measured]}. Kept in WORK/<name>/vods.json with the export, settings and program it was made with."""
    keep = WORK / model.name / "vods.json"
    key = dict(model.key, tool=programs["tool"])
    if keep.is_file():
        old = json.loads(keep.read_text())
        if old["key"] == key:
            return old["runs"], False
    out = {}
    for video in eval_vods.DEFAULT:
        stats = eval_vods.stats_for(video)
        if not stats:
            sys.exit(f"no stats file for {video}")
        say(f"report: reviewing {Path(video).stem[:NAME_CHARS]} with {model.name}")
        report = eval_vods.native_review(lib, video, model.export, WORK / model.name / "vods" /
                                         Path(video).stem[:NAME_CHARS], str(stats))
        info = report["summary"]["info"]
        out[Path(video).name] = [info["matched"], info["kills_stats"], report["summary"]["measured"]]
    keep.write_text(json.dumps(dict(key=key, runs=out), indent=1))
    return out, True


def video_alone(model, program, at_once):
    """eval_video_alone.py's runs scored with the video-alone finder: {run: {set, kind, truth, video, found}}, the runs
    left out, and the runs tracked now. The tracks come from its cache (runs tracked with another export are tracked
    again there, as the script does)."""
    runs = json.loads(eval_video_alone.RUNS.read_text(encoding="utf-8"))
    for run in runs:
        run["stats_file"] = Path(aimview_tools.STATS_DEFAULT) / run["stats"]
        if not run["stats_file"].is_file():
            sys.exit(f"no stats file {run['stats_file']}")
    cache = EVAL / "video_alone" / model.name
    todo = []
    for run in runs:
        stamp = cache / eval_video_alone.slug(run["id"]) / "model.json"
        if not stamp.is_file() or json.loads(stamp.read_text())["sha256"] != model.key["export"]:
            todo.append(run["id"])
        elif model.changed_after(stamp.stat().st_mtime, export=False):  # the export: its sha256
            sys.exit(f"{stamp.parent.relative_to(ROOT)} was tracked before {model.settings.name} changed: run "
                     f"python python/model/eval_video_alone.py {model.name} --retrack first")
    if todo:
        say(f"video alone: tracking {len(todo)} runs with {model.name}")
    eval_video_alone.track_all(runs, model.name, model.export, False, at_once)
    per_run, left_out = eval_video_alone.score(runs, cache, program, TOLERANCE)
    keep = ("set", "kind", "truth", "video", "found")
    return {run_id: {field: result[field] for field in keep} for run_id, result in per_run.items()}, left_out, todo


def evaluate(model, pick, lib, scorer, programs, at_once):
    started = time.time()
    moving_results, moving_fresh = moving(model, pick, lib, scorer, at_once)
    report_results, report_fresh = report_runs(model, lib, programs)
    alone, alone_left_out, alone_fresh = video_alone(model, scorer, at_once)
    return dict(moving=moving_results, report=report_results, video_alone=alone, video_alone_left_out=alone_left_out,
                fresh=dict(moving_tracked=moving_fresh, report_reviewed=report_fresh,
                           video_alone_tracked=len(alone_fresh)),
                seconds=round(time.time() - started, 1))


# ---- the limits ------------------------------------------------------------------------------------------------------
def share_sd(hits, total):
    """The standard deviation of a share over DRAWS draws of its units (each a hit or not)."""
    if total == 0:
        return 0.0
    rng = np.random.default_rng(SEED)
    return float(np.std(rng.binomial(total, hits / total, DRAWS) / total))


def run_sd(values, stat):
    """The standard deviation of stat(values) over DRAWS draws of the runs."""
    rng = np.random.default_rng(SEED)
    values = np.asarray(values, float)
    return float(np.std([stat(values[rng.integers(0, len(values), len(values))]) for _ in range(DRAWS)]))


def row(check, kind, value, ref, diff, allowed, passed, unit=""):
    return dict(check=check, kind=kind, model=value, best=ref, difference=diff, allowed=allowed, unit=unit,
                passed=bool(passed))


def count_rows(check, kind, candidate, base):
    """Kills matched (no drop) and flicks measured (2 SD of the best model's share over draws of its kills) from
    [matched, stats kills, measured] lists of the same recordings."""
    model_sums, best_sums = np.sum(candidate, axis=0), np.sum(base, axis=0)
    matched, kills = int(model_sums[0]), int(best_sums[1])
    out = [row(check, kind, f"{matched}/{kills}", f"{int(best_sums[0])}/{kills}", int(model_sums[0] - best_sums[0]), 0,
               model_sums[0] >= best_sums[0], "kills matched")]
    gap = SDS * share_sd(int(best_sums[2]), kills) * kills
    out.append(row(check, kind, f"{int(model_sums[2])}/{kills}", f"{int(best_sums[2])}/{kills}",
                   int(model_sums[2] - best_sums[2]), round(gap, 1), best_sums[2] - model_sums[2] <= gap,
                   "flicks measured"))
    return out


def tracking_rows(moving_model, moving_best):
    """The tracking runs' gap between time on the bot and accuracy: its mean size and its mean's distance from 0."""
    videos = [video for video, numbers in moving_best.items() if numbers[0] == "tracking" and video in moving_model]
    model_gap = np.array([moving_model[video][1] - moving_model[video][2] for video in videos])
    best_gap = np.array([moving_best[video][1] - moving_best[video][2] for video in videos])
    rows = []
    for unit, stat, worse in (("mean size of the gap", lambda gap: np.abs(gap).mean(),
                               lambda gap: np.abs(gap).mean()),
                              ("the mean gap's distance from 0", lambda gap: gap.mean(), lambda gap: abs(gap.mean()))):
        allowed = SDS * run_sd(best_gap, stat)
        rows.append(row("moving", "tracking", round(float(stat(model_gap)), 4), round(float(stat(best_gap)), 4),
                        round(float(worse(model_gap) - worse(best_gap)), 4), round(allowed, 4),
                        worse(model_gap) - worse(best_gap) <= allowed, unit))
    return rows


def video_alone_rows(candidate, base):
    """The video-alone finder's recall and precision, overall and per kind, on the runs both models have."""
    common = [run_id for run_id in base if run_id in candidate]
    model_totals = eval_video_alone.totals({run_id: candidate[run_id] for run_id in common})
    best_totals = eval_video_alone.totals({run_id: base[run_id] for run_id in common})
    rows = []
    for group in ("all", *CLICKING):
        model_group, best_group = model_totals[group], best_totals[group]
        for unit, units in (("recall", best_group["truth"]), ("precision", best_group["video"])):
            gap = SDS * share_sd(best_group["found"], units)
            rows.append(row("video_alone", group, round(model_group[unit], 4), round(best_group[unit], 4),
                            round(model_group[unit] - best_group[unit], 4), round(gap, 4),
                            best_group[unit] - model_group[unit] <= gap, unit))
    return rows


def judge(cand, base, con):
    rows = [dict(check="contract", kind="all", model="meets it" if con["passed"] else "fails",
                 best="", difference="", allowed="every check", unit="", passed=con["passed"])]
    moving_model, moving_best = cand["moving"], base["moving"]
    for kind in CLICKING:
        videos = [video for video, numbers in moving_best.items() if numbers[0] == kind and video in moving_model]
        rows += count_rows("moving", kind, [moving_model[video][1:] for video in videos],
                           [moving_best[video][1:] for video in videos])
    rows += tracking_rows(moving_model, moving_best)
    videos = [video for video in base["report"] if video in cand["report"]]
    rows += count_rows("report", "static", [cand["report"][video] for video in videos],
                       [base["report"][video] for video in videos])
    return rows + video_alone_rows(cand["video_alone"], base["video_alone"])


# ---- models.json -----------------------------------------------------------------------------------------------------
def entry(model, cand, rows):
    """The model's models.json entry as the others are written: its size and the checks the gate measured (the speeds
    and the words about it are the user's to add)."""
    out = {}
    checkpoint = EXPORTS / f"detector_{model.name}.pt"
    if checkpoint.is_file():
        import torch
        saved = torch.load(checkpoint, map_location="cpu", weights_only=False)
        out["params"] = int(saved.get("params") or sum(value.numel() for key, value in saved["model"].items()
                                                       if "running" not in key and "num_batches" not in key))
    fp32 = EXPORTS / f"detector_{model.name}_fp32.onnx"
    if fp32.is_file():
        out["kb"] = round(fp32.stat().st_size / 1024, 1)
    results = cand["moving"]

    def kills(kinds):
        sums = np.sum([results[video][1:] for video in results if results[video][0] in kinds], axis=0)
        return [int(sums[0]), int(sums[2])]
    track = {check["unit"]: check["model"] for check in rows if check["kind"] == "tracking"}
    out["checks"] = {"static": kills(("static",)), "moving": kills(("dynamic", "switching")),
                     "tracking": [round(track["the mean gap's distance from 0"], 3),
                                  round(track["mean size of the gap"], 3)]}
    out["accepted"] = f"{date.today().isoformat()}: python/model/reports/accept_{model.name}.json (the app's native review)"
    return out


def add_to_models(model, model_entry):
    """Adds the entry to models.json before "hand" (the last), in the file's own layout. False when it is there."""
    text = MODELS.read_text(encoding="utf-8")
    if model.name in json.loads(text)["models"]:
        return False
    head = ", ".join(f'"{key}": {json.dumps(model_entry[key])}' for key in ("params", "kb") if key in model_entry)
    lines = [f'    "{model.name}": {{'] + ([f"      {head},"] if head else []) + \
        [f'      "{key}": {json.dumps(model_entry[key], separators=(", ", ": "))},' for key in model_entry
         if key not in ("params", "kb")]
    lines[-1] = lines[-1].rstrip(",")
    block = "\n".join(lines + ["    },", ""])
    at = text.find('    "hand": {')
    if at < 0:
        sys.exit('models.json has no "hand" entry to add the model before: add it by hand')
    new = text[:at] + block + text[at:]
    if json.loads(new)["models"].get(model.name) != json.loads(json.dumps(model_entry)):
        sys.exit("the models.json entry would not read back as written: not changed")
    MODELS.write_text(new, encoding="utf-8")
    return True


def default_lines(best, name):
    """The one change that makes `name` the default: models.json's "default", which infer.py, the service and the
    browser all read."""
    return [f'python/model/models.json: "default": "{best}"  (to "{name}"; then bun run assets)']


# ---- the verdict -----------------------------------------------------------------------------------------------------
def table(rows):
    columns = ("check", "kind", "unit", "model", "best", "difference", "allowed", "result")
    cells = [[str(check[column]) if column != "result" else ("pass" if check["passed"] else "FAIL")
              for column in columns] for check in rows]
    widths = [max(len(column), *(len(line[i]) for line in cells)) for i, column in enumerate(columns)]
    say("  ".join(column.ljust(widths[i]) for i, column in enumerate(columns)))
    for line in cells:
        say("  ".join(value.ljust(widths[i]) for i, value in enumerate(line)))


def say_checks(rows, con, best):
    """The table of checks, and why each one failed."""
    say("")
    table(rows)
    say("")
    for failure in con["failed"]:
        say(f"contract: {failure}")
    for check in rows:
        if not check["passed"] and check["check"] != "contract":
            say(f"{check['check']} {check['kind']}: {check['unit']} {check['model']} against {best}'s {check['best']} "
                f"(difference {check['difference']}, allowed {check['allowed']})")


def say_verdict(model, best, passed):
    """The verdict; returns the lines to change to make the model the default (none on a fail)."""
    say(f"{model.name}: {'PASS: it may reach the app' if passed else 'FAIL: it may not reach the app'}")
    lines = default_lines(best, model.name) if passed and model.name != best else []
    if lines:
        say(f"To make {model.name} the default model (the user's call), change:")
        for line in lines:
            say(f"  {line}")
    return lines


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("name", help="the model: python/model/exports/detector_<name>_u8in.onnx and detector_<name>.json")
    parser.add_argument("--list", action="store_true", help="on a pass, add the model to models.json")
    parser.add_argument("--jobs", type=int, default=REVIEWS_AT_ONCE,
                        help=f"recordings reviewed at once [{REVIEWS_AT_ONCE}]: the results do not depend on it")
    args = parser.parse_args()
    best = best_model()
    model, best_one = Model(args.name), Model(best)
    say(f"{model.name} against {best}, the best model")
    con = contract(model)
    scorer, programs = pin_programs(model.name)
    lib = eval_vods.library()
    pick = eval_moving.picks(lib)
    cand = evaluate(model, pick, lib, scorer, programs, args.jobs)
    base = cand if best_one.name == model.name else evaluate(best_one, pick, lib, scorer, programs, args.jobs)
    rows = judge(cand, base, con)
    passed = all(check["passed"] for check in rows)
    say_checks(rows, con, best)
    listed = None
    if passed and args.list:
        listed = add_to_models(model, entry(model, cand, rows))
        say(f"models.json: {model.name} added" if listed else f"models.json: {model.name} is listed already (unchanged)")
    lines = say_verdict(model, best, passed)
    rep = dict(model=model.name, best=best, date=date.today().isoformat(), passed=passed,
               files=dict(model=model.key, best=best_one.key, programs=programs),
               limits=dict(draws=DRAWS, standard_deviations=SDS, seed=SEED,
                           kills_matched="no drop", flicks_recall_precision="2 SD of the best model's share over "
                           "draws of its kills", tracking="2 SD of the best model's number over draws of the runs"),
               contract=con, checks=rows, model_results=cand, best_results=base, listed=listed,
               default_lines=lines)
    out = REPORTS / f"accept_{model.name}.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(rep, indent=1, default=float))
    say(f"report: {out.relative_to(ROOT)}")
    sys.exit(0 if passed else 1)


if __name__ == "__main__":
    main()
