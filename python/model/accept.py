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
before the export or a changed settings file stops the gate. The four report recordings are reviewed again
(eval_vods.py keeps nothing), unless this gate reviewed them with the same export, settings and review program before.
Every review runs one copy of the review program, built once at the start (test_out/vod_model/accept/<name>/bin/), so
both models are reviewed by the same code even while the source changes. No script's result file is written.

On a pass, --list adds the model to models.json (so `bun run assets` ships it), unless it is there already. It never
edits models.json on a fail and never changes the default model: it prints the lines to change for that.
Writes python/model/reports/accept_<name>.json; exits 1 on a fail.
Usage: python python/model/accept.py <name> [--list]
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
import review  # noqa: E402

EXPORTS = HERE / "exports"
MODELS = HERE / "models.json"
REPORTS = HERE / "reports"
EVAL = ROOT / "test_out" / "vod_model" / "eval"
WORK = ROOT / "test_out" / "vod_model" / "accept"
DRAWS, SDS, SEED = 400, 2.0, 0
CLICKING = ("static", "dynamic", "switching")
TOLERANCE = 3                                   # eval_video_alone.py's frames between a video kill and its stats kill
# where the default model is named (the user changes it, never this gate)
def say(*a):
    print(*a, flush=True)


def sha(p):
    return hashlib.sha256(Path(p).read_bytes()).hexdigest()


# ---- the models ------------------------------------------------------------------------------------------------------
class Model:
    """A model's export and settings file, and what tells a cache made with them from one made before."""

    def __init__(self, name):
        self.name = name
        self.export = EXPORTS / f"detector_{name}_u8in.onnx"
        self.settings = EXPORTS / f"detector_{name}.json"
        for f in (self.export, self.settings):
            if not f.is_file():
                sys.exit(f"no {f.relative_to(ROOT)}: export the model first (python/model/export.py writes both)")
        s = json.loads(self.settings.read_text())
        # without a settings file the pipeline took threshold 0.3 and no map, so a file holding just those changes
        # nothing a cache holds
        self.plain = s.get("threshold") == 0.3 and s.get("score_map") is None
        self.key = dict(export=sha(self.export), settings=sha(self.settings))

    def changed_after(self, t, export=True):
        """The files that changed after time t (a cache made at t is stale)."""
        files = ([self.export] if export else []) + ([] if self.plain else [self.settings])
        return [str(f.relative_to(ROOT)) for f in files if f.stat().st_mtime > t]


def best_model():
    """The model models.json marks as default ("default": name, or an entry's "default": true), else infer.BEST."""
    info = json.loads(MODELS.read_text(encoding="utf-8"))
    if isinstance(info.get("default"), str):
        return info["default"]
    marked = [n for n, m in info["models"].items() if m.get("default") is True]
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
def contract(m):
    """contract.py on the model, its report kept in WORK/<name>/contract.json (the reports folder's is left alone)."""
    out = WORK / m.name / "contract.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    say(f"contract: {m.name}")
    code = subprocess.run([sys.executable, str(HERE / "contract.py"), m.name, "--report", str(out)]).returncode
    if code not in (0, 1) or not out.is_file():
        sys.exit(f"contract.py stopped (exit code {code})")
    rep = json.loads(out.read_text())
    return dict(passed=rep["passed"], report=str(out.relative_to(ROOT)),
                checks={k: c["passed"] for k, c in rep["checks"].items()},
                failed=[f"{k}: {why}" for k, c in rep["checks"].items() if not c["passed"] for why in failures(c)])


def failures(c, where=""):
    """Why a contract check failed: each failing part with its numbers."""
    if not isinstance(c, dict):
        return []
    out = []
    if c.get("problems"):
        out += c["problems"]
    if "more_than_reference" in c:
        if c["more_than_reference"] > c["allowed_gap"]:
            out.append(f"{where}pairs over {infer.BEST}'s: {c['more_than_reference']} of all (allowed "
                       f"{c['allowed_gap']})")
    elif "value" in c and "reference" in c and not c.get("passed", True):
        gap = c.get("allowed_gap", c.get("limit"))
        out.append(f"{where}{c['value']} against {infer.BEST}'s {c['reference']} (allowed gap {gap})"
                   if gap is not None else f"{where}{c['value']}")
    for k, v in c.items():
        if isinstance(v, dict):
            out += failures(v, f"{where}{k} ")
        elif isinstance(v, list):
            for x in v:
                if isinstance(x, dict) and not x.get("passed", True):
                    label = x.get("video") or x.get("band")
                    allowed = x.get("allowed_gap", c.get("allowed_gap_one_recording"))
                    gap = f" (allowed gap {allowed})" if allowed is not None else ""
                    out.append(f"{where}{k} {label}: {x.get('value')} against {infer.BEST}'s {x.get('reference')}{gap}")
    return out


def moving(m, pick, lib):
    """eval_moving.py's numbers on every recording, from its track cache (the recordings not in it tracked and added,
    as eval_moving does): {video: [kind, matched, stats kills, measured]} or, tracking, [kind, on target, accuracy]."""
    cache = EVAL / f"moving_{m.name}_native.pkl"
    tracks = {}
    if cache.exists():
        stale = m.changed_after(cache.stat().st_mtime)
        if stale:
            sys.exit(f"{cache.relative_to(ROOT)} is older than {', '.join(stale)}: move it to a retired/ folder and "
                     "run again to track with this model")
        tracks = pickle.load(open(cache, "rb"))
    facts, counts, fresh = review.scenario_facts(), review.target_counts(), []
    for vs in pick.values():
        for v, _ in vs:
            if v not in tracks:
                say(f"moving: tracking {Path(v).stem[:60]} with {m.name}")
                tracks[v] = lib.review_video(v, str(m.export), cap=counts.get(Path(v).stem.rsplit(" - ", 2)[0].lower()),
                                             quiet=True)["tracks"]
                fresh.append(Path(v).name)
                tmp = cache.with_suffix(".pkl.tmp")
                pickle.dump(tracks, open(tmp, "wb"))
                os.replace(tmp, cache)
    out = {}
    for kind, vs in pick.items():
        for v, st in vs:
            tr, (meta, rows) = tracks[v], review.load_stats(st)
            if kind == "tracking":
                h, mi = float(meta.get("Hit Count", 0)), float(meta.get("Miss Count", 0))
                lim = facts.get(Path(v).stem.rsplit(" - ", 2)[0].lower(), (None, None))[1]
                out[Path(v).name] = [kind, review.track_summary(tr, {}, lim)["on_target"] or 0.0, h / max(1.0, h + mi)]
            else:
                fl, info = review.match(tr, st)
                out[Path(v).name] = [kind, info["matched"], len(rows),
                                     len(review.measure(fl, tr["fps"], review.target_radius(fl)))]
    return out, fresh


def report_runs(m, lib, programs):
    """eval_vods.py's four recordings through the app's review with their stats files: {video: [matched, stats kills,
    measured]}. Kept in WORK/<name>/vods.json with the export, settings and program it was made with."""
    keep = WORK / m.name / "vods.json"
    key = dict(m.key, tool=programs["tool"])
    if keep.is_file():
        old = json.loads(keep.read_text())
        if old["key"] == key:
            return old["runs"], False
    out = {}
    for v in eval_vods.DEFAULT:
        st = eval_vods.stats_for(v)
        if not st:
            sys.exit(f"no stats file for {v}")
        say(f"report: reviewing {Path(v).stem[:60]} with {m.name}")
        r = eval_vods.native_review(lib, v, m.export, WORK / m.name / "vods" / Path(v).stem[:60], str(st))
        i = r["summary"]["info"]
        out[Path(v).name] = [i["matched"], i["kills_stats"], r["summary"]["measured"]]
    keep.write_text(json.dumps(dict(key=key, runs=out), indent=1))
    return out, True


def video_alone(m, program):
    """eval_video_alone.py's runs scored with the video-alone finder: {run: {set, kind, truth, video, found}}, the runs
    left out, and the runs tracked now. The tracks come from its cache (runs tracked with another export are tracked
    again there, as the script does)."""
    runs = json.loads(eval_video_alone.RUNS.read_text(encoding="utf-8"))
    for r in runs:
        r["stats_file"] = Path(aimview_tools.STATS_DEFAULT) / r["stats"]
        if not r["stats_file"].is_file():
            sys.exit(f"no stats file {r['stats_file']}")
    cache = EVAL / "video_alone" / m.name
    todo = []
    for r in runs:
        stamp = cache / eval_video_alone.slug(r["id"]) / "model.json"
        if not stamp.is_file() or json.loads(stamp.read_text())["sha256"] != m.key["export"]:
            todo.append(r["id"])
        elif m.changed_after(stamp.stat().st_mtime, export=False):  # the export: its sha256
            sys.exit(f"{stamp.parent.relative_to(ROOT)} was tracked before {m.settings.name} changed: run "
                     f"python python/model/eval_video_alone.py {m.name} --retrack first")
    if todo:
        say(f"video alone: tracking {len(todo)} runs with {m.name}")
    eval_video_alone.track_all(runs, m.name, m.export, False)
    per_run, left_out = eval_video_alone.score(runs, cache, program, TOLERANCE)
    keep = ("set", "kind", "truth", "video", "found")
    return {k: {f: x[f] for f in keep} for k, x in per_run.items()}, left_out, todo


def evaluate(m, pick, lib, scorer, programs):
    t = time.time()
    mv, mv_fresh = moving(m, pick, lib)
    vd, vd_fresh = report_runs(m, lib, programs)
    va, va_left, va_fresh = video_alone(m, scorer)
    return dict(moving=mv, report=vd, video_alone=va, video_alone_left_out=va_left,
                fresh=dict(moving_tracked=mv_fresh, report_reviewed=vd_fresh, video_alone_tracked=len(va_fresh)),
                seconds=round(time.time() - t, 1))


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
    v = np.asarray(values, float)
    return float(np.std([stat(v[rng.integers(0, len(v), len(v))]) for _ in range(DRAWS)]))


def row(check, kind, value, ref, diff, allowed, passed, unit=""):
    return dict(check=check, kind=kind, model=value, best=ref, difference=diff, allowed=allowed, unit=unit,
                passed=bool(passed))


def count_rows(check, kind, cand, base):
    """Kills matched (no drop) and flicks measured (2 SD of the best model's share over draws of its kills) from
    [matched, stats kills, measured] lists of the same recordings."""
    c, b = np.sum(cand, axis=0), np.sum(base, axis=0)
    matched, kills = int(c[0]), int(b[1])
    out = [row(check, kind, f"{matched}/{kills}", f"{int(b[0])}/{kills}", int(c[0] - b[0]), 0, c[0] >= b[0],
               "kills matched")]
    gap = SDS * share_sd(int(b[2]), kills) * kills
    out.append(row(check, kind, f"{int(c[2])}/{kills}", f"{int(b[2])}/{kills}", int(c[2] - b[2]), round(gap, 1),
                   b[2] - c[2] <= gap, "flicks measured"))
    return out


def judge(cand, base, con):
    rows = [dict(check="contract", kind="all", model="meets it" if con["passed"] else "fails",
                 best="", difference="", allowed="every check", unit="", passed=con["passed"])]
    mv_c, mv_b = cand["moving"], base["moving"]
    for kind in CLICKING:
        vs = [v for v, x in mv_b.items() if x[0] == kind and v in mv_c]
        rows += count_rows("moving", kind, [mv_c[v][1:] for v in vs], [mv_b[v][1:] for v in vs])
    vs = [v for v, x in mv_b.items() if x[0] == "tracking" and v in mv_c]
    gc = np.array([mv_c[v][1] - mv_c[v][2] for v in vs])
    gb = np.array([mv_b[v][1] - mv_b[v][2] for v in vs])
    for unit, stat, worse in (("mean size of the gap", lambda g: np.abs(g).mean(), lambda g: np.abs(g).mean()),
                              ("the mean gap's distance from 0", lambda g: g.mean(), lambda g: abs(g.mean()))):
        gap = SDS * run_sd(gb, stat)
        rows.append(row("moving", "tracking", round(float(stat(gc)), 4), round(float(stat(gb)), 4),
                        round(float(worse(gc) - worse(gb)), 4), round(gap, 4), worse(gc) - worse(gb) <= gap, unit))
    vs = [v for v in base["report"] if v in cand["report"]]
    rows += count_rows("report", "static", [cand["report"][v] for v in vs], [base["report"][v] for v in vs])
    common = [k for k in base["video_alone"] if k in cand["video_alone"]]
    tc = eval_video_alone.totals({k: cand["video_alone"][k] for k in common})
    tb = eval_video_alone.totals({k: base["video_alone"][k] for k in common})
    for g in ("all", *CLICKING):
        c, b = tc[g], tb[g]
        for unit, n in (("recall", b["truth"]), ("precision", b["video"])):
            gap = SDS * share_sd(b["found"], n)
            rows.append(row("video_alone", g, round(c[unit], 4), round(b[unit], 4), round(c[unit] - b[unit], 4),
                            round(gap, 4), b[unit] - c[unit] <= gap, unit))
    return rows


# ---- models.json -----------------------------------------------------------------------------------------------------
def entry(m, cand, rows):
    """The model's models.json entry as the others are written: its size and the checks the gate measured (the speeds
    and the words about it are the user's to add)."""
    e = {}
    pt = EXPORTS / f"detector_{m.name}.pt"
    if pt.is_file():
        import torch
        ck = torch.load(pt, map_location="cpu", weights_only=False)
        e["params"] = int(ck.get("params") or sum(v.numel() for k, v in ck["model"].items()
                                                  if "running" not in k and "num_batches" not in k))
    fp32 = EXPORTS / f"detector_{m.name}_fp32.onnx"
    if fp32.is_file():
        e["kb"] = round(fp32.stat().st_size / 1024, 1)
    mv = cand["moving"]

    def kills(kinds):
        x = np.sum([mv[v][1:] for v in mv if mv[v][0] in kinds], axis=0)
        return [int(x[0]), int(x[2])]
    track = {r["unit"]: r["model"] for r in rows if r["kind"] == "tracking"}
    e["checks"] = {"static": kills(("static",)), "moving": kills(("dynamic", "switching")),
                   "tracking": [round(track["the mean gap's distance from 0"], 3),
                                round(track["mean size of the gap"], 3)]}
    e["accepted"] = f"{date.today().isoformat()}: python/model/reports/accept_{m.name}.json (the app's native review)"
    return e


def add_to_models(m, e):
    """Adds the entry to models.json before "hand" (the last), in the file's own layout. False when it is there."""
    text = MODELS.read_text(encoding="utf-8")
    if m.name in json.loads(text)["models"]:
        return False
    head = ", ".join(f'"{k}": {json.dumps(e[k])}' for k in ("params", "kb") if k in e)
    lines = [f'    "{m.name}": {{'] + ([f"      {head},"] if head else []) + \
        [f'      "{k}": {json.dumps(e[k], separators=(", ", ": "))},' for k in e if k not in ("params", "kb")]
    lines[-1] = lines[-1].rstrip(",")
    block = "\n".join(lines + ["    },", ""])
    at = text.find('    "hand": {')
    if at < 0:
        sys.exit('models.json has no "hand" entry to add the model before: add it by hand')
    new = text[:at] + block + text[at:]
    if json.loads(new)["models"].get(m.name) != json.loads(json.dumps(e)):
        sys.exit("the models.json entry would not read back as written: not changed")
    MODELS.write_text(new, encoding="utf-8")
    return True


def default_lines(best, name):
    """The one change that makes `name` the default: models.json's "default", which infer.py, the service and the
    browser all read."""
    return [f'python/model/models.json: "default": "{best}"  (to "{name}"; then bun run assets)']


# ---- the verdict -----------------------------------------------------------------------------------------------------
def table(rows):
    cols = ("check", "kind", "unit", "model", "best", "difference", "allowed", "result")
    cells = [[str(r[c]) if c != "result" else ("pass" if r["passed"] else "FAIL") for c in cols] for r in rows]
    w = [max(len(c), *(len(x[i]) for x in cells)) for i, c in enumerate(cols)]
    say("  ".join(c.ljust(w[i]) for i, c in enumerate(cols)))
    for x in cells:
        say("  ".join(v.ljust(w[i]) for i, v in enumerate(x)))


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("name", help="the model: python/model/exports/detector_<name>_u8in.onnx and detector_<name>.json")
    ap.add_argument("--list", action="store_true", help="on a pass, add the model to models.json")
    a = ap.parse_args()
    best = best_model()
    m, b = Model(a.name), Model(best)
    say(f"{m.name} against {best}, the best model")
    con = contract(m)
    scorer, programs = pin_programs(m.name)
    lib, pick = eval_vods.library(), eval_moving.picks()
    cand = evaluate(m, pick, lib, scorer, programs)
    base = cand if b.name == m.name else evaluate(b, pick, lib, scorer, programs)
    rows = judge(cand, base, con)
    passed = all(r["passed"] for r in rows)

    say("")
    table(rows)
    say("")
    for f in con["failed"]:
        say(f"contract: {f}")
    for r in rows:
        if not r["passed"] and r["check"] != "contract":
            say(f"{r['check']} {r['kind']}: {r['unit']} {r['model']} against {best}'s {r['best']} "
                f"(difference {r['difference']}, allowed {r['allowed']})")
    listed = None
    if passed and a.list:
        listed = add_to_models(m, entry(m, cand, rows))
        say(f"models.json: {m.name} added" if listed else f"models.json: {m.name} is listed already (unchanged)")
    say(f"{m.name}: {'PASS: it may reach the app' if passed else 'FAIL: it may not reach the app'}")
    lines = default_lines(best, m.name) if passed and m.name != best else []
    if lines:
        say(f"To make {m.name} the default model (the user's call), change:")
        for line in lines:
            say(f"  {line}")

    rep = dict(model=m.name, best=best, date=date.today().isoformat(), passed=passed,
               files=dict(model=m.key, best=b.key, programs=programs),
               limits=dict(draws=DRAWS, standard_deviations=SDS, seed=SEED,
                           kills_matched="no drop", flicks_recall_precision="2 SD of the best model's share over "
                           "draws of its kills", tracking="2 SD of the best model's number over draws of the runs"),
               contract=con, checks=rows, model_results=cand, best_results=base, listed=listed,
               default_lines=lines)
    out = REPORTS / f"accept_{m.name}.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(rep, indent=1, default=float))
    say(f"report: {out.relative_to(ROOT)}")
    sys.exit(0 if passed else 1)


if __name__ == "__main__":
    main()
