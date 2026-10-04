"""Pick crops of a dataset for a check by eye (label_check.py --data): spread over the recordings and the scenario kinds,
three in four of them uncertain ones: a box scored near the threshold, a frame whose box count is not the scenario's
target count, a very small or very large box (under the 5th or over the 95th percentile of the dataset's boxes), or no
box at all. The picked crops are copied into --out, in their split folders, with picks.jsonl (where each one comes
from and why it was picked). The dataset is left as it is.
Usage: python python/model/pick_checks.py --data test_out/vod_model/data_moving_themes --out test_out/vod_model/check_moving_themes [--n 150]
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
import review  # noqa: E402


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--data", required=True, help="a dataset build_data.py wrote (manifest.jsonl, train, val, test)")
    ap.add_argument("--out", required=True)
    ap.add_argument("--n", type=int, default=150)
    ap.add_argument("--threshold", type=float, default=infer.THRESHOLD, help="the labelling model's threshold")
    ap.add_argument("--near", type=float, default=0.1, help="a score this close above the threshold is uncertain")
    ap.add_argument("--seed", type=int, default=0)
    a = ap.parse_args()
    data, out = Path(a.data), Path(a.out)
    if out.exists() and any(out.iterdir()):
        sys.exit(f"{out} is not empty")
    kinds = review.scenario_kinds()
    rec = {}
    for line in open(data / "manifest.jsonl", encoding="utf-8"):
        r = json.loads(line)
        if r["kept"]:
            rec[r["stem"]] = r
    crops = []
    for f in sorted(data.glob("*/*.npz")):
        z = np.load(f)
        b = z["boxes"]
        stem, k, _ = f.stem.split("_")
        r = rec[stem]
        crops.append(dict(path=f, stem=stem, n=len(b), side=np.maximum(b[:, 2], b[:, 3]),
                          score=z["scores"] if "scores" in z.files else np.ones(len(b)),
                          frame=r["counts"][int(k)], targets=r.get("targets"), kind=kinds.get(r["folder"].lower())))
    lo, hi = np.percentile(np.concatenate([c["side"] for c in crops]), (5, 95))
    for c in crops:                                     # each reason with its weight: the most telling first
        why = []
        if len(c["score"]) and c["score"].min() < a.threshold + a.near:
            why.append((3, f"a score near the threshold ({c['score'].min():.2f})"))
        if (c["side"] < lo).any():
            why.append((2, f"a very small box ({c['side'].min():.0f} px)"))
        if (c["side"] > hi).any():
            why.append((2, f"a very large box ({c['side'].max():.0f} px)"))
        if c["targets"] and c["frame"] != c["targets"]:
            why.append((1, f"{c['frame']} boxes in the frame for {c['targets']} targets"))
        if not c["n"]:
            why.append((1, "no box"))
        c["weight"], c["why"] = sum(w for w, _ in why), [t for _, t in why]
    rnd = random.Random(a.seed)
    by_kind = collections.defaultdict(lambda: collections.defaultdict(list))
    for c in crops:
        by_kind[c["kind"]][c["stem"]].append(c)
    # each kind's share: by its recordings, at least a sixth of the picks where it has that many crops
    total = sum(len(v) for v in by_kind.values())
    quota = {k: max(min(a.n // 6, sum(map(len, v.values()))), round(a.n * len(v) / total)) for k, v in by_kind.items()}
    while sum(quota.values()) > a.n:
        quota[max(quota, key=quota.get)] -= 1
    while sum(quota.values()) < min(a.n, len(crops)):
        quota[max(quota, key=lambda k: sum(map(len, by_kind[k].values())) - quota[k])] += 1
    picked = []
    for kind, recs in sorted(by_kind.items(), key=lambda kv: str(kv[0])):
        queues = []
        for stem in sorted(recs):
            cs = recs[stem]
            rnd.shuffle(cs)
            unsure = sorted((c for c in cs if c["why"]), key=lambda c: -c["weight"])
            plain = [c for c in cs if not c["why"]]
            order = []
            while unsure or plain:                      # three uncertain crops, then a plain one
                for src in (unsure, unsure, unsure, plain):
                    if src:
                        order.append(src.pop(0))
            queues.append(order)
        rnd.shuffle(queues)
        got = 0
        while got < quota[kind] and any(queues):        # one crop from each recording in turn
            for q in queues:
                if q and got < quota[kind]:
                    picked.append(q.pop(0))
                    got += 1
    out.mkdir(parents=True, exist_ok=True)
    with open(out / "picks.jsonl", "w", encoding="utf-8") as fh:
        for c in picked:
            split = c["path"].parent.name
            (out / split).mkdir(exist_ok=True)
            shutil.copy2(c["path"], out / split / c["path"].name)
            r = rec[c["stem"]]
            fh.write(json.dumps(dict(file=f"{split}/{c['path'].name}", folder=r["folder"], video=r["file"], kind=c["kind"],
                                     boxes=c["n"], scores=[round(float(s), 3) for s in c["score"]],
                                     frame_boxes=c["frame"], targets=c["targets"], why=c["why"])) + "\n")
    count = collections.Counter((c["kind"], bool(c["why"])) for c in picked)
    print(f"{len(picked)} crops from {len({c['stem'] for c in picked})} recordings into {out}; box sides p5 {lo:.1f} px, "
          f"p95 {hi:.1f} px")
    for kind in sorted({k for k, _ in count}, key=str):
        print(f"  {kind}: {count[(kind, True)]} uncertain, {count[(kind, False)]} plain")
    for w, n in collections.Counter(w.split(" (")[0] if "boxes in" not in w else "box count is not the target count"
                                    for c in picked for w in c["why"]).most_common():
        print(f"  {w}: {n}")


if __name__ == "__main__":
    main()
