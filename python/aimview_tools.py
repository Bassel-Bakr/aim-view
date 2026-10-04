"""Aim View's library and native review for the Python scripts, through the review service's command-line tool
(aimview-tool: service/src/bin/aimview-tool.rs). Each call runs the tool and reads the JSON it prints. This replaces
python/server.py's Library over the Python bindings (both retired: python/retired/server_thin.py,
retired/python-bindings/).

The tool runs through `cargo run --release` when cargo is on the PATH, so a change to the Rust code is built before the
call (a build takes about a minute, and waits while another release build runs); without cargo, the built
target/release/aimview-tool runs.

What the scripts use: Library (list, resolve, cache_dir, stats_for, stats_of, review_video), NAME, STATS_DEFAULT and
AREA_EXAMPLES (areas.py, model/build_kills.py, model/eval_vods.py, model/eval_moving.py; once also the retired
python/retired/tests/ scripts).
"""
import json
import os
import shutil
import subprocess
import tempfile
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
AREA_EXAMPLES = ROOT / "test_out" / "vod_app" / "area_examples.jsonl"   # the saved areas the area finder learns from
VODS_DEFAULT = r"E:\OBS\KovOBS"
STATS_DEFAULT = r"C:\Program Files (x86)\Steam\steamapps\common\FPSAimTrainer\FPSAimTrainer\stats"
TOOL = ROOT / "target" / "release" / ("aimview-tool.exe" if os.name == "nt" else "aimview-tool")
# the library's settings: aimview-tool's library options (download_ffmpeg: --download-ffmpeg)
SETTINGS = ("data", "layout", "models", "vods", "stats", "scenarios", "device", "ffmpeg", "download_ffmpeg")
# the tool's error statuses as Python's exceptions (anything else: RuntimeError)
ERRORS = {404: FileNotFoundError, 400: ValueError}
# a file-name time stamp, yyyy.mm.dd-hh.mm.ss: its length, its separators by place, and its fields' places
STAMP_LENGTH = 19
STAMP_SEPARATORS = ((4, "."), (7, "."), (10, "-"), (13, "."), (16, "."))
STAMP_FIELDS = ((0, 4), (5, 7), (8, 10), (11, 13), (14, 16), (17, 19))
MONTHS, DAYS, LAST_HOUR, LAST_MINUTE, LAST_SECOND = 12, 31, 23, 59, 60   # a leap second reads as a time too


def command():
    """How the tool starts: through cargo (built first when the Rust code changed), else the built program."""
    if shutil.which("cargo"):
        return ["cargo", "run", "-q", "--release", "--manifest-path", str(ROOT / "Cargo.toml"),
                "-p", "aimview-service", "--bin", "aimview-tool", "--"]
    if TOOL.is_file():
        return [str(TOOL)]
    raise RuntimeError(f"no cargo on the PATH and no {TOOL} (cargo build --release -p aimview-service "
                       "--bin aimview-tool)")


def in_parallel(calls, at_once):
    """Each call (a function of no arguments, such as a review: a process of its own) run `at_once` at a time. Yields
    (the call's index, its result) as each ends; a call's error is raised in its turn, and the calls not started then
    are dropped."""
    pool = ThreadPoolExecutor(max_workers=max(1, at_once))
    try:
        futures = {pool.submit(call): index for index, call in enumerate(calls)}
        for future in as_completed(futures):
            yield futures[future], future.result()
    finally:
        pool.shutdown(cancel_futures=True)


def run(*args):
    """The tool's answer to a command, as Python objects. Its error raises FileNotFoundError, ValueError or
    RuntimeError; what it prints on stderr (a review's progress, cargo's build errors) shows as it comes."""
    process = subprocess.run(command() + [str(arg) for arg in args], stdout=subprocess.PIPE)
    try:
        answer = json.loads(process.stdout)
    except ValueError:
        raise RuntimeError(f"aimview-tool {args[0]} gave no answer (exit code {process.returncode}): see "
                           "above") from None
    if process.returncode != 0 and isinstance(answer, dict) and "error" in answer:
        raise ERRORS.get(answer.get("status"), RuntimeError)(answer["error"])
    return answer


def options(settings):
    """The library's settings as the tool's options. vods=None: the VODs folder chosen in the app."""
    out = []
    for key, value in settings.items():
        if key not in SETTINGS:
            raise ValueError(f"no setting called {key} (the settings: {', '.join(SETTINGS)})")
        flag = "--" + key.replace("_", "-")
        if key == "vods" and value is None:
            out.append(f"{flag}=")
        elif value is None or value is False:
            continue
        elif value is True:
            out.append(flag)
        elif isinstance(value, (list, tuple)):
            out += [flag, *map(str, value)]
        else:
            out.append(f"{flag}={value}")
    return out


def _path(path):
    return None if path is None else Path(path)


def _key(video):
    """A video's path as one string per file (case and links resolved), to find its recording."""
    return os.path.normcase(os.path.realpath(video))


def _stamp_seconds_ok(stamp):
    """Whether a file-name time stamp (yyyy.mm.dd-hh.mm.ss) reads as a time (service/src/library/names.rs:
    stamp_seconds)."""
    if len(stamp) != STAMP_LENGTH or any(stamp[at] != separator for at, separator in STAMP_SEPARATORS):
        return False
    fields = [stamp[start:end] for start, end in STAMP_FIELDS]
    if not all(field.isascii() and field.isdigit() for field in fields):
        return False
    _, month, day, hour, minute, second = map(int, fields)
    return (1 <= month <= MONTHS and 1 <= day <= DAYS and hour <= LAST_HOUR and minute <= LAST_MINUTE
            and second <= LAST_SECOND)


class Names:
    """KovOBS's recording names ("<scenario> - <score> - <yyyy.mm.dd-hh.mm.ss>.mp4"), read as the service reads them
    (service/src/library/names.rs: parse_name): match(name) gives the parts by name (m["scenario"], m["score"],
    m["stamp"]), or None for another name."""

    @staticmethod
    def match(name):
        if not name.endswith(".mp4"):
            return None
        rest, gap, stamp = name.removesuffix(".mp4").rpartition(" - ")
        scenario, score_gap, score = rest.rpartition(" - ")
        if not gap or not score_gap:
            return None
        numeric = score != "" and all((char.isascii() and char.isdigit()) or char in "-." for char in score)
        if not numeric or not scenario or not _stamp_seconds_ok(stamp):
            return None
        try:
            return dict(scenario=scenario, score=float(score), stamp=stamp)
        except ValueError:
            return None


NAME = Names()


class Library:
    """The review service's library (aimview_service::Library) on this computer's data, as the scripts use it. The
    recordings and their stats files are read once, when it opens (one run of the tool for every recording)."""

    def __init__(self, vods=VODS_DEFAULT, stats=STATS_DEFAULT, **config):
        """config: the library's other settings (data, layout, models, scenarios: a list, device, ffmpeg,
        download_ffmpeg: True; see `aimview-tool help`). The defaults: the repo's test_out/ in Python's layout, the
        models in python/model/exports, KovaaK's scenario folders, the detector on the GPU, ffmpeg from the PATH."""
        self.options = options(dict(config, vods=vods, stats=stats))
        answer = run("recordings", *self.options)
        self.vods = _path(answer["vods"])
        self.recordings, self.by_id, self.by_video, self.found = [], {}, {}, {}
        for recording in answer["recordings"]:
            more = {key: recording.pop(key) for key in ("video", "dir", "stats_file", "stats_found")}
            self.recordings.append(recording)
            self.by_id[recording["id"]] = more
            if more["video"]:
                self.by_video[_key(more["video"])] = recording["id"]
            self.found[(recording["scenario"], recording["stamp"])] = more["stats_found"]

    def _recording(self, recording_id):
        """A recording's video, folder and stats file, by its id."""
        if recording_id not in self.by_id:
            self.by_id[recording_id] = run("lookup", *self.options, "--id", recording_id)["ids"][0]
        return self.by_id[recording_id]

    def list(self):
        """The recordings, newest first: what /api/vods gives."""
        return [dict(recording) for recording in self.recordings]

    def resolve(self, recording_id):
        """A recording's video (its id: its path in the VODs folder, or uploads/<name>); FileNotFoundError if none."""
        video = self._recording(recording_id)["video"]
        if video is None:
            raise FileNotFoundError(f"no recording {recording_id}")
        return Path(video)

    def cache_dir(self, recording_id):
        """A recording's folder: its reviews, areas and marks."""
        return Path(self._recording(recording_id)["dir"])

    def stats_for(self, scenario, stamp):
        """KovaaK's stats file for a run of the scenario that ended at the time stamp (within 5 s), or None."""
        if (scenario, stamp) not in self.found:
            runs = run("lookup", *self.options, "--run", scenario, stamp)["runs"]
            self.found[(scenario, stamp)] = runs[0]["stats_file"]
        return _path(self.found[(scenario, stamp)])

    def stats_of(self, recording_id, video):
        """The stats file for the recording at `video`: the user's choice, else one uploaded beside it, else by name
        and time; by name and time for a video outside the library. `recording_id` is not used (the video says
        which)."""
        found = self.by_video.get(_key(video))
        if found is not None:
            return _path(self.by_id[found]["stats_file"])
        return _path(run("lookup", *self.options, "--video", video)["videos"][0]["stats_file"])

    def scenarios(self):
        """Every scenario's facts by lower-case name, as the core reads the scenario folders (aimview-tool scenarios:
        {kind, limit, targets, reload}; a UTF-16 file read as UTF-16). Read once."""
        if getattr(self, "_scenarios", None) is None:
            self._scenarios = run("scenarios", *self.options)
        return self._scenarios

    def scenario_kinds(self):
        """{scenario: kind}, as old_review.scenario_kinds gives them."""
        return {name: facts["kind"] for name, facts in self.scenarios().items()}

    def scenario_facts(self):
        """{scenario: (kind, time limit)}, as old_review.scenario_facts gives them."""
        return {name: (facts["kind"], facts["limit"]) for name, facts in self.scenarios().items()}

    def target_counts(self):
        """{scenario: targets alive at once}, as old_review.target_counts gives them."""
        return {name: facts["targets"] for name, facts in self.scenarios().items() if facts["targets"] is not None}

    def load_stats_index(self):
        """Nothing to do: the stats files were paired when the library opened."""

    def review_video(self, video, model=None, out=None, *, stats=None, kind=None, limit=None, cap=None, areas=None,
                     runs=None, batch=4, window=None, quiet=False):
        """A video reviewed as the app reviews a recording, without touching the library's reviews (`aimview-tool
        review`): {tracks, readings, hud, seconds}. With `out`, a folder: tracks.json, readings.json, hud.json and
        the report the app shows (report.json, and `report` here) are written there; without it, no report.

        model: a model's name or a _u8in .onnx file (None: the app's pick). stats: the run's stats file. kind, limit,
        cap: the scenario's kind ("static", "dynamic", "tracking", "switching"), time limit (s) and targets alive at
        once. areas: [[x0, y0, x1, y1, kind], ...] to leave out (None: the recording's in the app, else KovOBS's).
        runs: the parts reviewed at once (None: 2 with 8 threads or more). window: (start, end) in seconds, only that
        part tracked. quiet: no progress on stderr."""
        args = ["review", video, *self.options, "--batch", batch]
        for flag, value in (("--model", model), ("--stats-file", stats), ("--kind", kind), ("--limit", limit),
                            ("--cap", cap), ("--runs", runs)):
            if value is not None:
                args += [flag, value]
        if areas is not None:
            args += ["--areas", json.dumps([list(area) for area in areas])]
        if window is not None:
            args += ["--window", window[0], window[1]]
        if quiet:
            args.append("--quiet")
        with tempfile.TemporaryDirectory() as temporary:
            folder = Path(temporary) if out is None else Path(out)
            answer = run(*args, "--out", folder, *([] if out is not None else ["--no-report"]))
            result = {name: json.loads((folder / f"{name}.json").read_bytes())
                      for name in ("tracks", "readings", "hud")}
            result["seconds"] = answer["seconds"]
            if answer["report"]:
                result["report"] = json.loads((folder / "report.json").read_bytes())
        return result
