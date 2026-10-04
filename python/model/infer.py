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
STRIDE = 4                      # frame pixels per output cell (net.STRIDE; this module needs no PyTorch)
PEAK_WINDOW = 3                 # cells: net.PEAK_WINDOW
MAX_DETECTIONS = 100            # per frame at most, the strongest: net.MAX_DETECTIONS
PRINT_EVERY = 30                # main: the frames whose detections are printed
PRINTED = 6                     # main: detections printed per frame


def decode_np(score, reg, threshold=THRESHOLD):
    """The exported model's outputs for one image to an (n, 5) array of cx, cy, w, h, score."""
    scores = score[0, 0]
    ys, xs = np.nonzero(scores > threshold)
    cells = reg[0][:, ys, xs]
    return np.stack([(xs + cells[0]) * STRIDE, (ys + cells[1]) * STRIDE, np.exp(cells[2]), np.exp(cells[3]),
                     scores[ys, xs]], 1).astype(np.float32)


class OnnxDetector:
    def __init__(self, path, threads=0, providers=None):
        import onnxruntime as ort
        options = ort.SessionOptions()
        options.intra_op_num_threads = threads
        self.session = ort.InferenceSession(str(path), options, providers=providers or ["CPUExecutionProvider"])
        # _u8in and _embed exports take the raw bytes; an _embed export gives the best boxes directly
        self.takes_bytes = self.session.get_inputs()[0].name == "rgb"
        self.gives_boxes = self.session.get_outputs()[0].name == "dets"

    def __call__(self, rgb, fixed, threshold=THRESHOLD):
        """rgb uint8 (H, W, 3), fixed 0/1 (H, W) -> (n, 5)."""
        if self.takes_bytes:
            out = self.session.run(None, {"rgb": rgb[None], "fixed": fixed[None].astype(np.uint8, copy=False)})
            if self.gives_boxes:
                boxes = out[0][0]
                return boxes[boxes[:, 4] > threshold]
            return decode_np(*out, threshold)
        height, width = fixed.shape
        frame = np.empty((1, 4, height, width), np.float32)  # one pass, no temporaries: 2 ms instead of 6.6 at 720p
        np.multiply(rgb.transpose(2, 0, 1), np.float32(1 / 255), out=frame[0, :3], dtype=np.float32)
        frame[0, 3] = fixed
        score, reg = self.session.run(None, {"x": frame})
        return decode_np(score, reg, threshold)


class TorchDetector:
    """PyTorch on the GPU, batched: for whole VODs. bf16 autocast, as in training: fp16 or fp32 inference was faster
    or equal in the abstract, but confirmed fewer kills held under the crosshair (Pokeball 1: 55 against 61)."""

    def __init__(self, checkpoint, device="cuda"):
        import torch
        import net
        saved = torch.load(checkpoint, map_location="cpu", weights_only=False)
        self.model = net.build(saved["config"])
        self.model.load_state_dict(saved["model"])
        self.model.eval().to(device)
        self.torch, self.net, self.device = torch, net, device
        self._fixed = (None, None)
        # bf16 needs an RTX 30-series card or newer to be fast; older ones run fp32 (a few kills under the crosshair
        # fewer confirmed, as on the CPU)
        self.bf16 = device == "cuda" and torch.cuda.is_bf16_supported(including_emulation=False)

    def batch(self, frames, fixed, threshold=THRESHOLD, max_detections=MAX_DETECTIONS):
        """frames uint8 (N, H, W, 3), a NumPy array or a (pinned) torch tensor; fixed 0/1 (H, W) shared by the batch
        -> list of (n, 5). The same detections as net.decode (at most max_detections per image, the strongest), but
        the peaks of the whole batch are found on the GPU and copied back once."""
        torch, functional = self.torch, self.torch.nn.functional
        with torch.no_grad():
            rgb = frames if isinstance(frames, torch.Tensor) else torch.from_numpy(frames)
            rgb = rgb.to(self.device, non_blocking=True)
            if self._fixed[0] is not fixed:                    # the fixed map is the same for a whole video
                self._fixed = (fixed, torch.from_numpy(np.ascontiguousarray(fixed)).to(self.device))
            count = rgb.shape[0]
            with torch.autocast("cuda", dtype=torch.bfloat16, enabled=self.bf16):
                out = self.model(self.net.prepare(rgb, self._fixed[1][None].expand(count, -1, -1)))
            heat = torch.sigmoid(out[:, 0:1].float())
            peak = ((heat == functional.max_pool2d(heat, PEAK_WINDOW, 1, 1)) & (heat > threshold))[:, 0]
            image, ys, xs = torch.nonzero(peak, as_tuple=True)
            cells = out[image, 1:5, ys, xs].float()
            found = torch.stack([(xs.float() + cells[:, 0]) * self.net.STRIDE, (ys.float() + cells[:, 1]) * self.net.STRIDE,
                                 cells[:, 2].exp(), cells[:, 3].exp(), heat[image, 0, ys, xs]], 1).cpu().numpy()
            image = image.cpu().numpy()
            per_frame = []
            for i in range(count):
                detections = found[image == i]
                if len(detections) > max_detections:           # menus and the like: the strongest, as net.decode
                    detections = detections[np.argsort(-detections[:, 4], kind="stable")[:max_detections]]
                per_frame.append(detections)
            return per_frame

    def __call__(self, rgb, fixed, threshold=THRESHOLD):
        return self.batch(rgb[None], fixed, threshold)[0]


def main():
    import old_review
    parser = argparse.ArgumentParser()
    parser.add_argument("model")
    parser.add_argument("video")
    parser.add_argument("--frames", type=int, default=120)
    args = parser.parse_args()
    detector = TorchDetector(args.model) if args.model.endswith(".pt") else OnnxDetector(args.model)
    keys = list(old_review._frames(args.video, keyframes=True))
    fixed = old_review.fixed_map(keys).astype(np.uint8)
    started = time.perf_counter()
    done = 0
    for frame, rgb in enumerate(old_review.rgb_frames(args.video)):
        if frame >= args.frames:
            break
        detections = detector(rgb, fixed)
        done += 1
        if frame % PRINT_EVERY == 0:
            print(f"frame {frame}: {len(detections)} targets: " + ", ".join(
                f"({x:.1f}, {y:.1f}) {width:.0f}x{height:.0f} {score:.2f}"
                for x, y, width, height, score in detections[:PRINTED]))
    print(f"{done} frames in {time.perf_counter() - started:.1f} s (decode included)")


if __name__ == "__main__":
    main()
