# Native detector test (Rust)

A small Rust program that runs the KovOBS target detector (`python/model/exports/detector_small_*.onnx`) without
Python. It answers one question: can the model be embedded in a Tauri 2 / Rust 2024 desktop app, with no heavy
runtime to ship?

It has three paths:

- **tract** (`tract-onnx`, pure Rust, no DLL). The default and the preferred path.
- **ort** (ONNX Runtime bindings), behind the cargo feature `ort`. A comparison.
- **wasm** (`wasm/`): the tract path built for `wasm32-unknown-unknown` and run under Node. A comparison.

The test frame is the 1280 x 720 KovOBS frame from `test_out/vod_model/` (`bench_frame_rgb.bin`,
`bench_frame_fixed.bin`). The program builds the 4-channel NCHW input, runs the model, decodes the score and reg
maps (score above 0.3), and compares with the detections that Python ONNX Runtime gave for the fp32 file
(`bench_expected.json`: centres within 0.05 px, scores within 0.01).

## Files

| File | Role |
| --- | --- |
| `src/main.rs` | Arguments, input tensor, timing loop, comparison, memory and CPU numbers. |
| `src/backend.rs` | One small `Detector` trait with a tract and an ort implementation. Also the tract profiler and the frame-parallel bench. |
| `src/decode.rs` | Turns the score and reg maps into (cx, cy, w, h, score). |
| `src/os.rs` | Peak working set and CPU time from the Windows API (`GetProcessMemoryInfo`, `GetProcessTimes`). |
| `expected_int8_ort.json` | What Python ONNX Runtime gives for the int8 file. The int8 file cannot match the fp32 numbers, so it has its own reference. |
| `wasm/` | A tiny `cdylib` that loads the ONNX bytes and runs them with tract, and `run.mjs`, which loads it in Node. Own `Cargo.toml` (not part of the main crate). |

## Commands

Run from `python/model/rust`. The first build takes about 5 minutes (tract is large).

```bash
cargo build --release
./target/release/kovobs-detect.exe ../exports/detector_small_fp32.onnx
```

Options (all optional, before the model path):

```text
--threads N          tract: rayon pool for matmul / element-wise ops (default 1); ort: intra-op threads (0 = auto)
--runs N --warmup N  timed runs (default 30) and warm-ups (default 3)
--expected FILE      compare with another reference (for example expected_int8_ort.json)
--profile            tract only: time per node, grouped by op
--parallel N         tract only: N worker threads, each running whole frames; prints frames per second
--backend tract|ort  default tract
```

ONNX Runtime comparison (downloads a prebuilt static ONNX Runtime from pyke's CDN at build time):

```bash
cargo build --release --features ort
./target/release/kovobs-detect.exe --backend ort --threads 1 ../exports/detector_small_fp32.onnx
```

The size KovOBS would see (Cargo's release defaults, no LTO, 16 codegen units):

```bash
cargo build --profile kovobs
```

wasm (needs `rustup target add wasm32-unknown-unknown` and Node or Bun):

```bash
cd wasm
cargo build --release --target wasm32-unknown-unknown
node run.mjs                                   # no SIMD
RUSTFLAGS="-C target-feature=+simd128" CARGO_TARGET_DIR=target-simd cargo build --release --target wasm32-unknown-unknown
node run.mjs ../../exports/detector_small_fp32.onnx target-simd/wasm32-unknown-unknown/release/kovobs_detect_wasm.wasm
```

## What was measured

Windows 11, 16 logical cores, release build, 30 runs after 3 warm-ups, 1280 x 720 input. The timings cover the
model run and the decode, plus the copy of the input into the tensor. They do not cover turning RGB bytes into the
float tensor (a few ms). Load time is reading the file, optimizing and making the model runnable.

Memory is the process **peak working set**, read inside the program with `GetProcessMemoryInfo` after the runs.

The machine was shared with other work, so some runs came out 2x slower. The table shows the best of three
processes per row; the timings inside a quiet run are steady (p90 within 2% of the median).

### tract (pure Rust)

| File | Load | Median | p90 | Peak memory | CPU cores busy | Output |
| --- | --- | --- | --- | --- | --- | --- |
| fp32, 1 thread | 17 ms | 95.7 ms | 97.0 ms | 74 MB | 1.0 | matches (0.0004 px, score 0.0002) |
| fp32, `--threads 8` | 16 ms | 91 ms | 118 ms | 73 MB | 1.5 to 1.9 | same |
| int8 (QDQ) | 21 ms | 150 ms | 152 ms | 73 MB | 1.0 | matches ONNX Runtime's int8 (0.0002 px); 0.25 px and 0.02 score off the fp32 result, as quantization does |
| fp16 | 24 ms | 1,630 ms | 1,650 ms | 52 MB | 0.9 | matches fp32 (0.001 px) |
| fp32, 4 frames at once | | 29 ms per frame (34 fps) | | 199 MB | 3.8 | |
| fp32, 8 frames at once | | 21 ms per frame (43 to 49 fps) | | 340 MB | 7.4 | |
| fp32, 16 frames at once | | 18 ms per frame (56 fps) | | 632 MB | 14.4 | |

Binary size: **24.5 MB** with LTO and strip (tract only; `default-features = false` saved 2 MB). 25.7 MB with
KovOBS's own release profile (`--profile kovobs`), at the same speed. No DLL is needed.

Threads: tract runs on one thread by default. `--threads N` (the `multithread-mm` feature of `tract-linalg`, set
through `tract_linalg::multithread::set_default_executor`) only parallelizes matrix multiplies and element-wise ops.
The profile of one frame is: depthwise convolution 50 ms (not parallel, scalar), matmul 29 ms, matmul packing
17 ms, padding 3 ms. So more threads inside one frame gain very little. Running whole frames on several threads
scales well, at about 35 MB of memory per worker.

### ONNX Runtime through the `ort` crate (2.0.0-rc.13, ONNX Runtime 1.28, static)

| File | Load | Median | p90 | Peak memory | CPU cores busy | Output |
| --- | --- | --- | --- | --- | --- | --- |
| fp32, 1 thread | 37 ms | 7.6 ms | 7.7 ms | 109 MB | 0.95 | matches |
| fp32, 4 threads | 36 ms | 5.0 ms | 6.3 ms | 113 MB | 1.9 | matches |
| fp32, auto threads | 40 ms | 4.1 ms | 4.5 ms | 109 MB | 3.5 | matches |
| int8, 1 thread | 40 ms | 10.7 ms | 11.0 ms | 104 MB | 1.0 | matches ONNX Runtime's int8 |
| fp16, 1 thread | 46 ms | 7.6 ms | 7.8 ms | 109 MB | 1.0 | matches fp32 (0.001 px) |

Binary size: **45.6 MB** with tract and ort together, so ONNX Runtime adds about 22 MB. The binary runs on its own:
the build copies `DirectML.dll` next to it, but the program ran without it. The build downloads a 341 MB static
`onnxruntime.lib` once, into `%LOCALAPPDATA%\ort.pyke.io`.

For reference, Python ONNX Runtime 1.30 on the same machine (`python/model/bench.py`, model alone, contiguous input):
fp32 6.1 ms (1 thread) and 1.8 ms (4 threads); int8 9.9 ms and 4.1 ms. It loads all three files, fp16 included
(checked again on 2026-10-01; one run of this test reported a Cast type error that did not reproduce).

### tract in WebAssembly (Node 24, V8)

The exported function loads the model, optimizes it and runs it on every call, so these numbers include about
20 ms of load. The scores match (0.638 and 0.927).

| Build | Size | gzip | Per call |
| --- | --- | --- | --- |
| no SIMD | 9.4 MB | 2.4 MB | 290 ms |
| `+simd128` | 8.8 MB | 2.4 MB | 115 ms |
| no SIMD, int8 file | | | 365 ms |
| no SIMD, Bun (JavaScriptCore) | | | 445 ms |

The wasm memory grew to 61 MB. The module was built with tract's `getrandom-js` feature, which adds
five `wasm-bindgen` imports; `run.mjs` stubs them. There is no thread support in this build.

## What did not work, and what to know

- **tract is slow on this model.** 96 ms per frame against 8 ms in ONNX Runtime on one thread, 12 times slower. The
  model is built from depthwise 3x3 plus 1x1 convolutions, and tract's depthwise kernel is a scalar loop. Building
  with `-C target-cpu=x86-64-v3` (AVX2) gave only 10% (95 ms). The same code in wasm with SIMD ran 2.5 times
  faster than wasm without SIMD, so there may be room if the depthwise kernel is improved or the model uses dense
  convolutions.
- **fp16 is useless in tract on x86.** It runs, with the right result, at 1.6 s per frame (software half
  floats). It also needed `with_ignore_value_info(true)`: the fp16 file keeps symbolic `h`, `w` in its inner value
  info, and tract fails with "Impossible to unify Val(720) with Sym(h)" without it.
- **int8 does not help.** It is slower than fp32 in both tract (150 ms) and ONNX Runtime (10.7 ms), and it moves
  the detections by 0.25 px.
- Intra-frame threads in tract barely help (see above).

Use the fp32 file.

## Embedding in KovOBS

KovOBS (`D:\Projects\KovOBS`, Tauri 2, edition 2024) already has what the pipeline needs: `ffmpeg-sidecar` to run
ffmpeg, `rayon` for parallel work and `tokio` for tasks. It ships Cargo's release defaults (no LTO). Nothing was
changed there.

A way to wire it up:

1. Keep the model in the binary with `include_bytes!` (fp32 is 138 KB), or as a Tauri resource.
2. Build the runnable model once and keep it in Tauri's managed state. In tract, `Arc<TypedRunnableModel>::run`
   takes `&self`, so every rayon worker can share one plan. In ort, give each worker its own `Session` or wrap one
   in a pool.
3. A command such as `detect_video(path)` runs inside `tokio::task::spawn_blocking`:
   - Sample a few dozen frames to compute the fixed map once per video. This is `fixed_map()` in `python/review.py`
     (pixels that stand out from the wall in 80% of the sampled frames). It is the 4th input channel, so it has to
     be ported to Rust.
   - Stream frames from ffmpeg-sidecar: `FfmpegCommand::new().input(path).rawvideo().spawn()?.iter()?`. The
     `rawvideo()` helper outputs rgb24, and each `OutputVideoFrame` carries `data: Vec<u8>`. Add a scale filter if
     the recording is not 1280 x 720; the input height and width must be multiples of 16 (a new plan per size).
   - Turn a batch of frames into float tensors, run them on a rayon pool, and send progress to the frontend with
     `app.emit`.
4. Return the detections (cx, cy, w, h, score per frame) to the existing flick matching code.

Throughput for a 60 s recording at 60 fps (3,600 frames), if every frame is analysed:

| Path | Time |
| --- | --- |
| tract, 1 thread | about 6 min |
| tract, 16 workers | about 65 s |
| ort, 4 threads | about 18 s |
| ort, auto threads | about 15 s |

If the review does not need every frame (the Python path works from decoded frames of the kills), the gap matters
less.

Assessment:

- **No heavyweight runtime is required to get correct results.** `tract-onnx` is pure Rust, loads all three files,
  and reproduces the Python fp32 detections to 0.0004 px. It adds about 25 MB to the exe and needs no DLL. The load
  takes about 20 ms and one run needs about 75 MB.
- **Speed is the cost.** For offline VOD review, tract with all cores is workable (about a minute per 60 s run if
  every frame is analysed). If it must feel instant, use `ort`.
- **`ort` is the fast path and still a single exe.** The default Windows build links ONNX Runtime statically, adds
  about 22 MB, needs no runtime DLL, and is 12 times faster per frame on one thread. The costs are a 341 MB
  download at build time (cached after that) and an `rc` crate version.
- A good arrangement is the `Detector` trait used here: `ort` as the default, `tract` as the fallback behind a
  cargo feature. Both give the same detections.
- The wasm build runs in a webview as well (115 ms with SIMD), so the same model could run in the frontend if
  Rust is not wanted for this, but it would need the frames sent to the webview, which is slower overall than
  doing it in the backend.
