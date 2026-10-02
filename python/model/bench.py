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
VIDEO = r"E:\OBS\KovOBS\1w4ts Voltaic\1w4ts Voltaic - 143 - 2026.09.30-04.55.23.mp4"
SAMPLE = Path("test_out/vod_model/bench_frame.npz")


def sample():
    """A real frame and its fixed map, cached."""
    if not SAMPLE.exists():
        import build_data
        import review
        yuv = build_data.keyframes(VIDEO, "yuv420p")
        rgb = build_data.keyframes(VIDEO, "rgb24")
        np.savez(SAMPLE, rgb=np.frombuffer(rgb[10], np.uint8).reshape(720, 1280, 3),
                 fixed=review.fixed_map(yuv).astype(np.uint8))
    z = np.load(SAMPLE)
    for name in ("rgb", "fixed"):                       # raw copies for the browser benchmark
        raw = SAMPLE.with_name(f"bench_frame_{name}.bin")
        if not raw.exists():
            raw.write_bytes(np.ascontiguousarray(z[name]).tobytes())
    return z["rgb"], z["fixed"]


def one(path, threads):
    """Runs in a fresh process: everything about one ONNX file at one thread count."""
    import psutil
    import onnxruntime as ort
    proc = psutil.Process()
    rgb, fixed = sample()
    base = proc.memory_info().rss
    t = time.perf_counter()
    so = ort.SessionOptions()
    so.intra_op_num_threads = threads
    sess = ort.InferenceSession(path, so, providers=["CPUExecutionProvider"])
    load = time.perf_counter() - t
    if sess.get_inputs()[0].name == "rgb":                 # a _u8in export takes the frame's bytes as they are
        feed = lambda: {"rgb": rgb[None], "fixed": fixed[None]}
    else:                                                   # the float input, made the way infer.OnnxDetector does
        def feed():
            x = np.empty((1, 4) + fixed.shape, np.float32)
            np.multiply(rgb.transpose(2, 0, 1), np.float32(1 / 255), out=x[0, :3], dtype=np.float32)
            x[0, 3] = fixed
            return {"x": x}
    f = feed()
    for _ in range(5):
        sess.run(None, f)
    times, frames = [], []
    c0, w0 = proc.cpu_times(), time.perf_counter()
    for _ in range(30):
        t = time.perf_counter()
        sess.run(None, f)
        times.append(time.perf_counter() - t)
    for _ in range(30):                                     # the whole frame: input conversion and model
        t = time.perf_counter()
        sess.run(None, feed())
        frames.append(time.perf_counter() - t)
    c1, w1 = proc.cpu_times(), time.perf_counter()
    cores = ((c1.user + c1.system) - (c0.user + c0.system)) / (w1 - w0)
    mi = proc.memory_info()
    peak = getattr(mi, "peak_wset", mi.rss)
    return dict(file=Path(path).name, kb=round(os.path.getsize(path) / 1024, 1), threads=threads,
                load_ms=round(1000 * load, 1), median_ms=round(1000 * float(np.median(times)), 2),
                p90_ms=round(1000 * float(np.percentile(times, 90)), 2), fps=round(1 / float(np.median(times)), 1),
                frame_ms=round(1000 * float(np.median(frames)), 2),
                peak_mb=round(peak / 2 ** 20, 1), model_mb=round((mi.rss - base) / 2 ** 20, 1), cores=round(cores, 2))


def gpu(path):
    import torch
    import net
    ck = torch.load(path, map_location="cpu", weights_only=False)
    m = net.build(ck["config"]).cuda().eval()
    m.load_state_dict(ck["model"])
    rgb, fixed = sample()
    res = dict(file=str(path), params=sum(p.numel() for p in m.parameters()))
    for bs in (1, 16):
        r = torch.from_numpy(np.repeat(rgb[None], bs, 0)).cuda()
        f = torch.from_numpy(fixed).cuda()[None].expand(bs, -1, -1)
        x = net.prepare(r, f)
        torch.cuda.reset_peak_memory_stats()
        with torch.no_grad(), torch.autocast("cuda", dtype=torch.bfloat16):
            for _ in range(10):
                m(x)
            torch.cuda.synchronize()
            s, e = torch.cuda.Event(enable_timing=True), torch.cuda.Event(enable_timing=True)
            s.record()
            for _ in range(50):
                m(x)
            e.record()
            torch.cuda.synchronize()
        res[f"batch{bs}_ms_per_frame"] = round(s.elapsed_time(e) / 50 / bs, 3)
        res[f"batch{bs}_peak_vram_mb"] = round(torch.cuda.max_memory_allocated() / 2 ** 20, 1)
    return res


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--onnx", nargs="*", default=[])
    ap.add_argument("--pt", nargs="*", default=[])
    ap.add_argument("--threads", nargs="*", type=int, default=[1, 4, 8])
    ap.add_argument("--one", nargs=2)
    a = ap.parse_args()
    if a.one:
        print(json.dumps(one(a.one[0], int(a.one[1]))))
        return
    sample()
    out = dict(cpu=[], gpu=[], machine=dict(cpu="AMD Ryzen 7 9800X3D (8 cores, 16 threads)", gpu="RTX 5070 Ti 16 GB"))
    for p in a.onnx:
        for th in a.threads:
            r = subprocess.run([sys.executable, __file__, "--one", p, str(th)], capture_output=True, text=True)
            row = json.loads(r.stdout.strip().splitlines()[-1]) if r.returncode == 0 else dict(file=p, threads=th,
                                                                                                    error=r.stderr[-300:])
            out["cpu"].append(row)
            print(row, flush=True)
    for p in a.pt:
        row = gpu(p)
        out["gpu"].append(row)
        print(row, flush=True)
    json.dump(out, open("test_out/vod_model/bench.json", "w"), indent=1)


if __name__ == "__main__":
    main()
