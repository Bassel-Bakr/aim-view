"""Pick crops of a dataset for a check by eye (label_check.py --data): spread over the recordings and the scenario
kinds, three in four of them uncertain ones: a box scored near the threshold, a frame whose box count is not the
scenario's target count, a very small or very large box (under the 5th or over the 95th percentile of the dataset's
boxes), or no box at all. The picked crops are copied into --out, in their split folders, with picks.jsonl (where each
one comes from and why it was picked). The dataset is left as it is.
Usage: python python/model/pick_checks.py --data test_out/vod_model/data_moving_themes
       --out test_out/vod_model/check_moving_themes [--n 150]
"""
import argparse
import collections
import json
import random
import shutil
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
import infer  # noqa: E402
import old_review  # noqa: E402

SIDE_PERCENTILES = (5, 95)          # a box side under the first or over the second is very small or very large
# each reason's weight: the most telling first
NEAR_THRESHOLD_WEIGHT, ODD_SIZE_WEIGHT, COUNT_WEIGHT, NO_BOX_WEIGHT = 3, 2, 1, 1
UNSURE_IN_TURN = 3                  # uncertain crops before each plain one
KIND_SHARE = 6                      # each kind gets at least a sixth of the picks where it has that many crops


def read_crops(data, recordings, kinds):
    """Every crop of the dataset with its boxes' sides and scores, its frame's box count, and its recording's target
    count and kind."""
    crops = []
    for path in sorted(data.glob("*/*.npz")):
        crop = np.load(path)
        boxes = crop["boxes"]
        stem, frame, _ = path.stem.split("_")
        recording = recordings[stem]
        crops.append(dict(path=path, stem=stem, n=len(boxes), side=np.maximum(boxes[:, 2], boxes[:, 3]),
                          score=crop["scores"] if "scores" in crop.files else np.ones(len(boxes)),
                          frame=recording["counts"][int(frame)], targets=recording.get("targets"),
                          kind=kinds.get(recording["folder"].lower())))
    return crops


def add_reasons(crops, threshold, near, small, large):
    """Sets each crop's reasons to check it ("why") and their summed weight. A score under `threshold` + `near` is
    near the threshold; a box side under `small` or over `large` px is very small or very large."""
    for crop in crops:
        why = []
        if len(crop["score"]) and crop["score"].min() < threshold + near:
            why.append((NEAR_THRESHOLD_WEIGHT, f"a score near the threshold ({crop['score'].min():.2f})"))
        if (crop["side"] < small).any():
            why.append((ODD_SIZE_WEIGHT, f"a very small box ({crop['side'].min():.0f} px)"))
        if (crop["side"] > large).any():
            why.append((ODD_SIZE_WEIGHT, f"a very large box ({crop['side'].max():.0f} px)"))
        if crop["targets"] and crop["frame"] != crop["targets"]:
            why.append((COUNT_WEIGHT, f"{crop['frame']} boxes in the frame for {crop['targets']} targets"))
        if not crop["n"]:
            why.append((NO_BOX_WEIGHT, "no box"))
        crop["weight"], crop["why"] = sum(weight for weight, _ in why), [text for _, text in why]


def quotas(by_kind, wanted, crop_count):
    """Each kind's share: by its recordings, at least a sixth of the picks where it has that many crops."""
    def crops_of(kind):
        """The kind's crop count, over all its recordings."""
        return sum(map(len, by_kind[kind].values()))
    total = sum(len(recordings) for recordings in by_kind.values())
    quota = {kind: max(min(wanted // KIND_SHARE, crops_of(kind)), round(wanted * len(recordings) / total))
             for kind, recordings in by_kind.items()}
    while sum(quota.values()) > wanted:
        quota[max(quota, key=quota.get)] -= 1
    while sum(quota.values()) < min(wanted, crop_count):
        quota[max(quota, key=lambda kind: crops_of(kind) - quota[kind])] += 1
    return quota


def recording_queue(crops, rnd):
    """A recording's crops in the order they are picked: three uncertain ones, then a plain one."""
    rnd.shuffle(crops)
    unsure = sorted((crop for crop in crops if crop["why"]), key=lambda crop: -crop["weight"])
    plain = [crop for crop in crops if not crop["why"]]
    order = []
    while unsure or plain:
        for source in [unsure] * UNSURE_IN_TURN + [plain]:
            if source:
                order.append(source.pop(0))
    return order


def pick(by_kind, quota, rnd):
    """The picked crops: each kind's quota, one crop from each recording in turn."""
    picked = []
    for kind, recordings in sorted(by_kind.items(), key=lambda item: str(item[0])):
        queues = [recording_queue(recordings[stem], rnd) for stem in sorted(recordings)]
        rnd.shuffle(queues)
        got = 0
        while got < quota[kind] and any(queues):
            for queue in queues:
                if queue and got < quota[kind]:
                    picked.append(queue.pop(0))
                    got += 1
    return picked


def write_picks(picked, out, recordings):
    """The picked crops copied into their split folders of `out`, with picks.jsonl."""
    out.mkdir(parents=True, exist_ok=True)
    with open(out / "picks.jsonl", "w", encoding="utf-8") as picks:
        for crop in picked:
            split = crop["path"].parent.name
            (out / split).mkdir(exist_ok=True)
            shutil.copy2(crop["path"], out / split / crop["path"].name)
            recording = recordings[crop["stem"]]
            picks.write(json.dumps(dict(file=f"{split}/{crop['path'].name}", folder=recording["folder"],
                                        video=recording["file"], kind=crop["kind"], boxes=crop["n"],
                                        scores=[round(float(score), 3) for score in crop["score"]],
                                        frame_boxes=crop["frame"], targets=crop["targets"], why=crop["why"])) + "\n")


def reason_name(why):
    """A reason without its numbers, to count the picks by reason."""
    return why.split(" (")[0] if "boxes in" not in why else "box count is not the target count"


def main():
    """Picks the crops of the dataset's kept recordings, copies them into --out (which must be empty), and prints the
    count per kind and per reason."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--data", required=True, help="a dataset build_data.py wrote (manifest.jsonl, train, val, test)")
    parser.add_argument("--out", required=True)
    parser.add_argument("--n", type=int, default=150)
    parser.add_argument("--threshold", type=float, default=infer.THRESHOLD, help="the labelling model's threshold")
    parser.add_argument("--near", type=float, default=0.1, help="a score this close above the threshold is uncertain")
    parser.add_argument("--seed", type=int, default=0)
    args = parser.parse_args()
    data, out = Path(args.data), Path(args.out)
    if out.exists() and any(out.iterdir()):
        sys.exit(f"{out} is not empty")
    recordings = {}
    for line in open(data / "manifest.jsonl", encoding="utf-8"):
        row = json.loads(line)
        if row["kept"]:
            recordings[row["stem"]] = row
    crops = read_crops(data, recordings, old_review.scenario_kinds())
    small, large = np.percentile(np.concatenate([crop["side"] for crop in crops]), SIDE_PERCENTILES)
    add_reasons(crops, args.threshold, args.near, small, large)
    rnd = random.Random(args.seed)
    by_kind = collections.defaultdict(lambda: collections.defaultdict(list))
    for crop in crops:
        by_kind[crop["kind"]][crop["stem"]].append(crop)
    picked = pick(by_kind, quotas(by_kind, args.n, len(crops)), rnd)
    write_picks(picked, out, recordings)
    count = collections.Counter((crop["kind"], bool(crop["why"])) for crop in picked)
    print(f"{len(picked)} crops from {len({crop['stem'] for crop in picked})} recordings into {out}; box sides p5 "
          f"{small:.1f} px, p95 {large:.1f} px")
    for kind in sorted({kind for kind, _ in count}, key=str):
        print(f"  {kind}: {count[(kind, True)]} uncertain, {count[(kind, False)]} plain")
    for reason, times in collections.Counter(reason_name(why) for crop in picked for why in crop["why"]).most_common():
        print(f"  {reason}: {times}")


if __name__ == "__main__":
    main()
