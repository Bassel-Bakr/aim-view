"""Evaluate a detector on the held-out crops (REPRODUCE.md step 3): PyTorch checkpoints (.pt) and ONNX exports (.onnx).

Reports, against the automatic labels (so: agreement with the hand-written detector, not ground truth):
  - test split: precision, recall, F1, centre error (median and p90, px) at the chosen threshold;
  - the same after recolouring every test crop (fixed seed): does the model ignore colour?
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
import infer  # noqa: E402
import net  # noqa: E402
import train  # noqa: E402

VODS = r"E:\OBS\KovOBS"


def folder_of_files(data):
    """Crop file stem prefix (md5 of the VOD path) to its scenario folder, from the manifest."""
    out = {}
    for l in open(Path(data) / "manifest.jsonl", encoding="utf-8"):
        r = json.loads(l)
        path = str(Path(VODS) / r["folder"] / r["file"])
        out[hashlib.md5(path.encode()).hexdigest()[:10]] = r["folder"]
    return out


class Runner:
    """One interface over a checkpoint (PyTorch, GPU) and an ONNX file (ONNX Runtime, CPU)."""

    def __init__(self, path):
        self.path = str(path)
        self.onnx = self.path.endswith(".onnx")
        if self.onnx:
            self.det = infer.OnnxDetector(path)
        else:
            ck = torch.load(path, map_location="cpu", weights_only=False)
            self.model = net.build(ck["config"]).cuda().eval()
            self.model.load_state_dict(ck["model"])

    def __call__(self, x, thr):
        """x: (B, 4, S, S) float tensor on the GPU -> list of (n, 5) tensors."""
        if self.onnx:
            xs = x.cpu().numpy()
            outs = []
            for i in range(len(xs)):
                if self.det.u8:                              # _u8in and _embed exports: the crop back to bytes
                    rgb = (xs[i, :3] * 255).round().clip(0, 255).astype(np.uint8).transpose(1, 2, 0)
                    outs.append(torch.from_numpy(np.ascontiguousarray(self.det(rgb, xs[i, 3].astype(np.uint8), thr))))
                    continue
                score, reg = self.det.sess.run(None, {"x": xs[i:i + 1]})
                outs.append(torch.from_numpy(infer.decode_np(score, reg, thr)))
            return outs
        with torch.no_grad(), torch.autocast("cuda", dtype=torch.bfloat16):
            return [d.cpu() for d in net.decode(self.model(x), thr)]


def run(runner, folder, thr, recolour=False, by_file=False):
    ds = train.Crops(folder)
    dl = DataLoader(ds, 64, num_workers=4)
    g = torch.Generator().manual_seed(0)
    tot = [0, 0, 0, []]
    per = {}
    k = 0
    for rgb, fixed, tmask, boxes, n, _ in dl:
        rgb, fixed, tmask = rgb.cuda(), fixed.cuda(), tmask.cuda()
        if recolour:
            torch.manual_seed(int(torch.randint(0, 10 ** 6, (1,), generator=g)))
            img = train.recolour(rgb.permute(0, 3, 1, 2).float() / 255.0, tmask[:, None].float(), 1.0, 1.0)
            x = torch.cat([img, fixed[:, None].float()], 1)
        else:
            x = net.prepare(rgb, fixed)
        preds = runner(x, thr)
        for i in range(len(preds)):
            a, b, c, e = train.match([preds[i]], boxes[i:i + 1], n[i:i + 1])
            tot = [tot[0] + a, tot[1] + b, tot[2] + c, tot[3] + e]
            if by_file:
                key = ds.files[k].name[:10]
                p = per.setdefault(key, [0, 0, 0, []])
                per[key] = [p[0] + a, p[1] + b, p[2] + c, p[3] + e]
            k += 1
    return train.summarize(*tot), per


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("models", nargs="+")
    ap.add_argument("--data", default="test_out/vod_model/data")
    ap.add_argument("--out", default="test_out/vod_model/eval")
    ap.add_argument("--thr", type=float, help="skip the sweep and use this threshold")
    a = ap.parse_args()
    Path(a.out).mkdir(parents=True, exist_ok=True)
    folders = folder_of_files(a.data)
    for path in a.models:
        r = Runner(path)
        res = dict(model=path)
        if a.thr is None:
            sweep = {}
            for thr in (0.2, 0.3, 0.4, 0.5, 0.6):
                sweep[thr] = run(r, Path(a.data) / "val", thr)[0]
            thr = max(sweep, key=lambda t: sweep[t]["f1"])
            res["val_sweep"] = {str(k): v for k, v in sweep.items()}
        else:
            thr = a.thr
        res["threshold"] = thr
        res["test"], per = run(r, Path(a.data) / "test", thr, by_file=True)
        res["test_recoloured"] = run(r, Path(a.data) / "test", thr, recolour=True)[0]
        by_folder = {}
        for key, v in per.items():
            f = folders.get(key, key)
            p = by_folder.setdefault(f, [0, 0, 0, []])
            by_folder[f] = [p[i] + v[i] for i in range(4)]
        res["test_by_scenario"] = {f: train.summarize(*v) for f, v in sorted(by_folder.items())}
        name = Path(path).stem if path.endswith(".onnx") else Path(path).parent.name
        json.dump(res, open(Path(a.out) / f"{name}.json", "w"), indent=1)
        t, rc = res["test"], res["test_recoloured"]
        print(f"{name:22s} thr {thr}  test P {t['precision']:.3f} R {t['recall']:.3f} F1 {t['f1']:.3f} "
              f"err {t['loc_err_median_px']}/{t['loc_err_p90_px']} px | recoloured F1 {rc['f1']:.3f} "
              f"err {rc['loc_err_median_px']} px", flush=True)
        worst = sorted(res["test_by_scenario"].items(), key=lambda kv: kv[1]["f1"])[:4]
        print("   weakest test scenarios:", "; ".join(f"{f[:32]} F1 {v['f1']:.2f}" for f, v in worst))


if __name__ == "__main__":
    main()
