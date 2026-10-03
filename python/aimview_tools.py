"""Aim View's library and native review for the Python scripts, through the review service's command-line tool
(aimview-tool: service/src/bin/aimview-tool.rs). Each call runs the tool and reads the JSON it prints. This replaces
python/server.py's Library over the Python bindings (both retired: python/retired/server_thin.py,
retired/python-bindings/).

The tool runs through `cargo run --release` when cargo is on the PATH, so a change to the Rust code is built before the
call (a build takes about a minute, and waits while another release build runs); without cargo, the built
target/release/aimview-tool runs.

What the scripts use: Library (list, resolve, cache_dir, stats_for, stats_of, review_video), NAME, STATS_DEFAULT and
AREA_EXAMPLES (areas.py, model/build_kills.py, model/eval_vods.py, model/eval_moving.py, tests/find_popups.py,
tests/fixtures.py).
"""
import json
import os
import shutil
import subprocess
import tempfile
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


def command():
    """How the tool starts: through cargo (built first when the Rust code changed), else the built program."""
    if shutil.which("cargo"):
        return ["cargo", "run", "-q", "--release", "--manifest-path", str(ROOT / "Cargo.toml"),
                "-p", "aimview-service", "--bin", "aimview-tool", "--"]
    if TOOL.is_file():
        return [str(TOOL)]
    raise RuntimeError(f"no cargo on the PATH and no {TOOL} (cargo build --release -p aimview-service "
                       "--bin aimview-tool)")


def run(*args):
    """The tool's answer to a command, as Python objects. Its error raises FileNotFoundError, ValueError or
    RuntimeError; what it prints on stderr (a review's progress, cargo's build errors) shows as it comes."""
    p = subprocess.run(command() + [str(a) for a in args], stdout=subprocess.PIPE)
    try:
        out = json.loads(p.stdout)
    except ValueError:
        raise RuntimeError(f"aimview-tool {args[0]} gave no answer (exit code {p.returncode}): see above") from None
    if p.returncode != 0 and isinstance(out, dict) and "error" in out:
        raise ERRORS.get(out.get("status"), RuntimeError)(out["error"])
    return out


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


def _path(p):
    return None if p is None else Path(p)


def _key(video):
    """A video's path as one string per file (case and links resolved), to find its recording."""
    return os.path.normcase(os.path.realpath(video))


def _stamp_seconds_ok(stamp):
    """Whether a file-name time stamp (yyyy.mm.dd-hh.mm.ss) reads as a time (service/src/library/names.rs:
    stamp_seconds)."""
    if len(stamp) != 19 or any(stamp[i] != c for i, c in ((4, "."), (7, "."), (10, "-"), (13, "."), (16, "."))):
        return False
    parts = (stamp[0:4], stamp[5:7], stamp[8:10], stamp[11:13], stamp[14:16], stamp[17:19])
    if not all(p.isascii() and p.isdigit() for p in parts):
        return False
    _, month, day, h, m, s = map(int, parts)
    return 1 <= month <= 12 and 1 <= day <= 31 and h <= 23 and m <= 59 and s <= 60


class Names:
    """KovOBS's recording names ("<scenario> - <score> - <yyyy.mm.dd-hh.mm.ss>.mp4"), read as the service reads them
    (service/src/library/names.rs: parse_name): match(name) gives the parts by name (m["scenario"], m["score"],
    m["stamp"]), or None for another name."""

    @staticmethod
    def match(name):
        if not name.endswith(".mp4"):
            return None
        rest, gap, stamp = name[:-4].rpartition(" - ")
        scenario, gap2, score = rest.rpartition(" - ")
        if not gap or not gap2:
            return None
        numeric = score != "" and all((c.isascii() and c.isdigit()) or c in "-." for c in score)
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
        out = run("recordings", *self.options)
        self.vods = _path(out["vods"])
        self.recordings, self.by_id, self.by_video, self.found = [], {}, {}, {}
        for r in out["recordings"]:
            more = {k: r.pop(k) for k in ("video", "dir", "stats_file", "stats_found")}
            self.recordings.append(r)
            self.by_id[r["id"]] = more
            if more["video"]:
                self.by_video[_key(more["video"])] = r["id"]
            self.found[(r["scenario"], r["stamp"])] = more["stats_found"]

    def _recording(self, vid):
        """A recording's video, folder and stats file, by its id."""
        if vid not in self.by_id:
            self.by_id[vid] = run("lookup", *self.options, "--id", vid)["ids"][0]
        return self.by_id[vid]

    def list(self):
        """The recordings, newest first: what /api/vods gives."""
        return [dict(r) for r in self.recordings]

    def resolve(self, vid):
        """A recording's video (its id: its path in the VODs folder, or uploads/<name>); FileNotFoundError if none."""
        video = self._recording(vid)["video"]
        if video is None:
            raise FileNotFoundError(f"no recording {vid}")
        return Path(video)

    def cache_dir(self, vid):
        """A recording's folder: its reviews, areas and marks."""
        return Path(self._recording(vid)["dir"])

    def stats_for(self, scenario, stamp):
        """KovaaK's stats file for a run of the scenario that ended at the time stamp (within 5 s), or None."""
        if (scenario, stamp) not in self.found:
            self.found[(scenario, stamp)] = run("lookup", *self.options, "--run", scenario, stamp)["runs"][0]["stats_file"]
        return _path(self.found[(scenario, stamp)])

    def stats_of(self, vid, video):
        """The stats file for the recording at `video`: the user's choice, else one uploaded beside it, else by name
        and time; by name and time for a video outside the library. `vid` is not used (the video says which)."""
        found = self.by_video.get(_key(video))
        if found is not None:
            return _path(self.by_id[found]["stats_file"])
        return _path(run("lookup", *self.options, "--video", video)["videos"][0]["stats_file"])

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
            args += ["--areas", json.dumps([list(a) for a in areas])]
        if window is not None:
            args += ["--window", window[0], window[1]]
        if quiet:
            args.append("--quiet")
        with tempfile.TemporaryDirectory() as tmp:
            folder = Path(tmp) if out is None else Path(out)
            answer = run(*args, "--out", folder, *([] if out is not None else ["--no-report"]))
            result = {name: json.loads((folder / f"{name}.json").read_bytes())
                      for name in ("tracks", "readings", "hud")}
            result["seconds"] = answer["seconds"]
            if answer["report"]:
                result["report"] = json.loads((folder / "report.json").read_bytes())
        return result
