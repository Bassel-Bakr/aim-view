"""End-to-end evaluation on whole held-out KovOBS recordings (REPRODUCE.md step 3b).

Reviews each VOD with the model (the app's native review: the review service's aimview-tool, through
python/aimview_tools.py) and scores it against the run's stats file, which is independent ground truth for when each
kill happened:
  - kills matched: stats kills found as a tracked target ending at the crosshair within 2.5 frames;
  - flicks measured: kills whose target was tracked from the previous kill;
  - the review's headline numbers.
None of these VODs' scenarios is in the training or validation split.
Writes test_out/vod_model/eval/vods_<model>_native.json.
Usage: python python/model/eval_vods.py <model> [--vods <mp4> ...]
The native review runs the model's _u8in export: the file itself, or the one beside an exports/detector_<name>.pt or
.onnx file (python/model/export.py --u8in). (The old Python review and its hand-written detector column retired with
python/review.py, now python/retired/review.py.)
"""
import argparse
import json
import re
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import old_review  # noqa: E402

STATS = r"C:\Program Files (x86)\Steam\steamapps\common\FPSAimTrainer\FPSAimTrainer\stats"
DEFAULT = [
    r"E:\OBS\KovOBS\1w4ts Voltaic\1w4ts Voltaic - 143 - 2026.09.30-04.55.23.mp4",
    r"E:\OBS\KovOBS\10 Sphere Hipfire Extra Small\10 Sphere Hipfire Extra Small - 1550 - 2026.08.26-04.40.26.mp4",
    r"E:\OBS\KovOBS\Pokeball 5 Sphere Hipfire Extra Small LG56 AIMGOD\Pokeball 5 Sphere Hipfire Extra Small LG56 AIMGOD - 114 - 2026.10.01-06.07.58.mp4",
    r"E:\OBS\KovOBS\Pokeball 1 Sphere Hipfire Extra Small LG56 AIMGO\Pokeball 1 Sphere Hipfire Extra Small LG56 AIMGO - 84 - 2026.10.01-06.22.30.mp4",
]


def stats_for(video):
    scen, score, stamp = Path(video).stem.rsplit(" - ", 2)
    p = Path(STATS) / f"{scen} - Challenge - {stamp} Stats.csv"
    return p if p.exists() else None


def u8in(model):
    """The model's _u8in export, which the app's native review runs: the file itself, or the export of the same model
    beside it (exports/detector_<name>.pt or _fp32.onnx: exports/detector_<name>_u8in.onnx)."""
    p = Path(model)
    m = re.match(r"(detector_.+?)(_u8in|_fp32|_fp16|_int8|_embed)?\.(pt|onnx)$", p.name)
    if m and (p.parent / f"{m[1]}_u8in.onnx").is_file():
        return p.parent / f"{m[1]}_u8in.onnx"
    sys.exit(f"{model}: the native review runs the model's _u8in export (python/model/export.py --u8in)")


def native_review(lib, video, model, out, stats):
    """The app's review of a video (aimview_tools: Library.review_video) with the model, and its report. The scenario's
    facts come from its file (old_review.scenario_facts, target_counts)."""
    scenario = Path(video).stem.rsplit(" - ", 2)[0].lower()
    kind, limit = old_review.scenario_facts().get(scenario, (None, None))
    return lib.review_video(video, str(model), str(out), stats=stats, kind=kind, limit=limit,
                            cap=old_review.target_counts().get(scenario))["report"]


def library():
    """The review service's library on this computer's data (the repo's test_out/)."""
    import aimview_tools
    return aimview_tools.Library()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("model")
    ap.add_argument("--vods", nargs="*", default=DEFAULT)
    ap.add_argument("--out", default="test_out/vod_model/eval")
    a = ap.parse_args()
    det, lib = u8in(a.model), library()
    rows = []
    for v in a.vods:
        st = stats_for(v)
        if not st:
            print("no stats file for", v)
            continue
        row = dict(video=Path(v).name)
        out = Path("test_out/vod_model/eval/vods") / "native" / Path(v).stem[:60]
        for f in ("tracks.json", "report.json"):
            (out / f).unlink(missing_ok=True)
        t = time.time()
        try:
            r = native_review(lib, v, det, out, str(st))
            s, i = r["summary"], r["summary"]["info"]
            row["model"] = dict(seconds=round(time.time() - t, 1), kills_stats=i["kills_stats"], matched=i["matched"],
                                confirmed=i.get("confirmed"),
                                measured=s["measured"], median_kill=s["median_interval"], still=s.get("still"),
                                hold=s.get("hold"), mode=s.get("mode"), radius=round(s["radius"], 3))
        except Exception as e:                    # report the failure, keep going
            row["model"] = dict(error=f"{type(e).__name__}: {e}")
        rows.append(row)
        m = row["model"]
        result = (f"{m['matched']}/{m['kills_stats']} kills ({m['confirmed']} confirmed), {m['measured']} flicks, "
                  f"{m['seconds']} s" if "matched" in m else m["error"][:60])
        print(f"{row['video'][:46]:46s} model: {result}", flush=True)
    name = Path(a.model).stem if a.model.endswith(".onnx") else Path(a.model).parent.name
    Path(a.out).mkdir(parents=True, exist_ok=True)
    json.dump(rows, open(Path(a.out) / f"vods_{name}_native.json", "w"), indent=1)


if __name__ == "__main__":
    main()
