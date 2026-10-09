"""Export a trained detector to ONNX (REPRODUCE.md steps 4-5): fp32, fp16 and int8 (static QDQ quantization, calibrated
on validation crops). The exported graph includes the decoding that is awkward elsewhere (sigmoid and 3x3 peak finding),
so a caller in any language only scans for cells over a threshold:
  input  "x"      float32 (1, 4, H, W)    RGB 0-1 and the fixed map 0/1; H, W multiples of 16 (1280 x 720 is fine)
  output "score"  float32 (1, 1, H/4, W/4) the center score where it is a local maximum, else 0
  output "reg"    float32 (1, 4, H/4, W/4) offset x, y within the cell (0-1), log width, log height (input px)
The _u8in file takes the raw bytes instead: "rgb" uint8 (N, H, W, 3) and "fixed" uint8 (N, H, W), the same outputs
for N frames at once (N is free: a browser runs several frames in one call).
The _embed file takes the raw bytes and gives "dets" float32 (1, 100, 5): the 100 best peaks as cx, cy, w, h, score,
best first; keep the rows over the threshold.
A detection at cell (i, j) with score > threshold: cx = (j + reg0) * 4, cy = (i + reg1) * 4, w = exp(reg2),
h = exp(reg3).
Last, the model's settings file detector_<name>.json (calibrate.py: its scores on the reference model's scale and its
threshold there; written only when the file does not exist yet), with the numbers behind it in
python/model/reports/calibration_<name>.json. Then check the model with python/model/contract.py <name>.
Usage: python python/model/export.py test_out/vod_model/runs/small/best.pt [--out python/model/exports]
       [--data <dataset>] [--val <dataset> ...]   (--data: the int8 calibration's and the threshold's val split;
       --val: the score map's datasets, calibrate.VAL by default)
       python python/model/export.py <checkpoint> --u8in [--out DIR]   (the _u8in file only, checked against the
       fp32 file beside it frame by frame and in a batch)
"""
import argparse
import copy
import json
import sys
from pathlib import Path

import numpy as np
import onnx
import torch
import torch.nn as nn
import torch.nn.functional as F

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import net  # noqa: E402
from local_config import folder  # noqa: E402

HEIGHT_PX, WIDTH_PX = 720, 1280         # the frame the graphs are traced with
OPSET = 17                              # the ONNX operator set the graphs use
PEAK_WINDOW = 3                         # cells: the 3 x 3 peak finding
EMBED_TOP = 100                         # the _embed graph's boxes, best first
TRACE_FRAMES = 2                        # the _u8in graph is traced with 2 frames: no axis fixed to one frame
BATCH_FRAMES = 4                        # the batch the _u8in check runs (the service and the browser send 4)
SHIFT_PX = 37                           # the batch's other frames: the frame rolled by multiples of this
SAME_REG = 1e-3                         # an export matches when its box values differ by no more than this
SAME_IN_BATCH = 1e-4                    # a frame in a batch gives what it gives alone, within this
CHECK_THRESHOLD = 0.3                   # the embed and fp16 checks' threshold
CALIBRATION_CROPS = 64                  # validation crops the int8 quantization is calibrated on
KB = 1024
# the float graphs' free axes: the input's height and width, and the outputs' (a quarter of them)
SPATIAL_AXES = {2: "h", 3: "w"}
CELL_AXES = {2: "h4", 3: "w4"}


class Exported(nn.Module):
    """The network with the decoding's first steps inside: the "score" and "reg" outputs of the module's docstring."""

    def __init__(self, model):
        """model: a net.Detector, its weights loaded."""
        super().__init__()
        self.model = model

    def forward(self, frames):
        """(score, reg) for the input (N, 4, H, W): the sigmoid of the heatmap where it is a 3 x 3 peak (else 0), and
        the offset and size channels."""
        out = self.model(frames)
        heat = torch.sigmoid(out[:, 0:1])
        score = heat * (heat == F.max_pool2d(heat, PEAK_WINDOW, 1, 1)).float()
        return score, out[:, 1:5]


class Exported16(nn.Module):
    """Exported with the network in half precision and the decoding in fp32."""

    def __init__(self, model):
        """model: a net.Detector, which this turns to half precision in place (pass a copy)."""
        super().__init__()
        self.model = model.half()

    def forward(self, frames):
        """Exported's outputs, the network run in fp16 on the fp32 input."""
        out = self.model(frames.half()).float()
        heat = torch.sigmoid(out[:, 0:1])
        score = heat * (heat == F.max_pool2d(heat, PEAK_WINDOW, 1, 1)).float()
        return score, out[:, 1:5]


class ExportedU8(nn.Module):
    """The fp32 model with the pre-processing inside the graph: raw uint8 frame bytes in, so a caller in any language
    passes the decoder's buffer as it is (and a browser uploads 3.7 MB per 720p frame, not 14.7 MB of floats)."""

    def __init__(self, model):
        """model: a net.Detector, its weights loaded."""
        super().__init__()
        self.inner = Exported(model)

    def forward(self, rgb, fixed):
        """Exported's outputs for rgb uint8 (N, H, W, 3) and the fixed map uint8 (N, H, W)."""
        # the fixed map to float before its channel axis: onnxruntime's WebGPU build has no Unsqueeze for uint8, and a
        # node left on the CPU rules out graph capture (the same values either way)
        frames = torch.cat([rgb.permute(0, 3, 1, 2).float() / 255.0, fixed.float()[:, None]], 1)
        return self.inner(frames)


class ExportedEmbed(nn.Module):
    """Raw bytes in, boxes out: the uint8-input model followed by the top K peaks, so a caller reads K rows of
    (cx, cy, w, h, score), best first, and keeps those over its threshold. No map to scan."""

    def __init__(self, model, k=EMBED_TOP):
        """model: a net.Detector, its weights loaded; k: the boxes the graph gives."""
        super().__init__()
        self.inner, self.k = ExportedU8(model), k

    def forward(self, rgb, fixed):
        """The k highest-scoring cells of one frame as (1, k, 5) boxes: cx, cy, w, h (input px) and score."""
        score, reg = self.inner(rgb, fixed)
        cells_wide = score.shape[3]
        scores, cells = torch.topk(score.flatten(1), self.k, dim=1)                          # (1, K)
        values = torch.gather(reg.flatten(2), 2, cells[:, None].expand(-1, 4, -1))         # (1, 4, K)
        xs, ys = (cells % cells_wide).float(), torch.div(cells, cells_wide, rounding_mode="floor").float()
        dets = torch.stack([(xs + values[:, 0]) * net.STRIDE, (ys + values[:, 1]) * net.STRIDE, values[:, 2].exp(),
                            values[:, 3].exp(), scores], 2)
        return dets                                                                          # (1, K, 5)


def byte_inputs(frames):
    """Zero uint8 frames and fixed maps to trace the raw-bytes graphs with."""
    return (torch.zeros(frames, HEIGHT_PX, WIDTH_PX, 3, dtype=torch.uint8),
            torch.zeros(frames, HEIGHT_PX, WIDTH_PX, dtype=torch.uint8))


def export_u8in(model, path):
    """The uint8-input graph, with a free batch axis. Traced with 2 frames, so nothing in it is fixed to one frame."""
    torch.onnx.export(ExportedU8(model).eval(), byte_inputs(TRACE_FRAMES), str(path),
                      input_names=["rgb", "fixed"], output_names=["score", "reg"],
                      dynamic_axes={"rgb": {0: "n", 1: "h", 2: "w"}, "fixed": {0: "n", 1: "h", 2: "w"},
                                    "score": {0: "n", 2: "h4", 3: "w4"}, "reg": {0: "n", 2: "h4", 3: "w4"}},
                      opset_version=OPSET, dynamo=False)


def cpu_session(path):
    """An ONNX Runtime session of the file on the CPU."""
    import onnxruntime as ort
    return ort.InferenceSession(str(path), providers=["CPUExecutionProvider"])


def check_u8in(u8, f32):
    """The uint8-input graph against the fp32 one on a real frame, alone and as the second of a batch of 4 (the
    others different frames): the same outputs."""
    import bench
    rgb, fixed = bench.sample()
    frame = net.prepare(torch.from_numpy(rgb)[None], torch.from_numpy(fixed)[None])
    _, float_reg = cpu_session(f32).run(None, {"x": frame.numpy()})
    session = cpu_session(u8)
    byte_score, byte_reg = session.run(None, {"rgb": rgb[None], "fixed": fixed[None].astype(np.uint8)})
    print(f"uint8-input graph against fp32: max |reg| diff {np.abs(byte_reg - float_reg).max():.2e}")
    if np.abs(byte_reg - float_reg).max() > SAME_REG:
        raise SystemExit("the uint8-input export does not match")
    batch = np.stack([np.roll(rgb, SHIFT_PX * k, axis=1) if k != 1 else rgb for k in range(BATCH_FRAMES)])
    fixed_maps = np.repeat(fixed[None].astype(np.uint8), BATCH_FRAMES, axis=0)
    batch_score, batch_reg = session.run(None, {"rgb": batch, "fixed": fixed_maps})
    print(f"a frame in a batch of 4 against alone: max |score| diff {np.abs(batch_score[1] - byte_score[0]).max():.2e}, "
          f"max |reg| diff {np.abs(batch_reg[1] - byte_reg[0]).max():.2e}")
    if batch_score.shape[0] != BATCH_FRAMES or np.abs(batch_reg[1] - byte_reg[0]).max() > SAME_IN_BATCH:
        raise SystemExit("the batch axis changes the outputs")


def calibration_reader(data_dir, count=CALIBRATION_CROPS):
    """The int8 quantization's inputs: `count` crops of the folder, spread over it in name order."""
    from onnxruntime.quantization import CalibrationDataReader

    every = sorted(Path(data_dir).glob("*.npz"))
    files = every[::max(1, len(every) // count)][:count]

    class Reader(CalibrationDataReader):
        """Gives quantize_static the crops one at a time as the fp32 graph's input."""

        def __init__(self):
            """Starts at the first crop."""
            self.remaining = iter(files)

        def get_next(self):
            """The next crop as {"x": (1, 4, 256, 256) float32}, or None after the last."""
            file = next(self.remaining, None)
            if file is None:
                return None
            crop = np.load(file)
            frame = np.concatenate([crop["rgb"].transpose(2, 0, 1)[None] / 255.0, crop["fixed"][None, None]], 1)
            return {"x": frame.astype(np.float32)}

    return Reader()


def export_fp32(model, saved, path, frame):
    """The fp32 graph, with the checkpoint's config, epoch and val numbers in its metadata."""
    torch.onnx.export(Exported(model).eval(), (frame,), str(path), input_names=["x"], output_names=["score", "reg"],
                      dynamic_axes={"x": SPATIAL_AXES, "score": CELL_AXES, "reg": CELL_AXES},
                      opset_version=OPSET, dynamo=False)
    graph = onnx.load(str(path))
    graph.metadata_props.add(key="config", value=json.dumps(saved["config"]))
    graph.metadata_props.add(key="epoch", value=str(saved["epoch"]))
    graph.metadata_props.add(key="val", value=json.dumps(saved["val"]))
    onnx.save(graph, str(path))
    onnx.checker.check_model(str(path))


def export_fp16(model, path, frame):
    """fp16: traced from PyTorch with the network in half precision and the decoding in fp32 (onnxconverter-common's
    converter breaks the decoding's Cast, and hangs when told to leave those nodes in fp32). Only with a GPU."""
    if torch.cuda.is_available():
        torch.onnx.export(Exported16(copy.deepcopy(model)).eval().cuda(), (frame.cuda(),), str(path), input_names=["x"],
                          output_names=["score", "reg"],
                          dynamic_axes={"x": SPATIAL_AXES, "score": CELL_AXES, "reg": CELL_AXES},
                          opset_version=OPSET, dynamo=False)


def export_embed(model, path):
    """Raw bytes in, the 100 best boxes out: "dets" float32 (1, 100, 5)."""
    torch.onnx.export(ExportedEmbed(model).eval(), byte_inputs(1), str(path),
                      input_names=["rgb", "fixed"], output_names=["dets"],
                      dynamic_axes={"rgb": {1: "h", 2: "w"}, "fixed": {1: "h", 2: "w"}}, opset_version=OPSET,
                      dynamo=False)


def export_int8(f32, path, out, name, data):
    """int8: static QDQ quantization of the fp32 graph, calibrated on the dataset's val crops."""
    from onnxruntime.quantization import QuantFormat, QuantType, quantize_static
    from onnxruntime.quantization.shape_inference import quant_pre_process
    prepared = out / f"_pre_{name}.onnx"
    quant_pre_process(str(f32), str(prepared), skip_symbolic_shape=True)
    quantize_static(str(prepared), str(path), calibration_reader(Path(data) / "val"), quant_format=QuantFormat.QDQ,
                    per_channel=True, activation_type=QuantType.QUInt8, weight_type=QuantType.QInt8)
    prepared.unlink(missing_ok=True)


def by_x(detections):
    """Detections in order of their x, to compare two lists."""
    return detections[np.argsort(detections[:, 0])]


def check_parity(model, files):
    """ONNX fp32 against PyTorch on a real 1280 x 720 KovOBS frame (random noise is a poor test: it drives the
    activations far outside anything a frame produces, where tiny summation-order differences grow), then the u8in,
    embed and fp16 graphs against the fp32 one."""
    import bench
    import infer
    rgb, fixed = bench.sample()
    frame = net.prepare(torch.from_numpy(rgb)[None], torch.from_numpy(fixed)[None])
    with torch.no_grad():
        torch_score, torch_reg = Exported(model.eval()).eval()(frame)
    score, reg = cpu_session(files["fp32"]).run(None, {"x": frame.numpy()})
    score_diff, reg_diff = np.abs(score - torch_score.numpy()).max(), np.abs(reg - torch_reg.numpy()).max()
    print(f"parity fp32 on a real frame: max |score| diff {score_diff:.2e}, max |reg| diff {reg_diff:.2e}")
    if reg_diff > SAME_REG:
        raise SystemExit("the ONNX export does not match PyTorch")
    check_u8in(files["u8in"], files["fp32"])
    (dets,) = cpu_session(files["embed"]).run(None, {"rgb": rgb[None], "fixed": fixed[None].astype(np.uint8)})
    want = infer.decode_np(score, reg, CHECK_THRESHOLD)
    got = dets[0][dets[0][:, 4] > CHECK_THRESHOLD]
    if len(got) != len(want) or (len(got) and np.abs(by_x(got) - by_x(want)).max() > SAME_REG):
        raise SystemExit("the embed export does not match")
    print(f"embed graph: the same {len(got)} detections over 0.3")
    if files["fp16"].exists():
        _, half_reg = cpu_session(files["fp16"]).run(None, {"x": frame.numpy()})
        at_targets = np.abs(half_reg - reg)[:, :, score[0, 0] > CHECK_THRESHOLD]
        if at_targets.size:
            print(f"fp16 against fp32 on a real frame: max |reg| diff where a target is {at_targets.max():.3f}")
        else:
            print("fp16 against fp32: no target over 0.3 on the frame to compare")


def main():
    """Writes every export of the checkpoint into --out, checks them against PyTorch and each other, prints their
    sizes and writes the settings file; with --u8in, writes and checks the _u8in file alone."""
    parser = argparse.ArgumentParser()
    parser.add_argument("checkpoint")
    parser.add_argument("--out", default="python/model/exports")
    parser.add_argument("--data", default=str(folder("data") / "vod_model" / "data"))
    parser.add_argument("--u8in", action="store_true", help="only the _u8in file (the fp32 file must be beside it)")
    parser.add_argument("--val", action="append", help="a dataset whose val split fits the score map (repeat it) "
                        "[calibrate.VAL]; the threshold is picked on --data's val split")
    args = parser.parse_args()
    saved = torch.load(args.checkpoint, map_location="cpu", weights_only=False)
    config = saved["config"]
    model = net.build(config)
    model.load_state_dict(saved["model"])
    model.eval()
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    name = config["name"]
    files = {kind: out / f"detector_{name}_{kind}.onnx" for kind in ("fp32", "fp16", "u8in", "embed", "int8")}
    if args.u8in:
        export_u8in(model, files["u8in"])
        check_u8in(files["u8in"], files["fp32"] if files["fp32"].exists()
                   else Path(__file__).parent / "exports" / files["fp32"].name)
        print(f"{files['u8in']}: {files['u8in'].stat().st_size / KB:.1f} KB")
        return
    frame = torch.zeros(1, 4, HEIGHT_PX, WIDTH_PX)
    export_fp32(model, saved, files["fp32"], frame)
    export_fp16(model, files["fp16"], frame)
    export_u8in(model, files["u8in"])                       # uint8 in: rgb (N, H, W, 3) and fixed (N, H, W)
    export_embed(model, files["embed"])
    export_int8(files["fp32"], files["int8"], out, name, args.data)
    check_parity(model, files)
    for path in (files["fp32"], files["fp16"], files["u8in"], files["embed"], files["int8"]):
        if path.exists():
            print(f"{path}: {path.stat().st_size / KB:.1f} KB")
    settings_file(files["u8in"], args)


def settings_file(u8, args):
    """The model's settings file, detector_<name>.json beside the exports: its scores put on the reference model's
    scale and its threshold there (calibrate.py), written only when the file does not exist yet. The numbers behind
    it go to python/model/reports/calibration_<name>.json."""
    import calibrate
    try:
        settings, report = calibrate.calibrate(u8, tuple(args.val or calibrate.VAL), args.data)
    except SystemExit as error:                           # no validation crops on this computer
        print(f"no settings file: {error}")
        return
    print(f"settings: {json.dumps(settings)}")
    calibrate.write_settings(settings, u8.parent)
    report_path = Path(__file__).resolve().parent / "reports" / f"calibration_{settings['name']}.json"
    report_path.parent.mkdir(exist_ok=True)
    json.dump(dict(settings=settings, **report), open(report_path, "w"), indent=1)
    print(f"{report_path}: the calibration's numbers")


if __name__ == "__main__":
    main()
