"""Run the detector (REPRODUCE.md step 6): ONNX Runtime (no PyTorch needed) or PyTorch on the GPU, on whole 1280 x 720
frames. Both give the same detections: (cx, cy, w, h, score) in frame pixels.
  python python/model/infer.py python/model/exports/detector_small_fp32.onnx <video> [--frames 120]
prints the detections of the first frames of a VOD (with the fixed map from its key frames) and the speed.
"""
import argparse
import sys
import time
from pathlib import Path

import json

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

THRESHOLD = 0.3                 # chosen on the val split for small_v2 (eval.py's sweep)
# the model the review app uses (exports/detector_<BEST>.pt or _fp32.onnx): models.json's "default", so a new model
# becomes the default by a change to that file, not to code
BEST = json.loads((Path(__file__).resolve().parent / "models.json").read_text(encoding="utf-8")).get("default", "full_v3")


def decode_np(score, reg, thr=THRESHOLD):
    """The exported model's outputs for one image to an (n, 5) array of cx, cy, w, h, score."""
    s = score[0, 0]
    ys, xs = np.nonzero(s > thr)
    r = reg[0][:, ys, xs]
    return np.stack([(xs + r[0]) * 4, (ys + r[1]) * 4, np.exp(r[2]), np.exp(r[3]), s[ys, xs]], 1).astype(np.float32)


class OnnxDetector:
    def __init__(self, path, threads=0, providers=None):
        import onnxruntime as ort
        so = ort.SessionOptions()
        so.intra_op_num_threads = threads
        self.sess = ort.InferenceSession(str(path), so, providers=providers or ["CPUExecutionProvider"])
        self.u8 = self.sess.get_inputs()[0].name == "rgb"     # _u8in and _embed exports take the raw bytes
        self.dets = self.sess.get_outputs()[0].name == "dets"  # an _embed export gives the best boxes directly

    def __call__(self, rgb, fixed, thr=THRESHOLD):
        """rgb uint8 (H, W, 3), fixed 0/1 (H, W) -> (n, 5)."""
        if self.u8:
            out = self.sess.run(None, {"rgb": rgb[None], "fixed": fixed[None].astype(np.uint8, copy=False)})
            if self.dets:
                d = out[0][0]
                return d[d[:, 4] > thr]
            return decode_np(*out, thr)
        h, w = fixed.shape
        x = np.empty((1, 4, h, w), np.float32)               # one pass, no temporaries: 2 ms instead of 6.6 at 720p
        np.multiply(rgb.transpose(2, 0, 1), np.float32(1 / 255), out=x[0, :3], dtype=np.float32)
        x[0, 3] = fixed
        score, reg = self.sess.run(None, {"x": x})
        return decode_np(score, reg, thr)


class TorchDetector:
    """PyTorch on the GPU, batched: for whole VODs. bf16 autocast, as in training: fp16 or fp32 inference was faster
    or equal in the abstract, but confirmed fewer kills held under the crosshair (Pokeball 1: 55 against 61)."""

    def __init__(self, checkpoint, device="cuda"):
        import torch
        import net
        ck = torch.load(checkpoint, map_location="cpu", weights_only=False)
        self.model = net.build(ck["config"])
        self.model.load_state_dict(ck["model"])
        self.model.eval().to(device)
        self.torch, self.net, self.dev = torch, net, device
        self._fixed = (None, None)
        # bf16 needs an RTX 30-series card or newer to be fast; older ones run fp32 (a few kills under the crosshair
        # fewer confirmed, as on the CPU)
        self.bf16 = device == "cuda" and torch.cuda.is_bf16_supported(including_emulation=False)

    def batch(self, rgbs, fixed, thr=THRESHOLD, k=100):
        """rgbs uint8 (N, H, W, 3), a NumPy array or a (pinned) torch tensor; fixed 0/1 (H, W) shared by the batch ->
        list of (n, 5). The same detections as net.decode (at most k per image, the strongest), but the peaks of the
        whole batch are found on the GPU and copied back once."""
        torch, F = self.torch, self.torch.nn.functional
        with torch.no_grad():
            r = rgbs if isinstance(rgbs, torch.Tensor) else torch.from_numpy(rgbs)
            r = r.to(self.dev, non_blocking=True)
            if self._fixed[0] is not fixed:                    # the fixed map is the same for a whole video
                self._fixed = (fixed, torch.from_numpy(np.ascontiguousarray(fixed)).to(self.dev))
            n = r.shape[0]
            with torch.autocast("cuda", dtype=torch.bfloat16, enabled=self.bf16):
                out = self.model(self.net.prepare(r, self._fixed[1][None].expand(n, -1, -1)))
            hm = torch.sigmoid(out[:, 0:1].float())
            peak = ((hm == F.max_pool2d(hm, 3, 1, 1)) & (hm > thr))[:, 0]
            b, ys, xs = torch.nonzero(peak, as_tuple=True)
            o = out[b, 1:5, ys, xs].float()
            d = torch.stack([(xs.float() + o[:, 0]) * self.net.STRIDE, (ys.float() + o[:, 1]) * self.net.STRIDE,
                             o[:, 2].exp(), o[:, 3].exp(), hm[b, 0, ys, xs]], 1).cpu().numpy()
            b = b.cpu().numpy()
            res = []
            for i in range(n):
                di = d[b == i]
                if len(di) > k:                                 # menus and the like: the k strongest, as net.decode
                    di = di[np.argsort(-di[:, 4], kind="stable")[:k]]
                res.append(di)
            return res

    def __call__(self, rgb, fixed, thr=THRESHOLD):
        return self.batch(rgb[None], fixed, thr)[0]


def main():
    import old_review
    ap = argparse.ArgumentParser()
    ap.add_argument("model")
    ap.add_argument("video")
    ap.add_argument("--frames", type=int, default=120)
    a = ap.parse_args()
    det = TorchDetector(a.model) if a.model.endswith(".pt") else OnnxDetector(a.model)
    keys = list(old_review._frames(a.video, keyframes=True))
    fixed = old_review.fixed_map(keys).astype(np.uint8)
    t = time.perf_counter()
    n = 0
    for k, rgb in enumerate(old_review.rgb_frames(a.video)):
        if k >= a.frames:
            break
        d = det(rgb, fixed)
        n += 1
        if k % 30 == 0:
            print(f"frame {k}: {len(d)} targets: " + ", ".join(f"({x:.1f}, {y:.1f}) {w:.0f}x{h:.0f} {s:.2f}"
                                                              for x, y, w, h, s in d[:6]))
    print(f"{n} frames in {time.perf_counter() - t:.1f} s (decode included)")


if __name__ == "__main__":
    main()
