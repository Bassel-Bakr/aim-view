"""Deployment benchmarks (REPRODUCE.md step 3c) on a real 1280 x 720 KovOBS frame:
  - ONNX Runtime CPU per model file and thread count, each in a fresh process: session load time, latency per frame
    (median, p90 of 30 runs after warm-up), the whole frame with the input conversion (frame_ms), throughput, peak
    memory (working set), CPU cores used on average;
  - PyTorch on the GPU per checkpoint: latency per frame at batch 1 and 16, peak VRAM.
Writes test_out/vod_model/bench.json.
Usage: python python/model/bench.py --onnx python/model/exports/*.onnx --pt test_out/vod_model/runs/*/best.pt
"""
import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
VIDEO = "1w4ts Voltaic/1w4ts Voltaic - 143 - 2026.09.30-04.55.23.mp4"     # in the recordings' folder
SAMPLE = Path("test_out/vod_model/bench_frame.npz")
SAMPLE_KEY_FRAME = 10           # the key frame the sample is
WARM_UP, TIMED = 5, 30          # CPU: runs before timing, runs timed
GPU_WARM_UP, GPU_TIMED = 10, 50
GPU_BATCHES = (1, 16)
MB = 2 ** 20
ERROR_CHARS = 300               # the end of a failed run's stderr kept


def sample():
    """A real frame and its fixed map, cached."""
    if not SAMPLE.exists():
        import build_data
        import local_config
        import old_review
        video = str(local_config.required(local_config.folder("vods"), "recordings' folder (vods)") / VIDEO)
        yuv = build_data.keyframes(video, "yuv420p")
        rgb = build_data.keyframes(video, "rgb24")
        np.savez(SAMPLE, rgb=np.frombuffer(rgb[SAMPLE_KEY_FRAME], np.uint8).reshape(720, 1280, 3),
                 fixed=old_review.fixed_map(yuv).astype(np.uint8))
    saved = np.load(SAMPLE)
    for name in ("rgb", "fixed"):                       # raw copies for the browser benchmark
        raw = SAMPLE.with_name(f"bench_frame_{name}.bin")
        if not raw.exists():
            raw.write_bytes(np.ascontiguousarray(saved[name]).tobytes())
    return saved["rgb"], saved["fixed"]


def feeder(session, rgb, fixed):
    """The model's input for the frame: the frame's bytes as they are for a _u8in export, else the float input made
    the way infer.OnnxDetector does (each call makes it again)."""
    if session.get_inputs()[0].name == "rgb":
        return lambda: {"rgb": rgb[None], "fixed": fixed[None]}

    def feed():
        frame = np.empty((1, 4) + fixed.shape, np.float32)
        np.multiply(rgb.transpose(2, 0, 1), np.float32(1 / 255), out=frame[0, :3], dtype=np.float32)
        frame[0, 3] = fixed
        return {"x": frame}
    return feed


def one(path, threads):
    """Runs in a fresh process: everything about one ONNX file at one thread count."""
    import psutil
    import onnxruntime as ort
    process = psutil.Process()
    rgb, fixed = sample()
    base = process.memory_info().rss
    started = time.perf_counter()
    options = ort.SessionOptions()
    options.intra_op_num_threads = threads
    session = ort.InferenceSession(path, options, providers=["CPUExecutionProvider"])
    load = time.perf_counter() - started
    feed = feeder(session, rgb, fixed)
    inputs = feed()
    for _ in range(WARM_UP):
        session.run(None, inputs)
    times, frames = [], []
    cpu_before, wall_before = process.cpu_times(), time.perf_counter()
    for _ in range(TIMED):
        started = time.perf_counter()
        session.run(None, inputs)
        times.append(time.perf_counter() - started)
    for _ in range(TIMED):                                  # the whole frame: input conversion and model
        started = time.perf_counter()
        session.run(None, feed())
        frames.append(time.perf_counter() - started)
    cpu_after, wall_after = process.cpu_times(), time.perf_counter()
    cores = ((cpu_after.user + cpu_after.system) - (cpu_before.user + cpu_before.system)) / (wall_after - wall_before)
    memory = process.memory_info()
    peak = getattr(memory, "peak_wset", memory.rss)
    return dict(file=Path(path).name, kb=round(os.path.getsize(path) / 1024, 1), threads=threads,
                load_ms=round(1000 * load, 1), median_ms=round(1000 * float(np.median(times)), 2),
                p90_ms=round(1000 * float(np.percentile(times, 90)), 2), fps=round(1 / float(np.median(times)), 1),
                frame_ms=round(1000 * float(np.median(frames)), 2),
                peak_mb=round(peak / MB, 1), model_mb=round((memory.rss - base) / MB, 1), cores=round(cores, 2))


def gpu(path):
    import torch
    import net
    saved = torch.load(path, map_location="cpu", weights_only=False)
    model = net.build(saved["config"]).cuda().eval()
    model.load_state_dict(saved["model"])
    rgb, fixed = sample()
    result = dict(file=str(path), params=sum(parameter.numel() for parameter in model.parameters()))
    for batch in GPU_BATCHES:
        frames = torch.from_numpy(np.repeat(rgb[None], batch, 0)).cuda()
        fixed_maps = torch.from_numpy(fixed).cuda()[None].expand(batch, -1, -1)
        inputs = net.prepare(frames, fixed_maps)
        torch.cuda.reset_peak_memory_stats()
        with torch.no_grad(), torch.autocast("cuda", dtype=torch.bfloat16):
            for _ in range(GPU_WARM_UP):
                model(inputs)
            torch.cuda.synchronize()
            start, end = torch.cuda.Event(enable_timing=True), torch.cuda.Event(enable_timing=True)
            start.record()
            for _ in range(GPU_TIMED):
                model(inputs)
            end.record()
            torch.cuda.synchronize()
        result[f"batch{batch}_ms_per_frame"] = round(start.elapsed_time(end) / GPU_TIMED / batch, 3)
        result[f"batch{batch}_peak_vram_mb"] = round(torch.cuda.max_memory_allocated() / MB, 1)
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--onnx", nargs="*", default=[])
    parser.add_argument("--pt", nargs="*", default=[])
    parser.add_argument("--threads", nargs="*", type=int, default=[1, 4, 8])
    parser.add_argument("--one", nargs=2)
    args = parser.parse_args()
    if args.one:
        print(json.dumps(one(args.one[0], int(args.one[1]))))
        return
    sample()
    out = dict(cpu=[], gpu=[], machine=dict(cpu="AMD Ryzen 7 9800X3D (8 cores, 16 threads)", gpu="RTX 5070 Ti 16 GB"))
    for path in args.onnx:
        for threads in args.threads:
            run = subprocess.run([sys.executable, __file__, "--one", path, str(threads)], capture_output=True, text=True)
            row = (json.loads(run.stdout.strip().splitlines()[-1]) if run.returncode == 0
                   else dict(file=path, threads=threads, error=run.stderr[-ERROR_CHARS:]))
            out["cpu"].append(row)
            print(row, flush=True)
    for path in args.pt:
        row = gpu(path)
        out["gpu"].append(row)
        print(row, flush=True)
    json.dump(out, open("test_out/vod_model/bench.json", "w"), indent=1)


if __name__ == "__main__":
    main()
