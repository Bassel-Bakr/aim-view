"""End-to-end evaluation on whole held-out KovOBS recordings (REPRODUCE.md step 3b).

Runs the VOD review pipeline (python/review.py) on each VOD twice, with the hand-written detector and with the model, and
scores both against the run's stats file, which is independent ground truth for when each kill happened:
  - kills matched: stats kills found as a tracked target ending at the crosshair within 2.5 frames;
  - flicks measured: kills whose target was tracked from the previous kill;
  - the review's headline numbers, to see whether the two detectors tell the same story.
None of these VODs' scenarios is in the training or validation split.
Writes test_out/vod_model/eval/vods_<model>.json.
Usage: python python/model/eval_vods.py <model.pt|model.onnx> [--vods <mp4> ...]
"""
import argparse
import json
import os
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import infer  # noqa: E402
import review  # noqa: E402

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


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("model")
    ap.add_argument("--vods", nargs="*", default=DEFAULT)
    ap.add_argument("--out", default="test_out/vod_model/eval")
    a = ap.parse_args()
    det = infer.TorchDetector(a.model) if a.model.endswith(".pt") else infer.OnnxDetector(a.model)
    rows = []
    for v in a.vods:
        st = stats_for(v)
        if not st:
            print("no stats file for", v)
            continue
        row = dict(video=Path(v).name)
        for name, d in (("hand-written", None), ("model", det)):
            out = Path("test_out/vod_model/eval/vods") / name / Path(v).stem[:60]
            for f in ("tracks.json", "report.json"):
                (out / f).unlink(missing_ok=True)
            t = time.time()
            try:
                r = review.review(v, str(st), out, detector=d)
                s, i = r["summary"], r["summary"]["info"]
                row[name] = dict(seconds=round(time.time() - t, 1), kills_stats=i["kills_stats"], matched=i["matched"],
                                 confirmed=i.get("confirmed"),
                                 measured=s["measured"], median_kill=s["median_interval"], still=s.get("still"),
                                 hold=s.get("hold"), mode=s.get("mode"), radius=round(s["radius"], 3))
            except Exception as e:                    # report the failure, keep going
                row[name] = dict(error=f"{type(e).__name__}: {e}")
        rows.append(row)
        h, m = row["hand-written"], row["model"]
        fmt = lambda x: (f"{x['matched']}/{x['kills_stats']} kills ({x['confirmed']} confirmed), {x['measured']} flicks, "
                         f"{x['seconds']} s"
                         if "matched" in x else x["error"][:60])
        print(f"{row['video'][:46]:46s} hand-written: {fmt(h)} | model: {fmt(m)}", flush=True)
    name = Path(a.model).stem if a.model.endswith(".onnx") else Path(a.model).parent.name
    Path(a.out).mkdir(parents=True, exist_ok=True)
    json.dump(rows, open(Path(a.out) / f"vods_{name}.json", "w"), indent=1)


if __name__ == "__main__":
    main()
