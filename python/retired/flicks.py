"""Match the tracked kills with the run's stats file and cut the VOD into flicks (step 2 of python/README.md).
Reads <dir>/tracks.json, writes <dir>/flicks.json. The work is review.match.
Usage: python python/flicks.py <dir> <stats csv>"""
import json
import sys
from pathlib import Path

import review

d = Path(sys.argv[1])
flicks, info = review.match(json.load(open(d / "tracks.json")), sys.argv[2])
json.dump(flicks, open(d / "flicks.json", "w"))
print(f"{info['kills_video']} kills found in the video; video = challenge + {info['offset']:.3f} s; "
      f"{info['matched']} of {info['kills_stats']} stats kills matched")
