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
import local_config  # noqa: E402

STATS = local_config.required(local_config.kovaak("stats"), "KovaaK's stats folder (Steam's)")
VODS = local_config.required(local_config.folder("vods"), "recordings' folder (vods)")
DEFAULT = [str(VODS / video) for video in (
    "1w4ts Voltaic/1w4ts Voltaic - 143 - 2026.09.30-04.55.23.mp4",
    "10 Sphere Hipfire Extra Small/10 Sphere Hipfire Extra Small - 1550 - 2026.08.26-04.40.26.mp4",
    "Pokeball 5 Sphere Hipfire Extra Small LG56 AIMGOD/Pokeball 5 Sphere Hipfire Extra Small LG56 AIMGOD - 114 - 2026.10.01-06.07.58.mp4",
    "Pokeball 1 Sphere Hipfire Extra Small LG56 AIMGO/Pokeball 1 Sphere Hipfire Extra Small LG56 AIMGO - 84 - 2026.10.01-06.22.30.mp4",
)]
FOLDER_CHARS = 60               # a review's folder keeps this much of the video's name
VIDEO_CHARS, ERROR_CHARS = 46, 60   # the printed line's columns


def stats_for(video):
    scenario, _, stamp = Path(video).stem.rsplit(" - ", 2)
    path = Path(STATS) / f"{scenario} - Challenge - {stamp} Stats.csv"
    return path if path.exists() else None


def u8in(model):
    """The model's _u8in export, which the app's native review runs: the file itself, or the export of the same model
    beside it (exports/detector_<name>.pt or _fp32.onnx: exports/detector_<name>_u8in.onnx)."""
    path = Path(model)
    name = re.match(r"(detector_.+?)(_u8in|_fp32|_fp16|_int8|_embed)?\.(pt|onnx)$", path.name)
    if name and (path.parent / f"{name[1]}_u8in.onnx").is_file():
        return path.parent / f"{name[1]}_u8in.onnx"
    sys.exit(f"{model}: the native review runs the model's _u8in export (python/model/export.py --u8in)")


def native_review(lib, video, model, out, stats):
    """The app's review of a video (aimview_tools: Library.review_video) with the model, and its report. The scenario's
    facts come from its file, as the core reads it (Library.scenario_facts, target_counts)."""
    scenario = Path(video).stem.rsplit(" - ", 2)[0].lower()
    kind, limit = lib.scenario_facts().get(scenario, (None, None))
    return lib.review_video(video, str(model), str(out), stats=stats, kind=kind, limit=limit,
                            cap=lib.target_counts().get(scenario))["report"]


def library():
    """The review service's library on this computer's data (the repo's test_out/)."""
    import aimview_tools
    return aimview_tools.Library()


def review_row(lib, video, model, stats):
    """One VOD's row: its kills matched, flicks measured and headline numbers, or the error that stopped it."""
    row = dict(video=Path(video).name)
    out = Path("test_out/vod_model/eval/vods") / "native" / Path(video).stem[:FOLDER_CHARS]
    for name in ("tracks.json", "report.json"):
        (out / name).unlink(missing_ok=True)
    started = time.time()
    try:
        report = native_review(lib, video, model, out, str(stats))
        summary, info = report["summary"], report["summary"]["info"]
        row["model"] = dict(seconds=round(time.time() - started, 1), kills_stats=info["kills_stats"],
                            matched=info["matched"], confirmed=info.get("confirmed"),
                            measured=summary["measured"], median_kill=summary["median_interval"],
                            still=summary.get("still"), hold=summary.get("hold"), mode=summary.get("mode"),
                            radius=round(summary["radius"], 3))
    except Exception as error:                    # report the failure, keep going
        row["model"] = dict(error=f"{type(error).__name__}: {error}")
    return row


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("model")
    parser.add_argument("--vods", nargs="*", default=DEFAULT)
    parser.add_argument("--out", default="test_out/vod_model/eval")
    args = parser.parse_args()
    model, lib = u8in(args.model), library()
    rows = []
    for video in args.vods:
        stats = stats_for(video)
        if not stats:
            print("no stats file for", video)
            continue
        row = review_row(lib, video, model, stats)
        rows.append(row)
        numbers = row["model"]
        result = (f"{numbers['matched']}/{numbers['kills_stats']} kills ({numbers['confirmed']} confirmed), "
                  f"{numbers['measured']} flicks, {numbers['seconds']} s" if "matched" in numbers
                  else numbers["error"][:ERROR_CHARS])
        print(f"{row['video'][:VIDEO_CHARS]:46s} model: {result}", flush=True)
    name = Path(args.model).stem if args.model.endswith(".onnx") else Path(args.model).parent.name
    Path(args.out).mkdir(parents=True, exist_ok=True)
    json.dump(rows, open(Path(args.out) / f"vods_{name}_native.json", "w"), indent=1)


if __name__ == "__main__":
    main()
