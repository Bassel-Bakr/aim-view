"""Measure each flick of a VOD and judge it (step 3 of python/README.md). Reads <dir>/tracks.json and flicks.json, writes
<dir>/measures.json and prints the summary and the provisional issue checks. The work is review.measure, summarize and
judge; the target radius comes from the targets' size in the video unless --radius gives it in degrees.
Usage: python python/measure.py <dir> <stats csv> [--radius 0.43]"""
import json
import sys
from pathlib import Path

import review

d = Path(sys.argv[1])
tracks = json.load(open(d / "tracks.json"))
flicks = json.load(open(d / "flicks.json"))
R = float(sys.argv[sys.argv.index("--radius") + 1]) if "--radius" in sys.argv else review.target_radius(flicks)
ms = review.measure(flicks, tracks["fps"], R)
json.dump(ms, open(d / "measures.json", "w"))
meta, _ = review.load_stats(sys.argv[2])
s = review.summarize(ms, review.choices(tracks, flicks), meta, {}, R)
ms_ = lambda v: f"{1000 * v:.0f} ms" if v is not None else "-"
print(f"{len(ms)} flicks measured; target radius {R:.2f} deg")
print(f"median kill {ms_(s['median_interval'])}, reaction {ms_(s['react'])}, main flick {ms_(s['flick'])}, "
      f"on target {ms_(s['arrive'])}, still before the click {ms_(s['still'])}, peak {s['peak']:.0f} deg/s")
if s["budget"]:
    print("average kill:", ", ".join(f"{n} {ms_(v)}" for n, v in zip(
        ("react", "main flick", "onto the target", "settle", "still"), s["budget"])))
for b in s["by_distance"]:
    print(f"distance {b['lo']:2d}-{b['hi']:2d} deg: n {b['n']:3d}  kill {ms_(b['interval'])}  short {b['short']:.0%}  "
          f"past {b['past']:.0%}  still {ms_(b['still'])}")
for b in s["by_direction"]:
    print(f"toward {b['name']:10s}: n {b['n']:3d}  kill {ms_(b['interval'])}  distance {b['distance']:.1f} deg  "
          f"short {b['short']:.0%}  past {b['past']:.0%}")
for i in review.judge(s):
    print(f"#{i['issue']:<3d} {i['title']:28s} {i['flag']:9s} {i['value']}")
