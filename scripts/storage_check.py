"""The storage check (docs/storage-design.md): what the review service keeps, compared between two builds. Each run
copies the app data of test_out (Python's layout) and the desktop app's data folder afresh, asks one build of
service/examples/api.rs a fixed list of questions and changes (reads and writes on a few recordings: marks, picks,
lists, area kinds, saved areas, a cut-off and its labels), and keeps every answer and a hash of every file left. Two
runs agree when their answers and files are the same. Nothing runs on the GPU: the areas are saved for a recording with
no review, so no review is made again.

Folders come from aimview.defaults.json and aimview.json (python/local_config.py); the desktop app's data folder is
its identifier (desktop/tauri.conf.json) in the roaming app data. The runs go in test_out/storage_check/<name>/.

Usage: python scripts/storage_check.py <name> <api exe>
           (the exe: cargo build --profile quick -p aimview-service --example api, then copy
           target/quick/examples/api.exe aside per build)
       python scripts/storage_check.py compare <name a> <name b>
"""
import glob
import hashlib
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path
from urllib.parse import quote

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO / "python"))
import local_config  # noqa: E402

DATA = local_config.required(local_config.folder("data"), "data folder")
VODS = local_config.required(local_config.folder("vods"), "recordings' folder (vods)")
STATS = local_config.required(local_config.kovaak("stats"), "KovaaK's stats folder (Steam's)")
MODELS = local_config.required(local_config.folder("models"), "models folder")
DESKTOP = Path(os.environ.get("APPDATA", Path.home())) / json.loads(
    (REPO / "desktop" / "tauri.conf.json").read_text(encoding="utf-8"))["identifier"]
CHECKS = DATA / "storage_check"
# Python's layout in the data folder: the parts the service keeps or reads (config.rs: Layout::Python)
PYTHON_PARTS = ("vod_app", "vod_uploads", "mouse", "vod_model/hand/cutoff")

# the recordings asked about (in the recordings' folder, or uploads): a model's review with areas and a cut-off; two
# models' reviews; an old review only; uploads with and without areas; one with no review (its areas are saved); one
# in the desktop app's data
WITH_CUTOFF = "1wall 6targets extra small/1wall 6targets extra small - 889.26 - 2026.10.01-16.17.48.mp4"
TWO_MODELS = "1w4ts Voltaic/1w4ts Voltaic - 143 - 2026.09.30-04.55.23.mp4"
OLD_ONLY = "10 Sphere Hipfire Extra Small/10 Sphere Hipfire Extra Small - 1550 - 2026.08.26-04.40.26.mp4"
UPLOAD = "uploads/1902 1wall 6targets small ｜ #2.mp4"
UPLOAD_PLAIN = "uploads/ww3t 141vodh264.mp4"
NO_REVIEW = "1 wall 6 targets Micro++/1 wall 6 targets Micro++ - 9801 - 2026.05.03-02.04.28.mp4"
DESKTOP_ONE = "1wall 2targets xsmall - valorant/1wall 2targets xsmall - valorant - 558.46 - 2026.10.01-16.23.04.mp4"
# the GET routes asked about each recording
QUESTIONS = ("job", "report", "run", "stats", "exclude", "faint", "mouse", "tracks", "find_areas")
# how long the cut-off's labels take to be written in the background
LABELS_WAIT_S = 40
# a recording's name as KovOBS writes it: "<scenario> - <score> - <stamp>"
NAME_PARTS = 3


def q(text):
    """`text` escaped whole for a URL's query (a slash too)."""
    return quote(text, safe="")


def stats_name(video_id):
    """The stats file of a recording named as KovOBS names one, by its scenario and minute; None otherwise."""
    parts = Path(video_id).stem.rsplit(" - ", NAME_PARTS - 1)
    if len(parts) != NAME_PARTS:
        return None
    scenario, _, stamp = parts
    near = sorted(glob.glob(str(STATS / f"{glob.escape(scenario)} - Challenge - {stamp[:13]}*")))
    return Path(near[0]).name if near else None


def requests(ids, saved_areas):
    """The questions and changes, as service/examples/api.rs reads them."""
    lines = ["GET /api/vods", "GET /api/vods?quick=1", "GET /api/models", "GET /api/info", "GET /api/exclude",
             "GET /api/label_queue", "GET /api/faint_queue", "GET /api/area_examples"]
    lines += [f"GET /api/{path}?id={q(video_id)}" for video_id in ids for path in QUESTIONS]
    first, last = ids[0], ids[-1]
    lines += [
        f"POST /api/run?id={q(first)}\t" + json.dumps({"start": 5, "end": 30, "length": None}),
        f"GET /api/run?id={q(first)}", f"GET /api/report?id={q(first)}",
        f"POST /api/run?id={q(first)}\t" + json.dumps({"start": None, "end": None, "length": None}),
        f"GET /api/run?id={q(first)}",
        "POST /api/area_kinds\t" + json.dumps({"name": "Storage check"}),
        f"POST /api/label_skip?id={q(last)}", f"POST /api/not_aim?id={q(last)}&on=1", "GET /api/label_queue",
        f"POST /api/not_aim?id={q(last)}&on=0",
        f"POST /api/faint?id={q(first)}\t" + json.dumps({"on": True, "offset": 0.4}),
        f"GET /api/faint?id={q(first)}", f"GET /api/report?id={q(first)}",
        f"POST /api/faint_skip?id={q(last)}", "GET /api/faint_queue",
    ]
    for video_id in ids:
        name = stats_name(video_id)
        if name:
            lines += [f"POST /api/stats?id={q(video_id)}\t" + json.dumps({"file": name, "source": "kovaak"}),
                      f"GET /api/stats?id={q(video_id)}"]
    lines += [f"POST /api/stats?id={q(first)}\t" + json.dumps({"file": None}), f"GET /api/stats?id={q(first)}"]
    if saved_areas is not None:
        lines += [f"POST /api/exclude?id={q(NO_REVIEW)}\t{json.dumps(saved_areas)}", f"GET /api/exclude?id={q(NO_REVIEW)}",
                  f"GET /api/find_areas?id={q(NO_REVIEW)}", f"GET /api/job?id={q(NO_REVIEW)}"]
        lines += [f"POST /api/faint_submit?id={q(WITH_CUTOFF)}&offset=0.35", f"SLEEP {LABELS_WAIT_S}",
                  f"GET /api/faint?id={q(WITH_CUTOFF)}"]
    return lines + ["GET /api/label_queue", "GET /api/faint_queue", "GET /api/area_examples", "GET /api/vods"]


def tree(root):
    """Every file under root: its path and its bytes' hash."""
    return {path.relative_to(root).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in sorted(root.rglob("*")) if path.is_file()}


def run(name, exe):
    """One run of the build `exe` into test_out/storage_check/<name>/: copies both data folders there afresh, asks
    the questions on each, and keeps per layout the answers, the error output and every file's hash left after."""
    out = CHECKS / name
    if out.exists():
        shutil.rmtree(out)
    python_data, app_data = out / "python", out / "app"
    for part in PYTHON_PARTS:
        shutil.copytree(DATA / part, python_data / part)
    shutil.copytree(DESKTOP, app_data)
    (out / "day.txt").write_text(time.strftime("%Y.%m.%d"), encoding="utf-8")
    saved = json.loads((DATA / "vod_app" / cutoff_slug() / "exclude.json").read_text(encoding="utf-8"))
    runs = (("python", python_data, [WITH_CUTOFF, TWO_MODELS, OLD_ONLY, UPLOAD, UPLOAD_PLAIN], saved),
            ("app", app_data, [DESKTOP_ONE, WITH_CUTOFF], None))
    for layout, data, ids, saved_areas in runs:
        asked = out / f"{layout}_requests.txt"
        asked.write_text("\n".join(requests(ids, saved_areas)) + "\n", encoding="utf-8")
        args = [exe, str(data), str(MODELS), str(asked), "--layout", layout, "--stats", str(STATS)]
        if layout == "python":
            args += ["--vods", str(VODS)]
        answers = subprocess.run(args, capture_output=True, text=True, encoding="utf-8", check=False)
        (out / f"{layout}_answers.jsonl").write_text(answers.stdout, encoding="utf-8")
        (out / f"{layout}_stderr.txt").write_text(answers.stderr, encoding="utf-8")
        (out / f"{layout}_tree.json").write_text(json.dumps(tree(data), indent=1), encoding="utf-8")
        print(layout, "exit", answers.returncode, "answers", len(answers.stdout.splitlines()))


def steady(line, days):
    """An answer without what changes from run to run: recordings made on the runs' days (the user may be playing), a
    submit's time; None for the log's lines."""
    if not line.startswith("{"):
        return None

    def clean(value):
        """The JSON value without list items that name one of the days, and with each "submitted" time replaced."""
        if isinstance(value, list):
            return [clean(item) for item in value if not any(day in json.dumps(item) for day in days)]
        if isinstance(value, dict):
            return {key: ("<time>" if key == "submitted" else clean(item)) for key, item in value.items()}
        return value
    return json.dumps(clean(json.loads(line)), ensure_ascii=False, sort_keys=True)


def compare(a, b):
    """Whether runs `a` and `b` agree: prints, per layout, whether their answers, files and error output are the
    same (less what changes from run to run, and the cut-off's faint.json, which keeps its submit's time)."""
    days = {(CHECKS / name / "day.txt").read_text(encoding="utf-8") for name in (a, b)}
    # the cut-off keeps its submit's time
    timed = Path("vod_app") / cutoff_slug() / "faint.json"
    same = True
    for layout in ("python", "app"):
        for kind in ("answers.jsonl", "tree.json", "stderr.txt"):
            left = (CHECKS / a / f"{layout}_{kind}").read_text(encoding="utf-8")
            right = (CHECKS / b / f"{layout}_{kind}").read_text(encoding="utf-8")
            if kind == "tree.json":
                left, right = json.loads(left), json.loads(right)
                diff = sorted(key for key in left.keys() | right.keys()
                              if left.get(key) != right.get(key) and key != timed.as_posix())
            else:
                left, right = left.splitlines(), right.splitlines()
                if kind == "answers.jsonl":
                    left = [line for line in (steady(line, days) for line in left) if line is not None]
                    right = [line for line in (steady(line, days) for line in right) if line is not None]
                diff = [i for i in range(max(len(left), len(right))) if left[i:i + 1] != right[i:i + 1]]
            print(layout, kind, "same" if not diff else f"{len(diff)} differ: {diff[:20]}")
            same = same and not diff
    print("SAME" if same else "DIFFERENT")
    return same


def cutoff_slug():
    """The cut-off's recording's folder name (library/names.rs: slug)."""
    out, gap = [], False
    for character in Path(WITH_CUTOFF).stem:
        if character.isalnum() or character in "_.-":
            out.append(character)
            gap = False
        elif not gap:
            out.append("_")
            gap = True
    return "".join(out)


if __name__ == "__main__":
    if sys.argv[1] == "compare":
        sys.exit(0 if compare(sys.argv[2], sys.argv[3]) else 1)
    run(sys.argv[1], sys.argv[2])
