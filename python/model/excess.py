"""False detections without labels: a static scenario's file says how many targets are alive at once (AddedBots), so on
any frame, detections beyond that count are false (seams, decorations, effects). Samples frames across each VOD and
reports, per model, the share of frames with too many detections and the excess per frame.
Usage: python python/model/excess.py <model.pt> [<model.pt> ...] --vods <mp4> [<mp4> ...] [--frames 40]
"""
import argparse
import subprocess
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
import build_data  # noqa: E402
import infer  # noqa: E402
import review  # noqa: E402


def frames_at(video, n):
    fps, dur = review.probe(str(video))
    out = []
    for t in np.linspace(3, max(3.5, dur - 3), n):
        r = subprocess.run(["ffmpeg", "-v", "error", "-ss", f"{t:.3f}", "-i", str(video), "-frames:v", "1", "-vf",
                            f"scale={review.W}:{review.H}:flags=area,format=rgb24", "-f", "rawvideo", "-"], capture_output=True)
        if len(r.stdout) == review.W * review.H * 3:
            out.append(np.frombuffer(r.stdout, np.uint8).reshape(review.H, review.W, 3))
    return np.stack(out)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("models", nargs="+")
    ap.add_argument("--vods", nargs="+", required=True)
    ap.add_argument("--frames", type=int, default=40)
    a = ap.parse_args()
    counts = build_data.target_counts()
    dets = [infer.TorchDetector(m) for m in a.models]
    print(f"{'VOD':48s} {'alive':>5s}  " + "  ".join(f"{Path(m).parent.name if Path(m).name == 'best.pt' else Path(m).stem:>22s}" for m in a.models))
    for v in a.vods:
        scen = Path(v).stem.rsplit(" - ", 2)[0]
        k = counts.get(scen.lower())
        if not k:
            print(f"{scen[:48]:48s} no target count")
            continue
        fr = frames_at(v, a.frames)
        fixed = review.fixed_map(list(review._frames(v, keyframes=True))).astype(np.uint8)
        cells = []
        for d in dets:
            n = []
            for i in range(0, len(fr), 16):
                for b in d.batch(fr[i:i + 16].copy(), fixed):
                    n.append(sum(1 for x in b if review.MASK[min(review.H - 1, int(x[1])), min(review.W - 1, int(x[0]))]))
            n = np.array(n)
            cells.append(f"{np.mean(n > k):5.0%} over, {np.mean(np.maximum(0, n - k)):5.2f} extra")
        print(f"{Path(v).stem[:48]:48s} {k:5d}  " + "  ".join(f"{c:>22s}" for c in cells), flush=True)


if __name__ == "__main__":
    main()
