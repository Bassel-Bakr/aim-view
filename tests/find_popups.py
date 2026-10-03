"""Which recordings have an excluded area that review.AreaWatch finds to be a pop-up (shown only some of the time)?
Runs Python's AreaWatch over every frame of each recording with saved exclude areas (test_out/vod_app/*/exclude.json),
and prints the areas it marks as pop-ups and in how many frames each is excluded. Picks a fixture for the Rust
port's parity test (tests/fixtures.py). Usage: python tests/find_popups.py"""
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "python"))

import review  # noqa: E402
import aimview_tools  # noqa: E402


def main():
    lib = aimview_tools.Library(r"E:\OBS\KovOBS", aimview_tools.STATS_DEFAULT)
    by_dir = {lib.cache_dir(v["id"]).name: v["id"] for v in lib.list()}
    for p in sorted((ROOT / "test_out" / "vod_app").glob("*/exclude.json")):
        vid = by_dir.get(p.parent.name)
        if not vid:
            continue
        boxes = json.load(open(p))
        watch = review.AreaWatch(boxes)
        n = 0
        for rgb in review.rgb_frames(str(lib.resolve(vid))):
            watch.add(rgb)
            n += 1
        shows = watch.showing()
        pops = [(i, int(s.sum())) for i, s in enumerate(shows) if s is not None]
        print(json.dumps(dict(id=vid, frames=n, areas=len(boxes), popups=pops)), flush=True)


if __name__ == "__main__":
    main()
