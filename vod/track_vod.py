"""Track the targets in a KovaaK's static clicking VOD, relative to the crosshair (step 1 of vod/README.md).
Writes <out dir>/tracks.json: per frame, each target's track id and its position in view degrees (x right, y up, from
the crosshair). The work is review.track: one ffmpeg decodes in order, and the detection runs on every CPU core.
Usage: python vod/track_vod.py <video> <out dir>"""
import json
import sys
from pathlib import Path

import review

if __name__ == "__main__":
    out = Path(sys.argv[2])
    out.mkdir(parents=True, exist_ok=True)
    tracks = review.track(sys.argv[1], lambda stage, k, n: print(f"\r{stage} {k}/{n}", end="", file=sys.stderr))
    json.dump(tracks, open(out / "tracks.json", "w"))
    print(f"\n{len(tracks['frames'])} frames, {1 + max((t[0] for f in tracks['frames'] for t in f['t']), default=-1)} tracks")
