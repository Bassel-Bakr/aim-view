"""Evaluate a detector on the held-out crops (REPRODUCE.md step 3): PyTorch checkpoints (.pt) and ONNX exports (.onnx).

Reports, against the automatic labels (so: agreement with the hand-written detector, not ground truth):
  - test split: precision, recall, F1, center error (median and p90, px) at the chosen threshold;
  - the same after recoloring every test crop (fixed seed): does the model ignore color?
  - per scenario folder on the test split;
  - a threshold sweep on the val split (the threshold is chosen there, never on test).
Writes test_out/vod_model/eval/<model>.json.
Usage: python python/model/eval.py <model.pt|model.onnx> [more models...]
"""
import argparse
import hashlib
import json
import sys
from pathlib import Path

import numpy as np
import torch
from torch.utils.data import DataLoader

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import infer  # noqa: E402
import local_config  # noqa: E402
import net  # noqa: E402
import train  # noqa: E402

VODS = local_config.folder("vods")
SWEEP = (0.2, 0.3, 0.4, 0.5, 0.6)       # the thresholds tried on the val split
BATCH = 64
WORKERS = 4
RECOLOR_SEEDS = 10 ** 6                 # each batch's recoloring seed is drawn below this
STEM_CHARS = 10                         # a crop name starts with this many characters of its VOD's md5
WEAKEST = 4                             # test scenarios printed as the weakest
COUNTS = 4                              # a tally: true positives, false positives, false negatives, center errors


def folder_of_files(data):
    """Crop file stem prefix (md5 of the VOD path) to its scenario folder, from the manifest."""
    out = {}
    for line in open(Path(data) / "manifest.jsonl", encoding="utf-8"):
        row = json.loads(line)
        path = str(Path(VODS) / row["folder"] / row["file"])
        out[hashlib.md5(path.encode()).hexdigest()[:STEM_CHARS]] = row["folder"]
    return out


class Runner:
    """One interface over a checkpoint (PyTorch, GPU) and an ONNX file (ONNX Runtime, CPU)."""

    def __init__(self, path):
        self.path = str(path)
        self.onnx = self.path.endswith(".onnx")
        if self.onnx:
            self.detector = infer.OnnxDetector(path)
        else:
            saved = torch.load(path, map_location="cpu", weights_only=False)
            self.model = net.build(saved["config"]).cuda().eval()
            self.model.load_state_dict(saved["model"])

    def __call__(self, inputs, threshold):
        """inputs: (B, 4, S, S) float tensor on the GPU -> list of (n, 5) tensors."""
        if self.onnx:
            crops = inputs.cpu().numpy()
            outs = []
            for i in range(len(crops)):
                if self.detector.takes_bytes:                # _u8in and _embed exports: the crop back to bytes
                    rgb = (crops[i, :3] * 255).round().clip(0, 255).astype(np.uint8).transpose(1, 2, 0)
                    found = self.detector(rgb, crops[i, 3].astype(np.uint8), threshold)
                    outs.append(torch.from_numpy(np.ascontiguousarray(found)))
                    continue
                score, reg = self.detector.session.run(None, {"x": crops[i:i + 1]})
                outs.append(torch.from_numpy(infer.decode_np(score, reg, threshold)))
            return outs
        with torch.no_grad(), torch.autocast("cuda", dtype=torch.bfloat16):
            return [found.cpu() for found in net.decode(self.model(inputs), threshold)]


def added(tally, more):
    """Two tallies (true positives, false positives, false negatives, center errors) added up."""
    return [tally[i] + more[i] for i in range(COUNTS)]


def run(runner, folder, threshold, recolor=False, by_file=False):
    crops = train.Crops(folder)
    loader = DataLoader(crops, BATCH, num_workers=WORKERS)
    seeds = torch.Generator().manual_seed(0)
    total = [0, 0, 0, []]
    per_file = {}
    crop = 0
    for rgb, fixed, target_mask, boxes, count, _ in loader:
        rgb, fixed, target_mask = rgb.cuda(), fixed.cuda(), target_mask.cuda()
        if recolor:
            torch.manual_seed(int(torch.randint(0, RECOLOR_SEEDS, (1,), generator=seeds)))
            image = train.recolour(rgb.permute(0, 3, 1, 2).float() / 255.0, target_mask[:, None].float(), 1.0, 1.0)
            inputs = torch.cat([image, fixed[:, None].float()], 1)
        else:
            inputs = net.prepare(rgb, fixed)
        predictions = runner(inputs, threshold)
        for i in range(len(predictions)):
            tally = train.match([predictions[i]], boxes[i:i + 1], count[i:i + 1])
            total = added(total, tally)
            if by_file:
                key = crops.files[crop].name[:STEM_CHARS]
                per_file[key] = added(per_file.setdefault(key, [0, 0, 0, []]), tally)
            crop += 1
    return train.summarize(*total), per_file


def evaluate(path, args, folders):
    """One model's results (the val sweep unless --thr gives the threshold), written to <out>/<name>.json."""
    runner = Runner(path)
    result = dict(model=path)
    if args.thr is None:
        sweep = {}
        for threshold in SWEEP:
            sweep[threshold] = run(runner, Path(args.data) / "val", threshold)[0]
        threshold = max(sweep, key=lambda tried: sweep[tried]["f1"])
        result["val_sweep"] = {str(tried): numbers for tried, numbers in sweep.items()}
    else:
        threshold = args.thr
    result["threshold"] = threshold
    result["test"], per_file = run(runner, Path(args.data) / "test", threshold, by_file=True)
    result["test_recoloured"] = run(runner, Path(args.data) / "test", threshold, recolor=True)[0]
    by_folder = {}
    for key, tally in per_file.items():
        folder = folders.get(key, key)
        by_folder[folder] = added(by_folder.setdefault(folder, [0, 0, 0, []]), tally)
    result["test_by_scenario"] = {folder: train.summarize(*tally) for folder, tally in sorted(by_folder.items())}
    name = Path(path).stem if path.endswith(".onnx") else Path(path).parent.name
    json.dump(result, open(Path(args.out) / f"{name}.json", "w"), indent=1)
    return name, result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("models", nargs="+")
    parser.add_argument("--data", default="test_out/vod_model/data")
    parser.add_argument("--out", default="test_out/vod_model/eval")
    parser.add_argument("--thr", type=float, help="skip the sweep and use this threshold")
    args = parser.parse_args()
    Path(args.out).mkdir(parents=True, exist_ok=True)
    folders = folder_of_files(args.data)
    for path in args.models:
        name, result = evaluate(path, args, folders)
        test, recolored = result["test"], result["test_recoloured"]
        print(f"{name:22s} thr {result['threshold']}  test P {test['precision']:.3f} R {test['recall']:.3f} "
              f"F1 {test['f1']:.3f} err {test['loc_err_median_px']}/{test['loc_err_p90_px']} px | recoloured F1 "
              f"{recolored['f1']:.3f} err {recolored['loc_err_median_px']} px", flush=True)
        weakest = sorted(result["test_by_scenario"].items(), key=lambda item: item[1]["f1"])[:WEAKEST]
        print("   weakest test scenarios:", "; ".join(f"{folder[:32]} F1 {numbers['f1']:.2f}"
                                                     for folder, numbers in weakest))


if __name__ == "__main__":
    main()
