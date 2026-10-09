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
import old_review  # noqa: E402
import recording_names  # noqa: E402

EDGE_S = 3.0            # seconds left out at each end of a VOD
MIN_SPAN_END_S = 3.5    # the last sample is at least this far in
BATCH = 16              # frames a detector call


def frames_at(video, count):
    """`count` frames spread over the video (RGB, the review's 1280 x 720 scale)."""
    _, duration = old_review.probe(str(video))
    out = []
    for at in np.linspace(EDGE_S, max(MIN_SPAN_END_S, duration - EDGE_S), count):
        decoded = subprocess.run(["ffmpeg", "-v", "error", "-ss", f"{at:.3f}", "-i", str(video), "-frames:v", "1",
                                  "-vf", f"scale={old_review.W}:{old_review.H}:flags=area,format=rgb24", "-f",
                                  "rawvideo", "-"], capture_output=True)
        if len(decoded.stdout) == old_review.W * old_review.H * 3:
            out.append(np.frombuffer(decoded.stdout, np.uint8).reshape(old_review.H, old_review.W, 3))
    return np.stack(out)


def in_play_area(box):
    """Whether a detection's center is where the review reads targets (old_review.MASK)."""
    return old_review.MASK[min(old_review.H - 1, int(box[1])), min(old_review.W - 1, int(box[0]))]


def column_name(model):
    """A model's name for its column: its folder's name for a run's best.pt, else the file's name without .pt."""
    return Path(model).parent.name if Path(model).name == "best.pt" else Path(model).stem


def main():
    """Prints a row per VOD: its scenario's target count, then per model the share of sampled frames with more
    detections in the play area than that, and the mean number of extra detections a frame (0 on a frame within the
    count). A VOD whose scenario has no target count gets a line that says so."""
    parser = argparse.ArgumentParser()
    parser.add_argument("models", nargs="+")
    parser.add_argument("--vods", nargs="+", required=True)
    parser.add_argument("--frames", type=int, default=40)
    args = parser.parse_args()
    counts = build_data.target_counts()
    detectors = [infer.TorchDetector(model) for model in args.models]
    print(f"{'VOD':48s} {'alive':>5s}  " + "  ".join(f"{column_name(model):>22s}" for model in args.models))
    for video in args.vods:
        scenario = recording_names.scenario_name(video)
        alive = counts.get(scenario.lower())
        if not alive:
            print(f"{scenario[:48]:48s} no target count")
            continue
        frames = frames_at(video, args.frames)
        fixed = old_review.fixed_map(list(old_review._frames(video, keyframes=True))).astype(np.uint8)
        cells = []
        for detector in detectors:
            found = []
            for i in range(0, len(frames), BATCH):
                for boxes in detector.batch(frames[i:i + BATCH].copy(), fixed):
                    found.append(sum(1 for box in boxes if in_play_area(box)))
            found = np.array(found)
            cells.append(f"{np.mean(found > alive):5.0%} over, {np.mean(np.maximum(0, found - alive)):5.2f} extra")
        print(f"{Path(video).stem[:48]:48s} {alive:5d}  " + "  ".join(f"{cell:>22s}" for cell in cells), flush=True)


if __name__ == "__main__":
    main()
