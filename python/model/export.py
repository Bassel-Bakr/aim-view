"""Export a trained detector to ONNX (REPRODUCE.md steps 4-5): fp32, fp16 and int8 (static QDQ quantization, calibrated
on validation crops). The exported graph includes the decoding that is awkward elsewhere (sigmoid and 3x3 peak finding),
so a caller in any language only scans for cells over a threshold:
  input  "x"      float32 (1, 4, H, W)    RGB 0-1 and the fixed map 0/1; H, W multiples of 16 (1280 x 720 is fine)
  output "score"  float32 (1, 1, H/4, W/4) the centre score where it is a local maximum, else 0
  output "reg"    float32 (1, 4, H/4, W/4) offset x, y within the cell (0-1), log width, log height (input px)
The _u8in file takes the raw bytes instead: "rgb" uint8 (N, H, W, 3) and "fixed" uint8 (N, H, W), the same outputs
for N frames at once (N is free: a browser runs several frames in one call).
The _embed file takes the raw bytes and gives "dets" float32 (1, 100, 5): the 100 best peaks as cx, cy, w, h, score,
best first; keep the rows over the threshold.
A detection at cell (i, j) with score > threshold: cx = (j + reg0) * 4, cy = (i + reg1) * 4, w = exp(reg2), h = exp(reg3).
Usage: python python/model/export.py test_out/vod_model/runs/small/best.pt [--out python/model/exports]
       python python/model/export.py <checkpoint> --u8in [--out DIR]   (the _u8in file only, checked against the
       fp32 file beside it frame by frame and in a batch)
"""
import argparse
import json
import sys
from pathlib import Path

import numpy as np
import onnx
import torch
import torch.nn as nn
import torch.nn.functional as F

sys.path.insert(0, str(Path(__file__).resolve().parent))
import net  # noqa: E402


class Exported(nn.Module):
    def __init__(self, model):
        super().__init__()
        self.model = model

    def forward(self, x):
        out = self.model(x)
        heat = torch.sigmoid(out[:, 0:1])
        score = heat * (heat == F.max_pool2d(heat, 3, 1, 1)).float()
        return score, out[:, 1:5]


class Exported16(nn.Module):
    def __init__(self, model):
        super().__init__()
        self.model = model.half()

    def forward(self, x):
        out = self.model(x.half()).float()
        heat = torch.sigmoid(out[:, 0:1])
        score = heat * (heat == F.max_pool2d(heat, 3, 1, 1)).float()
        return score, out[:, 1:5]


class ExportedU8(nn.Module):
    """The fp32 model with the pre-processing inside the graph: raw uint8 frame bytes in, so a caller in any language
    passes the decoder's buffer as it is (and a browser uploads 3.7 MB per 720p frame, not 14.7 MB of floats)."""

    def __init__(self, model):
        super().__init__()
        self.inner = Exported(model)

    def forward(self, rgb, fixed):
        # the fixed map to float before its channel axis: onnxruntime's WebGPU build has no Unsqueeze for uint8, and a
        # node left on the CPU rules out graph capture (the same values either way)
        x = torch.cat([rgb.permute(0, 3, 1, 2).float() / 255.0, fixed.float()[:, None]], 1)
        return self.inner(x)


class ExportedEmbed(nn.Module):
    """Raw bytes in, boxes out: the uint8-input model followed by the top K peaks, so a caller reads K rows of
    (cx, cy, w, h, score), best first, and keeps those over its threshold. No map to scan."""

    def __init__(self, model, k=100):
        super().__init__()
        self.inner, self.k = ExportedU8(model), k

    def forward(self, rgb, fixed):
        score, reg = self.inner(rgb, fixed)
        w4 = score.shape[3]
        s, i = torch.topk(score.flatten(1), self.k, dim=1)                       # (1, K)
        r = torch.gather(reg.flatten(2), 2, i[:, None].expand(-1, 4, -1))         # (1, 4, K)
        xs, ys = (i % w4).float(), torch.div(i, w4, rounding_mode="floor").float()
        dets = torch.stack([(xs + r[:, 0]) * 4, (ys + r[:, 1]) * 4, r[:, 2].exp(), r[:, 3].exp(), s], 2)
        return dets                                                               # (1, K, 5)


def export_u8in(model, path):
    """The uint8-input graph, with a free batch axis. Traced with 2 frames, so nothing in it is fixed to one frame."""
    torch.onnx.export(ExportedU8(model).eval(), (torch.zeros(2, 720, 1280, 3, dtype=torch.uint8),
                                                 torch.zeros(2, 720, 1280, dtype=torch.uint8)), str(path),
                      input_names=["rgb", "fixed"], output_names=["score", "reg"],
                      dynamic_axes={"rgb": {0: "n", 1: "h", 2: "w"}, "fixed": {0: "n", 1: "h", 2: "w"},
                                    "score": {0: "n", 2: "h4", 3: "w4"}, "reg": {0: "n", 2: "h4", 3: "w4"}},
                      opset_version=17, dynamo=False)


def check_u8in(u8, f32):
    """The uint8-input graph against the fp32 one on a real frame, alone and as the second of a batch of 4 (the
    others different frames): the same outputs."""
    import onnxruntime as ort
    import bench
    rgb, fixed = bench.sample()
    xr = net.prepare(torch.from_numpy(rgb)[None], torch.from_numpy(fixed)[None])
    s_o, r_o = ort.InferenceSession(str(f32), providers=["CPUExecutionProvider"]).run(None, {"x": xr.numpy()})
    sess = ort.InferenceSession(str(u8), providers=["CPUExecutionProvider"])
    s_u, r_u = sess.run(None, {"rgb": rgb[None], "fixed": fixed[None].astype(np.uint8)})
    print(f"uint8-input graph against fp32: max |reg| diff {np.abs(r_u - r_o).max():.2e}")
    if np.abs(r_u - r_o).max() > 1e-3:
        raise SystemExit("the uint8-input export does not match")
    batch = np.stack([np.roll(rgb, 37 * k, axis=1) if k != 1 else rgb for k in range(4)])
    fixed4 = np.repeat(fixed[None].astype(np.uint8), 4, axis=0)
    s_b, r_b = sess.run(None, {"rgb": batch, "fixed": fixed4})
    print(f"a frame in a batch of 4 against alone: max |score| diff {np.abs(s_b[1] - s_u[0]).max():.2e}, "
          f"max |reg| diff {np.abs(r_b[1] - r_u[0]).max():.2e}")
    if s_b.shape[0] != 4 or np.abs(r_b[1] - r_u[0]).max() > 1e-4:
        raise SystemExit("the batch axis changes the outputs")


def calibration_reader(data_dir, n=64):
    from onnxruntime.quantization import CalibrationDataReader

    files = sorted(Path(data_dir).glob("*.npz"))[::max(1, len(sorted(Path(data_dir).glob("*.npz"))) // n)][:n]

    class Reader(CalibrationDataReader):
        def __init__(self):
            self.it = iter(files)

        def get_next(self):
            f = next(self.it, None)
            if f is None:
                return None
            z = np.load(f)
            x = np.concatenate([z["rgb"].transpose(2, 0, 1)[None] / 255.0, z["fixed"][None, None]], 1)
            return {"x": x.astype(np.float32)}

    return Reader()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("checkpoint")
    ap.add_argument("--out", default="python/model/exports")
    ap.add_argument("--data", default="test_out/vod_model/data")
    ap.add_argument("--u8in", action="store_true", help="only the _u8in file (the fp32 file must be beside it)")
    a = ap.parse_args()
    ck = torch.load(a.checkpoint, map_location="cpu", weights_only=False)
    cfg = ck["config"]
    model = net.build(cfg)
    model.load_state_dict(ck["model"])
    model.eval()
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    name = cfg["name"]
    f32 = out / f"detector_{name}_fp32.onnx"
    if a.u8in:
        u8 = out / f"detector_{name}_u8in.onnx"
        export_u8in(model, u8)
        check_u8in(u8, f32 if f32.exists() else Path(__file__).parent / "exports" / f32.name)
        print(f"{u8}: {u8.stat().st_size / 1024:.1f} KB")
        return
    x = torch.zeros(1, 4, 720, 1280)
    torch.onnx.export(Exported(model).eval(), (x,), str(f32), input_names=["x"], output_names=["score", "reg"],
                      dynamic_axes={"x": {2: "h", 3: "w"}, "score": {2: "h4", 3: "w4"}, "reg": {2: "h4", 3: "w4"}},
                      opset_version=17, dynamo=False)
    m = onnx.load(str(f32))
    m.metadata_props.add(key="config", value=json.dumps(cfg))
    m.metadata_props.add(key="epoch", value=str(ck["epoch"]))
    m.metadata_props.add(key="val", value=json.dumps(ck["val"]))
    onnx.save(m, str(f32))
    onnx.checker.check_model(str(f32))
    # fp16: traced from PyTorch with the network in half precision and the decoding in fp32 (onnxconverter-common's
    # converter breaks the decoding's Cast, and hangs when told to leave those nodes in fp32)
    import copy
    f16 = out / f"detector_{name}_fp16.onnx"
    if torch.cuda.is_available():
        torch.onnx.export(Exported16(copy.deepcopy(model)).eval().cuda(), (x.cuda(),), str(f16), input_names=["x"],
                          output_names=["score", "reg"],
                          dynamic_axes={"x": {2: "h", 3: "w"}, "score": {2: "h4", 3: "w4"}, "reg": {2: "h4", 3: "w4"}},
                          opset_version=17, dynamo=False)
    # uint8 in: rgb (N, H, W, 3) and fixed (N, H, W), both uint8, same outputs
    u8 = out / f"detector_{name}_u8in.onnx"
    export_u8in(model, u8)
    # raw bytes in, the 100 best boxes out: "dets" float32 (1, 100, 5)
    emb = out / f"detector_{name}_embed.onnx"
    torch.onnx.export(ExportedEmbed(model).eval(), (torch.zeros(1, 720, 1280, 3, dtype=torch.uint8),
                                                    torch.zeros(1, 720, 1280, dtype=torch.uint8)), str(emb),
                      input_names=["rgb", "fixed"], output_names=["dets"],
                      dynamic_axes={"rgb": {1: "h", 2: "w"}, "fixed": {1: "h", 2: "w"}}, opset_version=17, dynamo=False)
    from onnxruntime.quantization import QuantFormat, QuantType, quantize_static
    from onnxruntime.quantization.shape_inference import quant_pre_process
    pre = out / f"_pre_{name}.onnx"
    quant_pre_process(str(f32), str(pre), skip_symbolic_shape=True)
    i8 = out / f"detector_{name}_int8.onnx"
    quantize_static(str(pre), str(i8), calibration_reader(Path(a.data) / "val"), quant_format=QuantFormat.QDQ,
                    per_channel=True, activation_type=QuantType.QUInt8, weight_type=QuantType.QInt8)
    pre.unlink(missing_ok=True)
    # parity: ONNX fp32 against PyTorch on a real 1280 x 720 KovOBS frame (random noise is a poor test: it drives the
    # activations far outside anything a frame produces, where tiny summation-order differences grow)
    import onnxruntime as ort
    import bench
    rgb, fixed = bench.sample()
    xr = net.prepare(torch.from_numpy(rgb)[None], torch.from_numpy(fixed)[None])
    with torch.no_grad():
        s_t, r_t = Exported(model.eval()).eval()(xr)
    s_o, r_o = ort.InferenceSession(str(f32), providers=["CPUExecutionProvider"]).run(None, {"x": xr.numpy()})
    sd, rd = np.abs(s_o - s_t.numpy()).max(), np.abs(r_o - r_t.numpy()).max()
    print(f"parity fp32 on a real frame: max |score| diff {sd:.2e}, max |reg| diff {rd:.2e}")
    if rd > 1e-3:
        raise SystemExit("the ONNX export does not match PyTorch")
    check_u8in(u8, f32)
    (dets,) = ort.InferenceSession(str(emb), providers=["CPUExecutionProvider"]).run(
        None, {"rgb": rgb[None], "fixed": fixed[None].astype(np.uint8)})
    import infer
    want = infer.decode_np(s_o, r_o, 0.3)
    got = dets[0][dets[0][:, 4] > 0.3]
    order = lambda d: d[np.argsort(d[:, 0])]
    if len(got) != len(want) or np.abs(order(got) - order(want)).max() > 1e-3:
        raise SystemExit("the embed export does not match")
    print(f"embed graph: the same {len(got)} detections over 0.3")
    if f16.exists():
        s16, r16 = ort.InferenceSession(str(f16), providers=["CPUExecutionProvider"]).run(None, {"x": xr.numpy()})
        print(f"fp16 against fp32 on a real frame: max |reg| diff where a target is {np.abs(r16 - r_o)[:, :, s_o[0, 0] > 0.3].max():.3f}")
    for p in (f32, f16, u8, emb, i8):
        if p.exists():
            print(f"{p}: {p.stat().st_size / 1024:.1f} KB")


if __name__ == "__main__":
    main()
