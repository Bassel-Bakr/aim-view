"""The VOD review app: a small local web server (Python standard library) for python/app/.

It lists the KovOBS recordings, finds each one's stats file, runs the review pipeline (review.py) in the background
when asked, caches the results under test_out/vod_app/, and streams the videos to the page with range requests, so the
page can jump to any flick. It listens on 127.0.0.1 only.
Usage: python python/server.py [--vods E:/OBS/KovOBS] [--stats <KovaaK's stats folder>] [--port 8770]
Then open http://127.0.0.1:8770/
"""
import argparse
import functools
import json
import mimetypes
import os
import re
import sys
import threading
import time
import traceback
from datetime import datetime
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse

import areas
import review

HERE = Path(__file__).resolve().parent
APP = HERE / "app"
CACHE = HERE.parent / "test_out" / "vod_app"
EXPORTS = HERE / "model" / "exports"
MODELS = HERE / "model" / "models.json"                # what the model picker shows about each model
SETTINGS = CACHE / "settings.json"                     # the model the user picked, kept across restarts
UPLOADS = HERE.parent / "test_out" / "vod_uploads"      # VODs uploaded from the page, any name; ids start "uploads/"
EXCLUDE_UPLOADS = CACHE / "exclude_uploads.json"        # the exclude areas last saved for an upload
AREA_EXAMPLES = CACHE / "area_examples.jsonl"            # the saved areas the area finder learns from (areas.py)
AREA_KINDS = CACHE / "area_kinds.json"                  # the types of area the user added, with what each is
NOT_AIM = CACHE / "not_aim_trainer.json"                # recordings the user marked as another game, not an aim trainer
LABEL_SKIPPED = CACHE / "label_skipped.json"            # recordings skipped in the labelling queue
FAINT_SKIPPED = CACHE / "faint_skipped.json"            # recordings skipped in the cut-off queue
CUTOFF_LABELS = HERE.parent / "test_out" / "vod_model" / "hand" / "cutoff"   # detector labels from submitted cut-offs
KIND_ABOUT = {"Session stats": "KovaaK's SESSION box (kills, accuracy, damage), or a game's score and accuracy boxes",
              "Timer": "the run's time left", "Clock": "the time of day, a session clock, FPS",
              "Scenario name": "the scenario's name", "Magazine": "the ammo count", "Weapon": "the weapon's name or model",
              "Settings": "a box of settings (sensitivity, FOV, theme, sounds)", "Webcam": "a hand cam, face cam or avatar",
              "Version": "a version number", "Other": "anything else that is not the game",
              "Zoomed crosshair": "a magnified view of the screen round the crosshair"}
VIDEO_TYPES = (".mp4", ".mkv", ".mov", ".webm")
STATS_DEFAULT = r"C:\Program Files (x86)\Steam\steamapps\common\FPSAimTrainer\FPSAimTrainer\stats"
NAME = re.compile(r"^(?P<scenario>.+) - (?P<score>[-\d.]+) - (?P<stamp>\d{4}\.\d\d\.\d\d-\d\d\.\d\d\.\d\d)\.mp4$")
STATS_NAME = re.compile(r"^(?P<scenario>.+) - Challenge - (?P<stamp>\d{4}\.\d\d\.\d\d-\d\d\.\d\d\.\d\d) Stats\.csv$")


EPOCH = datetime(2000, 1, 1)


def stamp_seconds(stamp):
    """A file-name time stamp as seconds (local time, no time zone needed: both sides use the same clock). Some KovOBS
    recordings from June 2026 are named with the year 0026: read as 2026."""
    if stamp.startswith("00"):
        stamp = "20" + stamp[2:]
    try:
        return (datetime.strptime(stamp, "%Y.%m.%d-%H.%M.%S") - EPOCH).total_seconds()
    except ValueError:
        return None


class Library:
    def __init__(self, vods, stats):
        self.vods, self.stats = Path(vods), Path(stats)
        self.index, self.jobs, self.lock = {}, {}, threading.Lock()
        self.model, self.detector = "hand", None          # use() sets the model
        self.loaded = {}                                  # detectors by model name, each loaded once
        if AREA_EXAMPLES.exists():                        # examples from before type ids held the type's name
            ex, kinds = areas.load_examples(AREA_EXAMPLES), self.kinds()
            fixed = [dict(e, kind=e["kind"] if e["kind"] == areas.NONE else self.kind_id(e["kind"], kinds)) for e in ex]
            if fixed != ex:
                with open(AREA_EXAMPLES, "w", encoding="utf-8") as fh:
                    for e in fixed:
                        fh.write(json.dumps(e) + "\n")

    def load_stats_index(self):
        """Stats files by (scenario, time). The folder holds tens of thousands of files; one listing is quick."""
        idx = {}
        for name in os.listdir(self.stats):
            m = STATS_NAME.match(name)
            if m:
                t = stamp_seconds(m["stamp"])
                if t is not None:
                    idx.setdefault(m["scenario"], []).append((t, name))
        self.index = idx

    def stats_for(self, scenario, stamp):
        t = stamp_seconds(stamp)
        if t is None:
            return None
        cands = [(abs(ts - t), n) for ts, n in self.index.get(scenario, []) if abs(ts - t) <= 5]
        return self.stats / min(cands)[1] if cands else None

    def resolve(self, vid):
        root, rel = (UPLOADS, vid[len("uploads/"):]) if vid.startswith("uploads/") else (self.vods, vid)
        p = (root / rel).resolve()
        if root.resolve() not in p.parents or p.suffix.lower() not in VIDEO_TYPES or not p.exists():
            raise FileNotFoundError(vid)
        return p

    def stats_of(self, video):
        """The stats file for a VOD: an uploaded one beside it (same name, .csv), else by name and time."""
        side = video.with_suffix(".csv")
        if video.parent.resolve() == UPLOADS.resolve() and side.exists():
            return side
        m = NAME.match(video.with_suffix(".mp4").name)
        return self.stats_for(m["scenario"], m["stamp"]) if m else None

    @staticmethod
    def cache_dir(vid):
        return CACHE / re.sub(r"[^\w.-]+", "_", Path(vid).stem)

    def use(self, model):
        """Reviews from now on use this model (a name from models(), "hand", or a .pt/.onnx path)."""
        if model not in self.loaded:
            self.loaded[model] = load_detector(model)
        with self.lock:                                   # a file's reviews are kept under its name
            self.model = Path(model).stem if model.endswith((".pt", ".onnx")) else model
            self.detector = self.loaded[model]

    def pick(self, model):
        """The user's pick in the model panel: used, and kept for the next start."""
        if not any(m["name"] == model and m["available"] for m in self.models()["models"]):
            raise ValueError(f"no model called {model} can run here")
        self.use(model)
        json.dump(dict(model=model), open(SETTINGS, "w"))
        return self.models()

    def models(self):
        """The models to pick from: the ones models.json describes (with their checks and speeds), then every other
        exported one as an older model. available: it can run here (a .pt file needs the GPU)."""
        info = json.load(open(MODELS, encoding="utf-8"))
        gpu = has_cuda()
        files = {p.name for p in EXPORTS.glob("detector_*")}
        names = {re.sub(r"^detector_|_fp32\.onnx$|\.pt$", "", f) for f in files if f.endswith(("_fp32.onnx", ".pt"))}

        def available(n):
            return n == "hand" or (gpu and f"detector_{n}.pt" in files) or f"detector_{n}_fp32.onnx" in files \
                or f"detector_{n}_u8in.onnx" in files

        def order(n):                                     # tiny, small, full; then by version
            m = re.match(r"(tiny|small|full)(?:_v(\d+))?$", n)
            return (("tiny", "small", "full").index(m[1]), int(m[2] or 1)) if m else (3, 0)

        best = infer_module().BEST
        out = [dict(m, name=n, label=m.get("label", n), default=n == best, available=available(n))
               for n, m in info["models"].items() if n == "hand" or n in names]
        for n in sorted(names - set(info["models"]), key=order):
            fp32 = EXPORTS / f"detector_{n}_fp32.onnx"
            out.append(dict(name=n, label=n, older=True, available=available(n),
                            kb=round(fp32.stat().st_size / 1024, 1) if fp32.exists() else None))
        return dict(chosen=self.model, device=getattr(self.detector, "dev", "cpu"),
                    speed=info["speed"], checks=info["checks"], checked_on=info["checked_on"], models=out)

    def shown(self, vid):
        """The review to show for a recording: (model, folder). The chosen model's; else one from before reviews were
        kept per model, the one the user has been working with (in the recording's own folder; its model is "hand"
        when the hand-written detector made it, else None: not recorded); else the newest one by another model. With
        no review: the chosen model's folder, for a new one."""
        d = self.cache_dir(vid)
        own = d / "models" / self.model
        if (own / "tracks.json").exists():
            return self.model, own
        old = d / "tracks.json"
        if old.exists():
            with open(old, "rb") as fh:                   # the detector's name ends the file; hand-written has none
                fh.seek(max(0, old.stat().st_size - 200))
                return (None if b'"detector"' in fh.read() else "hand"), d
        other = max((d / "models").glob("*/tracks.json"), key=lambda p: p.stat().st_mtime, default=None)
        if other:
            return other.parent.name, other.parent
        return self.model, own

    @functools.cached_property
    def scenario_kinds(self):
        """Each scenario's kind by lower-case name (review.scenario_kinds), read once: it reads every scenario file."""
        return review.scenario_kinds()

    def reviewed(self, vid):
        d = self.cache_dir(vid)
        return (d / "tracks.json").exists() or any((d / "models").glob("*/tracks.json"))

    def list(self):
        out = []
        for p in self.vods.glob("*/*.mp4"):
            m = NAME.match(p.name)
            if not m:
                continue
            vid = p.relative_to(self.vods).as_posix()
            st = self.stats_for(m["scenario"], m["stamp"])
            out.append(dict(id=vid, scenario=m["scenario"], kind=self.scenario_kinds.get(m["scenario"].lower()),
                            score=float(m["score"]), stamp=m["stamp"],
                            mtime=p.stat().st_mtime, size=p.stat().st_size, stats=bool(st),
                            analysed=self.reviewed(vid)))
        for p in UPLOADS.glob("*"):
            if p.suffix.lower() not in VIDEO_TYPES:
                continue
            vid = f"uploads/{p.name}"
            m = NAME.match(p.with_suffix(".mp4").name)
            out.append(dict(id=vid, scenario=m["scenario"] if m else p.stem,
                            kind=self.scenario_kinds.get((m["scenario"] if m else p.stem).lower()),
                            score=float(m["score"]) if m else None,
                            stamp=m["stamp"] if m else datetime.fromtimestamp(p.stat().st_mtime).strftime("%Y.%m.%d-%H.%M.%S"),
                            mtime=p.stat().st_mtime, size=p.stat().st_size, stats=bool(self.stats_of(p)), uploaded=True,
                            analysed=self.reviewed(vid)))
        other = self.not_aim()
        for v in out:
            v["not_aim"] = v["id"] in other
        out.sort(key=lambda v: -v["mtime"])
        return out

    @staticmethod
    def not_aim():
        return set(json.load(open(NOT_AIM, encoding="utf-8"))) if NOT_AIM.exists() else set()

    def set_not_aim(self, vid, on):
        """Mark a recording as another game (not an aim trainer), or unmark it: a marked one is left out of the
        labelling queue and of what the area finder learns."""
        self.resolve(vid)
        ids = self.not_aim()
        (ids.add if on else ids.discard)(vid)
        json.dump(sorted(ids), open(NOT_AIM, "w", encoding="utf-8"), indent=1)
        return dict(id=vid, not_aim=on)

    @staticmethod
    def kinds():
        """The area types: [{id, name, about}], built-in ones first. Each keeps its id; its name and what it is can be
        changed at any time (saved areas and the area finder's examples store the id, the user's wish, 2026-10-02)."""
        data = json.load(open(AREA_KINDS, encoding="utf-8")) if AREA_KINDS.exists() else []
        if not data or any("id" not in k for k in data):     # first use, or the list of added types without ids
            own_by = {k["name"].lower(): k for k in data}
            out = []
            for name in review.EXCLUDE_KINDS:                # a type the user added before it was built in: theirs
                mine = own_by.pop(name.lower(), None)
                out.append(dict(id=_slug(name), name=name, about=(mine or {}).get("about") or KIND_ABOUT.get(name, "")))
            for k in own_by.values():
                out.append(dict(id=_new_id(k["name"], out), name=k["name"], about=k.get("about", "")))
            json.dump(out, open(AREA_KINDS, "w", encoding="utf-8"), indent=1)
            data = out
        return data

    def kind_id(self, v, kinds=None):
        """A type's id from its id or its name (saved areas from before ids held names); unknown: "other"."""
        kinds = kinds or self.kinds()
        if any(v == k["id"] for k in kinds):
            return v
        return next((k["id"] for k in kinds if str(v).lower() == k["name"].lower()), "other")

    def save_kind(self, kid, name, about):
        """A new type (no id), or a type's new name and description."""
        name, about = str(name or "").strip()[:40], str(about or "").strip()[:200]
        if not name:
            raise ValueError("a type needs a name")
        kinds = self.kinds()
        if any(k["name"].lower() == name.lower() and k["id"] != kid for k in kinds):
            raise ValueError(f"there is a type called {name} already")
        if kid:
            k = next((k for k in kinds if k["id"] == kid), None)
            if k is None:
                raise ValueError(f"no type with the id {kid}")
            k.update(name=name, about=about)
        else:
            kinds.append(dict(id=_new_id(name, kinds), name=name, about=about))
        json.dump(kinds, open(AREA_KINDS, "w", encoding="utf-8"), indent=1)
        return kinds

    def skip_label(self, vid):
        """Skipped in the labelling queue: left out of it from now on (it started from the same recordings each time)."""
        ids = set(json.load(open(LABEL_SKIPPED, encoding="utf-8"))) if LABEL_SKIPPED.exists() else set()
        ids.add(vid)
        json.dump(sorted(ids), open(LABEL_SKIPPED, "w", encoding="utf-8"), indent=1)
        return dict(id=vid, skipped=True)

    def label_queue(self):
        """Recordings to label areas in: uploads first (other players' layouts), then the most recent recording of each
        scenario, leaving out those with saved areas and probes (the view hardly moves in them)."""
        out, seen = [], set()
        skipped = set(json.load(open(LABEL_SKIPPED, encoding="utf-8"))) if LABEL_SKIPPED.exists() else set()
        for v in sorted(self.list(), key=lambda v: (not v.get("uploaded"), -v["mtime"])):
            sc = v["id"].split("/")[0] if not v.get("uploaded") else v["id"]
            if sc in seen or "Probe" in sc or v["not_aim"] or v["id"] in skipped or                     (self.cache_dir(v["id"]) / "exclude.json").exists():
                continue
            seen.add(sc)
            out.append(v["id"])
        return out

    def exclude(self, vid):
        """The areas a review ignores, as shares of the frame [x0, y0, x1, y1]: the ones saved for this recording, else
        for an upload the ones last saved for an upload (an uploader's setup tends to stay the same), else KovOBS's
        layout. source says which."""
        p = self.cache_dir(vid) / "exclude.json"
        if p.exists():
            out = dict(boxes=json.load(open(p)), source="saved")
        elif vid.startswith("uploads/") and EXCLUDE_UPLOADS.exists():
            out = dict(boxes=json.load(open(EXCLUDE_UPLOADS)), source="last upload")
        else:
            out = dict(boxes=review.OVERLAY_SHARES, source="kovobs")
        out["boxes"] = self.with_ids(out["boxes"])
        return out

    def with_ids(self, boxes):
        """Areas with their type as an id ([x0, y0, x1, y1, id])."""
        kinds = self.kinds()
        return [list(b[:4]) + [self.kind_id(b[4] if len(b) > 4 else "other", kinds)] for b in boxes]

    def faint(self, vid):
        """The recording's faint-target cut-off (the review app's setting): {on, offset}. Off by default."""
        p = self.cache_dir(vid) / "faint.json"
        return json.load(open(p)) if p.exists() else dict(on=False, offset=0.3)

    def set_faint(self, vid, body, submitted=None):
        on, offset = bool(body.get("on")), float(body.get("offset", 0.3))
        if not 0.2 <= offset <= 0.6:
            raise ValueError("offset must be between 0.2 and 0.6")
        out = self.cache_dir(vid)
        out.mkdir(parents=True, exist_ok=True)
        old = self.faint(vid)
        new = dict(on=on, offset=round(offset, 2))
        if submitted or old.get("submitted"):          # a later tweak keeps the record of the last submit
            new.update(submitted=submitted or old["submitted"], labels=old.get("labels") if not submitted else None)
        json.dump(new, open(out / "faint.json", "w"))
        rp = self.shown(vid)[1] / "report.json"         # a tracking run's measures use the cut: measured again
        if not submitted and rp.exists() and json.load(open(rp)).get("mode") == "track" and \
                (on != bool(old.get("on")) or (on and new["offset"] != old.get("offset"))):
            self.analyse(vid, again="measures")
        return self.faint(vid)

    def submit_faint(self, vid, offset):
        """The user's cut-off for this recording, submitted: saved (on), and its tracks written as detector labels in
        the background (model/hand_crops.cutoff_crops: frames inside the run, crops clear of the exclude areas)."""
        d, r = self.cache_dir(vid), self.shown(vid)[1]
        if not (r / "tracks.json").exists() or not (r / "report.json").exists():
            raise ValueError("review the recording first")
        out = self.set_faint(vid, dict(on=True, offset=offset), submitted=datetime.now().isoformat(timespec="seconds"))
        video, boxes = str(self.resolve(vid)), [b[:4] for b in self.exclude(vid)["boxes"]]

        def run():
            sys.path.insert(0, str(HERE / "model"))
            import hand_crops
            tracks, rep = json.load(open(r / "tracks.json")), json.load(open(r / "report.json"))
            fl, sm = rep.get("flicks") or [], rep.get("summary") or {}
            if rep.get("mode") == "track":             # the run's window; the bot sits under the crosshair
                span, near = (sm.get("start"), sm.get("end")), 0.0
            else:
                span, near = ((min(m["start_frame"] for m in fl), max(m["kill_frame"] for m in fl)) if fl
                              else (None, None)), 2.0
            n = hand_crops.cutoff_crops(video, tracks["frames"], tracks["fps"], span[0], span[1], boxes, float(offset),
                                        CUTOFF_LABELS, near=near) if span[0] is not None else 0
            f = self.faint(vid)
            f["labels"] = n
            json.dump(f, open(d / "faint.json", "w"))
        threading.Thread(target=run, daemon=True).start()
        return out

    def skip_faint(self, vid):
        ids = set(json.load(open(FAINT_SKIPPED, encoding="utf-8"))) if FAINT_SKIPPED.exists() else set()
        ids.add(vid)
        json.dump(sorted(ids), open(FAINT_SKIPPED, "w", encoding="utf-8"), indent=1)
        return dict(id=vid, skipped=True)

    def faint_queue(self):
        """Recordings to set a cut-off in, in the area queue's order (uploads first, then the most recent recording of
        each scenario), leaving out probes, other games, skipped and submitted ones."""
        skipped = set(json.load(open(FAINT_SKIPPED, encoding="utf-8"))) if FAINT_SKIPPED.exists() else set()
        out, seen = [], set()
        for v in sorted(self.list(), key=lambda v: (not v.get("uploaded"), -v["mtime"])):
            sc = v["id"].split("/")[0] if not v.get("uploaded") else v["id"]
            if sc in seen or "Probe" in sc or v["not_aim"] or v["id"] in skipped or \
                    self.faint(v["id"]).get("submitted"):
                continue
            seen.add(sc)
            out.append(v["id"])
        return out

    def set_exclude(self, vid, boxes):
        self.resolve(vid)                                   # a known recording
        if not isinstance(boxes, list) or not all(
                isinstance(b, list) and len(b) in (4, 5) and all(isinstance(v, (int, float)) for v in b[:4])
                and 0 <= b[0] < b[2] <= 1 and 0 <= b[1] < b[3] <= 1
                and (len(b) == 4 or isinstance(b[4], str)) for b in boxes):
            raise ValueError("boxes: a list of [x0, y0, x1, y1, type id] (shares of the frame)")
        boxes = self.with_ids(boxes)
        out = self.cache_dir(vid)
        out.mkdir(parents=True, exist_ok=True)
        json.dump(boxes, open(out / "exclude.json", "w"))
        if vid.startswith("uploads/"):
            json.dump(boxes, open(EXCLUDE_UPLOADS, "w"))

        def learn():                                    # the area finder learns from what was saved (in the
            try:                                        # background: finding the areas takes a few seconds)
                video = str(self.resolve(vid))
                found = areas.find(video, out, AREA_EXAMPLES)[1]   # the found areas (cached)
                areas.learn(AREA_EXAMPLES, vid, found, boxes, areas.maps(video, out))
            except Exception:
                traceback.print_exc()
        threading.Thread(target=learn, daemon=True).start()
        return self.exclude(vid)

    def labelled(self, but=None):
        """The recordings the user saved areas for: (id, its found areas, its saved areas), from the cache."""
        out = []
        other = {self.cache_dir(v) for v in self.not_aim()}
        for d in CACHE.iterdir():
            f, s = d / "areas.json", d / "exclude.json"
            if d.is_dir() and d != self.cache_dir(but or "") and d not in other and f.exists() and s.exists():
                out.append((d.name, json.load(open(f)), json.load(open(s))))
        return out

    def find_areas(self, vid, copy=True):
        """The areas to propose (areas.py): the user's own areas from a recording with the same layout (unless copy is
        off: "Detect fresh"), else the areas found in this one, named by what was learned from saved areas or by
        rules."""
        areas.find.by = {}
        boxes, found, copied = areas.find(str(self.resolve(vid)), self.cache_dir(vid), AREA_EXAMPLES,
                                          self.labelled(vid) if copy else ())
        boxes = self.with_ids(boxes)
        ex = areas.load_examples(AREA_EXAMPLES)
        mine = {e["rec"] for e in ex if not e["rec"].startswith("kovobs:")}
        return dict(boxes=boxes, examples=len(ex), recordings=len(mine), copied=copied, by=areas.find.by)

    def run(self, vid):
        """The user's run window for this recording: {start, end, length} in seconds, any of them None."""
        p = self.cache_dir(vid) / "run.json"
        return json.load(open(p)) if p.exists() else dict(start=None, end=None, length=None)

    def set_run(self, vid, body):
        """Saves the run window (all None: back to automatic) and measures the run again with it, on its tracks."""
        clean = {}
        for k in ("start", "end", "length"):
            v = body.get(k)
            clean[k] = None if v in (None, "") else float(v)
            if clean[k] is not None and not 0 <= clean[k] < 36000:
                raise ValueError(f"{k} out of range")
        if clean["start"] is not None and clean["end"] is not None and clean["end"] <= clean["start"]:
            raise ValueError("the end must come after the start")
        out = self.cache_dir(vid)
        out.mkdir(parents=True, exist_ok=True)
        if all(v is None for v in clean.values()):
            (out / "run.json").unlink(missing_ok=True)
        else:
            json.dump(clean, open(out / "run.json", "w"))
        return self.analyse(vid, again="measures")

    def analyse(self, vid, again=False):
        """Reviews a recording. again=True: a new review with the chosen model (any other model's review stays).
        "measures": the shown review measured again on its own tracks. Else the shown review, or a new one by the
        chosen model when there is none."""
        with self.lock:
            job = self.jobs.get(vid)
            if job and job["stage"] not in ("done", "error"):
                if again == "measures":                   # new marks while it runs: measured again once it ends
                    job["again_after"] = True
                return job
            video = self.resolve(vid)
            stats = self.stats_of(video)                   # None: the session HUD, else the video alone
            model, out = self.shown(vid)
            if again is True:
                model, out = self.model, self.cache_dir(vid) / "models" / self.model
            if again and out.exists():                     # "measures": the tracks stay, only the measures again
                for f in ("report.json",) if again == "measures" else ("tracks.json", "report.json", "camera.json"):
                    (out / f).unlink(missing_ok=True)
            out.mkdir(parents=True, exist_ok=True)
            detector = self.detector
            job = self.jobs[vid] = dict(stage="starting", done=0, total=1, started=time.time(), model=model)

        def progress(stage, done, total):
            job.update(stage=stage, done=done, total=total)

        def run():
            try:
                while True:
                    review.review(str(video), str(stats) if stats else None, out, progress, detector=detector,
                                  exclude=self.exclude(vid)["boxes"],
                                  run=self.run(vid) if (out / "run.json").exists() else None, faint=self.faint(vid))
                    if not job.pop("again_after", False):
                        break
                    (out / "report.json").unlink(missing_ok=True)
                job.update(stage="done", done=1, total=1, seconds=round(time.time() - job["started"], 1))
            except Exception as e:                     # shown on the page
                traceback.print_exc()
                job.update(stage="error", error=str(e))

        threading.Thread(target=run, daemon=True).start()
        return job


def _slug(name):
    return re.sub(r"[^a-z0-9]+", "_", str(name).lower()).strip("_") or "type"


def _new_id(name, kinds):
    base, have, n = _slug(name), {k["id"] for k in kinds}, 2
    out = base
    while out in have:
        out, n = f"{base}_{n}", n + 1
    return out


class Handler(BaseHTTPRequestHandler):
    lib: Library = None

    def log_message(self, fmt, *args):            # quiet: only errors reach the console
        pass

    def send_json(self, obj, code=200):
        body = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        u = urlparse(self.path)
        q = {k: v[0] for k, v in parse_qs(u.query).items()}
        try:
            if u.path == "/api/info":
                return self.send_json(dict(detector="hand-written" if self.lib.model == "hand" else self.lib.model,
                                           device=getattr(self.lib.detector, "dev", "cpu")))
            if u.path == "/api/models":
                return self.send_json(self.lib.models())
            if u.path == "/api/vods":
                return self.send_json(self.lib.list())
            if u.path == "/api/job":
                return self.send_json(self.lib.jobs.get(q["id"], dict(stage="none")))
            if u.path == "/api/report":                     # review_model: the model that made it (None: not recorded)
                model, d = self.lib.shown(q["id"])
                p = d / "report.json"
                return self.send_json(dict(json.load(open(p)), review_model=model) if p.exists() else None)
            if u.path == "/api/exclude":                    # ?layout=kovobs: KovOBS's layout, for the page's button
                return self.send_json(dict(kinds=self.lib.kinds(), **(
                    dict(boxes=review.OVERLAY_SHARES, source="kovobs") if q.get("layout") == "kovobs"
                    else self.lib.exclude(q["id"]))))
            if u.path == "/api/label_queue":
                return self.send_json(self.lib.label_queue())
            if u.path == "/api/find_areas":
                return self.send_json(self.lib.find_areas(q["id"], q.get("copy", "1") == "1"))
            if u.path == "/api/faint":
                return self.send_json(self.lib.faint(q["id"]))
            if u.path == "/api/run":
                return self.send_json(self.lib.run(q["id"]))
            if u.path == "/api/faint_queue":
                return self.send_json(self.lib.faint_queue())
            if u.path == "/api/tracks":                     # every target per frame, for the fastest-order overlay
                p = self.lib.shown(q["id"])[1] / "tracks.json"
                return self.send_json(json.load(open(p)) if p.exists() else None)
            if u.path == "/video":
                return self.send_file(self.lib.resolve(q["id"]), video=True)
            name = "index.html" if u.path in ("/", "") else u.path.lstrip("/")
            p = (APP / name).resolve()
            if APP.resolve() not in p.parents or not p.is_file():
                return self.send_json(dict(error="not found"), 404)
            return self.send_file(p)
        except FileNotFoundError as e:
            return self.send_json(dict(error=str(e)), 404)
        except (ConnectionError, BrokenPipeError):
            pass

    def do_POST(self):
        u = urlparse(self.path)
        q = {k: v[0] for k, v in parse_qs(u.query).items()}
        try:
            if u.path == "/api/analyse":
                return self.send_json(self.lib.analyse(q["id"], q.get("again") == "1"))
            if u.path == "/api/upload":
                return self.send_json(self.upload(q))
            if u.path == "/api/exclude":
                body = json.loads(self.rfile.read(int(self.headers["Content-Length"])) or b"null")
                return self.send_json(self.lib.set_exclude(q["id"], body))
            if u.path == "/api/faint":                      # {on, offset}: the faint-target cut-off for this recording
                body = json.loads(self.rfile.read(int(self.headers["Content-Length"])) or b"{}")
                return self.send_json(self.lib.set_faint(q["id"], body))
            if u.path == "/api/run":                        # {start, end, length}: the run window, then measured again
                body = json.loads(self.rfile.read(int(self.headers["Content-Length"])) or b"{}")
                return self.send_json(self.lib.set_run(q["id"], body))
            if u.path == "/api/faint_submit":               # ?offset=: saved, and written as detector labels
                return self.send_json(self.lib.submit_faint(q["id"], float(q.get("offset", 0.3))))
            if u.path == "/api/faint_skip":
                return self.send_json(self.lib.skip_faint(q["id"]))
            if u.path == "/api/not_aim":
                return self.send_json(self.lib.set_not_aim(q["id"], q.get("on", "1") == "1"))
            if u.path == "/api/area_kinds":                 # {name, about}: a new type; with id: a type's new wording
                body = json.loads(self.rfile.read(int(self.headers["Content-Length"])) or b"{}")
                return self.send_json(self.lib.save_kind(body.get("id"), body.get("name"), body.get("about")))
            if u.path == "/api/label_skip":
                return self.send_json(self.lib.skip_label(q["id"]))
            if u.path == "/api/model":                      # ?name=: the model reviews use from now on
                return self.send_json(self.lib.pick(q.get("name", "")))
            return self.send_json(dict(error="not found"), 404)
        except FileNotFoundError as e:
            return self.send_json(dict(error=str(e)), 404)
        except ValueError as e:
            return self.send_json(dict(error=str(e)), 400)

    def upload(self, q):
        """A VOD (or its stats CSV, with for=<the VOD's file name>) streamed to UPLOADS. Returns its id."""
        name = Path(q.get("name", "")).name
        ext = Path(name).suffix.lower()
        if ext == ".csv":
            target = Path(q.get("for", "")).name
            if Path(target).suffix.lower() not in VIDEO_TYPES:
                raise ValueError("a stats file needs for=<the VOD's file name>")
            dest = UPLOADS / (Path(target).stem + ".csv")
        elif ext in VIDEO_TYPES:
            dest = UPLOADS / name
        else:
            raise ValueError(f"not a video ({', '.join(VIDEO_TYPES)}) or a stats .csv: {name}")
        UPLOADS.mkdir(parents=True, exist_ok=True)
        left = int(self.headers["Content-Length"])
        part = dest.with_name(dest.name + ".part")
        with open(part, "wb") as f:
            while left > 0:
                chunk = self.rfile.read(min(left, 1 << 20))
                if not chunk:
                    raise ValueError("the upload stopped early")
                f.write(chunk)
                left -= len(chunk)
        part.replace(dest)
        return dict(id=f"uploads/{dest.name}" if ext != ".csv" else f"uploads/{Path(q['for']).name}", saved=dest.name)

    def send_file(self, p, video=False):
        size = p.stat().st_size
        start, end = 0, size - 1
        rng = self.headers.get("Range")
        m = re.match(r"bytes=(\d*)-(\d*)", rng or "")
        if m:
            if m[1]:
                start = int(m[1])
                end = int(m[2]) if m[2] else size - 1
            elif m[2]:
                start = size - int(m[2])
            end = min(end, size - 1)
        self.send_response(206 if m else 200)
        self.send_header("Content-Type", "video/mp4" if video else (mimetypes.guess_type(p.name)[0] or "application/octet-stream"))
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Content-Length", str(end - start + 1))
        if m:
            self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
        if not video:
            self.send_header("Cache-Control", "no-cache")
        self.end_headers()
        with open(p, "rb") as f:
            f.seek(start)
            left = end - start + 1
            while left > 0:
                chunk = f.read(min(1 << 20, left))
                if not chunk:
                    break
                self.wfile.write(chunk)
                left -= len(chunk)


def infer_module():
    if str(HERE / "model") not in sys.path:
        sys.path.insert(0, str(HERE / "model"))
    import infer
    return infer


@functools.cache
def has_cuda():
    try:
        import torch
        return torch.cuda.is_available()
    except ImportError:
        return False


def load_detector(model):
    """A model by name (exports/detector_<name>): PyTorch on the GPU when there is one, else ONNX Runtime on the CPU.
    "hand": None, the hand-written detector in review.py. A .pt or .onnx path: that file."""
    if model == "hand":
        return None
    infer = infer_module()
    if model.endswith((".pt", ".onnx")):
        return infer.TorchDetector(model) if model.endswith(".pt") else infer.OnnxDetector(model)
    if has_cuda() and (EXPORTS / f"detector_{model}.pt").exists():
        d = infer.TorchDetector(str(EXPORTS / f"detector_{model}.pt"))
        d.name = model
        return d
    # no GPU: ONNX Runtime on the CPU, with the export that takes the frame's bytes as they are (same detections as
    # the fp32 file, 30 s against 47 s of tracking for a 66-second 120 fps VOD on an 8-core Ryzen 7 9800X3D)
    for name in (f"detector_{model}_u8in.onnx", f"detector_{model}_fp32.onnx"):
        if (EXPORTS / name).exists():
            d = infer.OnnxDetector(str(EXPORTS / name))
            d.name, d.dev = model, "cpu"
            return d
    raise FileNotFoundError(f"no exported model called {model}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--vods", default=r"E:\OBS\KovOBS")
    ap.add_argument("--stats", default=STATS_DEFAULT)
    ap.add_argument("--port", type=int, default=8770)
    ap.add_argument("--detector", default="auto",
                    help="auto (the model picked in the app, else infer.BEST, else hand-written), a model name, hand, "
                         "or a .pt/.onnx path")
    a = ap.parse_args()
    lib = Library(a.vods, a.stats)
    picked = json.load(open(SETTINGS)).get("model") if SETTINGS.exists() else None
    for model in [a.detector] if a.detector != "auto" else [m for m in (picked, infer_module().BEST) if m] + ["hand"]:
        try:
            lib.use(model)
            break
        except FileNotFoundError:
            print(f"model {model}: no file it can run from here")
    lib.load_stats_index()
    Handler.lib = lib
    print(f"VOD review: http://127.0.0.1:{a.port}/  (VODs in {a.vods}; {sum(len(v) for v in lib.index.values())} stats "
          f"files; model: {lib.model} on the {'GPU' if getattr(lib.detector, 'dev', 'cpu') == 'cuda' else 'CPU'})")
    sys.stdout.flush()
    ThreadingHTTPServer(("127.0.0.1", a.port), Handler).serve_forever()


if __name__ == "__main__":
    main()
