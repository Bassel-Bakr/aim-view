# Target detector: status

Last updated 2026-10-01. How to rebuild every number here: [REPRODUCE.md](REPRODUCE.md).

## Objective

A small model, embedded in software, that finds the targets in a KovaaK's static-clicking recording made by KovOBS.
It replaces the hand-written detector in `python/review.py`, which fails when targets are not dark, when the theme
changes, and when a target sits under the crosshair (Pokeball scenarios).

- **Input:** one 1280 × 720 RGB frame, plus a "fixed map": 1 where the screen stays put over the run (crosshair, HUD),
  0 elsewhere. The fixed map comes from the run's key frames (`review.fixed_map`).
- **Output:** one box per target: centre x, centre y, width, height (frame pixels) and a score from 0 to 1.
- **Where it plugs in:** `python/review.py` links the boxes into tracks, matches the kills with the stats file and
  measures each flick. The review app (`bun run server`, or the desktop app) uses the model on its own when the export exists.
- **Not a chatbot.** The model only detects. The review's text, numbers and issue rules stay ordinary code.

Boxes, not a fixed "static" label, so tracking scenarios can be added later by fine-tuning on more data, without
starting over.

## Architecture

A CenterNet-style detector ("objects as points"), written from scratch in PyTorch (`net.py`):

- depthwise-separable convolutions, a small feature pyramid (strides 4, 8 and 16), nearest-neighbour upsampling;
- one output map at stride 4: a centre heatmap, the centre's offset within its cell, and log width and log height;
- 4 input channels: RGB (0 to 1) and the fixed map. The fixed map tells the crosshair and HUD apart from targets, so
  colour does not have to.

The exported ONNX graph also does the decoding that is awkward in other languages: the sigmoid and the 3 × 3 peak
search. A caller only scans the score map for cells over the threshold. Two more exports go further. `_u8in` also
does the pre-processing, so a caller passes the decoder's raw bytes, and takes several frames in one call (a free
batch axis: the browser's GPU reviews 4 at a time by default, the same boxes as one at a time). `_embed` does that and also picks the 100 best
peaks, so a caller reads 100 rows of (cx, cy, w, h, score) and keeps those over the threshold: no pre- or
post-processing left to port. It gives exactly the same boxes as the plain file.

| Variant | Widths | Parameters | Compute at 1280 × 720 |
| --- | --- | --- | --- |
| tiny | 8, 16, 24, 32 | 6,829 | 0.16 GMAC |
| small | 16, 32, 48, 64 | 32,037 | 0.59 GMAC |
| full | 24, 48, 64, 96 | 80,765 | 1.37 GMAC |

Why this and not an off-the-shelf detector: Ultralytics YOLO is AGPL, which rules it out for embedding in KovOBS.
YOLOX-Nano is Apache-2.0 but needs about 6 GFLOPs at 720p, ten times small. Targets are simple shapes, so a tiny
network trained on the right data does the job. Everything here is our own code under the project's licence. The
runtimes are MIT (ONNX Runtime, onnxruntime-web) or MIT/Apache-2.0 (tract).

## Dataset

Real KovOBS frames, labelled automatically. No frame was labelled by hand.

- **Source:** the user's KovOBS library, `E:\OBS\KovOBS`. A scenario counts as static when every target's MaxSpeed
  is 0 in its `.sce` file. That gives 453 static scenarios; the 4 newest recordings of each make 530 VODs.
- **Labels:** the hand-written detector in `review.py`, run on key frames only (about one every 2 seconds). A VOD is
  kept only when its labels look trustworthy: targets found in most frames, with a steady count (steadiness at least
  0.7). Frames whose target count is off are skipped. 419 of 530 VODs were kept.
- **Crops:** 256 × 256 crops round the crosshair, round a target and at random, with the fixed map and a target mask.
- **Splits:** by scenario folder, so no scenario is in two splits (checked by `validate_data.py` and the tests). A
  stable hash puts 10% of folders in val and 10% in test. The folders of the four VODs used for the end-to-end check
  (1w4ts Voltaic, 10 Sphere Hipfire Extra Small, both Pokeball LG56 folders) are always in test.

| Split | VODs kept | Scenarios | Crops | Boxes | Box size (px, p5 / median / p95) |
| --- | --- | --- | --- | --- | --- |
| train | 320 of 400 | 168 | 21,719 | 38,021 | 5 / 8 / 32 |
| val | 43 of 58 | 22 | 3,109 | 4,083 | 5 / 9 / 24 |
| test | 56 of 72 | 28 | 3,830 | 6,138 | 3 / 7 / 24 |

**Synthetic augmentation**, all on the GPU during training (`train.py`):
- flips and quarter turns;
- a new theme: the wall moved to a random colour, keeping its texture;
- targets repainted in a random colour that still stands out, with their anti-aliased edges blended again;
- added wall texture, blur and noise;
- synthetic crosshairs (dot, plus or ring, any colour), drawn into the image and the fixed map. In v2, 70% of them sit
  on a target, up to 0.6 target sizes off its centre, as when a player holds slightly off.

So the model learns from real frames and real automatic labels, and sees colours and crosshairs that the library does
not have.

## Training

AdamW, cosine schedule with warm-up, bf16 on the GPU, batch 64, 20 epochs, fixed seed. Focal loss on the heatmap
(Gaussian peaks), L1 on offset and size. The checkpoint with the best validation F1 is kept. About 7 minutes for
small on an RTX 5070 Ti. Configs are in `configs/` (`tiny`, `small`, `full`, and the same three with `_v2`).

v2 changed only the crosshair augmentation (see above), after the first models lost targets held under the crosshair
in Pokeball runs.

## Evaluation

### Crops of held-out scenarios

Measured against the automatic labels, so these scores are agreement with the hand-written detector on frames where
it was reliable, not ground truth. The threshold is chosen on val (0.4 for v1, 0.3 for v2), never on test.
"Recoloured" repaints every test crop's wall and targets (fixed seed): does the model ignore colour?

| Model | Precision | Recall | F1 | Centre error, median / p90 (px) | Recoloured F1 |
| --- | --- | --- | --- | --- | --- |
| tiny | 0.955 | 0.935 | 0.945 | 0.45 / 1.22 | 0.955 |
| small | 0.969 | 0.934 | 0.951 | 0.30 / 0.85 | 0.967 |
| full | 0.974 | 0.941 | 0.957 | 0.24 / 0.71 | 0.973 |
| tiny_v2 | 0.931 | 0.937 | 0.934 | 0.49 / 1.40 | 0.936 |
| **small_v2** | 0.954 | 0.956 | **0.955** | 0.32 / 1.02 | 0.954 |
| full_v2 | 0.960 | 0.961 | 0.961 | 0.27 / 0.84 | 0.970 |

The exports keep these scores: small_v2 fp32, fp16, u8in and embed give the same F1 (0.955); int8 gives 0.950.
On crops, the v2 recipe helps full, costs small a little precision for recall, and hurts tiny, which is too small
to learn targets under crosshairs as well.

Per scenario, small_v2 scores 0.96 to 0.99 on the end-to-end scenarios (1w4ts Voltaic 0.96, 10 Sphere 0.98,
Pokeball 5 0.96, Pokeball 1 0.99). The weakest test scenarios are covered under "Known limitations".

### Whole VODs against the stats file

The independent check. Each held-out VOD goes through the whole review, once with the hand-written detector and once
with the model. The stats file records when each kill happened, which nothing in the pipeline can fake.
- **Matched:** stats kills found as a tracked target ending at the crosshair.
- **Confirmed:** matched kills whose track ends within 2 frames of the stats kill, under 0.6° from the crosshair.
- **Flicks measured:** kills whose target was tracked from the previous kill.
- **Time:** the whole review, on this PC.

| VOD | Hand-written | small (v1) | small_v2 |
| --- | --- | --- | --- |
| 1w4ts Voltaic, 143 kills | 143 matched, 143 confirmed, 67 flicks, 26.8 s | 142, 142, 142 flicks, 19.1 s | 142, 142, 142 flicks, 19.2 s |
| 10 Sphere Hipfire XS, 155 kills | 155, 138, 123 flicks, 16.4 s | 155, 127, 134 flicks, 10.0 s | 155, 127, 136 flicks, 11.3 s |
| Pokeball 5 Sphere XS, 114 kills | 114, 74, 109 flicks, 22.7 s | 113, 48, 108 flicks, 18.4 s | 113, 61, 107 flicks, 18.9 s |
| Pokeball 1 Sphere XS, 84 kills | 82, 41, 75 flicks, 34.2 s | 82, 58, 73 flicks, 25.2 s | 84, 61, 82 flicks, 17.8 s |
| **Total, 496 kills** | 494, 396, 374 flicks | 492, 375, 457 flicks | **494, 391, 467 flicks** |

The other sizes on the v2 recipe, same four VODs: tiny_v2 494 matched, 385 confirmed, 436 flicks; full_v2 493, 382,
467. full_v2 is the best on crops and on Pokeball 5 (70 confirmed) but worse on Pokeball 1 (46). small_v2 has the best
total and runs at twice full's speed, so it stays the pick.

Over these four VODs, small_v2 matches as many kills as the hand-written detector and measures 93 more flicks,
because its tracks rarely break. It works on targets of any colour, which the hand-written detector does not. It
confirms 5 fewer kills overall: 20 more on Pokeball 1, but 11 fewer on 10 Sphere and 13 fewer on Pokeball 5, where an
extra-small target sits under the crosshair at the kill.

Two more held-out VODs, from the scenarios with the lowest crop scores (small_v2 first, then the hand-written
detector):
- **Jumbo1wall9000targets, 299 kills:** 299 and 299 matched; 280 and 293 confirmed; 257 and 158 flicks.
- **ClickTrack Vertical 2t Long, 59 kills:** both match and confirm all 59 and measure 58 flicks.

## Deployment

All numbers on this PC: AMD Ryzen 7 9800X3D (8 cores, 16 threads), RTX 5070 Ti 16 GB, Windows 11. One real
1280 × 720 frame. "Model" is the model alone; "frame" adds turning the frame's bytes into the model's input.

### Files

| File | Size | Use |
| --- | --- | --- |
| `exports/detector_small_v2_fp32.onnx` | 134.5 KB | The plain file: float input, score and box maps out; any ONNX runtime |
| `exports/detector_small_v2_u8in.onnx` | 135.9 KB | Same model, raw uint8 frame in: the fastest whole frame on CPU and WASM |
| `exports/detector_small_v2_embed.onnx` | 143.0 KB | **Recommended for embedding**: raw bytes in, the 100 best boxes out, same speed as u8in |
| `exports/detector_small_v2_fp16.onnx` | 73.3 KB | Half the download, same detections; no faster on CPU |
| `exports/detector_small_v2_int8.onnx` | 99.5 KB | Slower than fp32 on CPU, wrong on WebGPU: not recommended |
| `exports/detector_small_v2.pt` | | PyTorch checkpoint, for batched GPU inference in the review app |

The same set exists for tiny, small (v1) and full (v1: fp32, fp16, int8), and for tiny_v2 and full_v2 (all five).

### Native CPU: ONNX Runtime 1.30 (Python here; the same library is used from Rust through the `ort` crate)

Model alone / whole frame, ms per frame, median of 30:

| File | 1 thread | 4 threads | 8 threads | Load | Peak memory |
| --- | --- | --- | --- | --- | --- |
| tiny fp32 | 3.1 / 5.2 | 1.2 / 3.2 | 1.0 / 3.2 | 11 ms | 140 MB |
| small_v2 fp32 | 6.2 / 8.4 | 1.8 / 4.0 | 1.7 / 5.0 | 12 ms | 141 MB |
| small_v2 fp16 | 6.5 / 8.6 | 1.8 / 4.1 | 1.8 / 4.8 | 15 ms | 141 MB |
| **small_v2 u8in** | 7.7 / 7.7 | 2.3 / 2.3 | 2.3 / 2.4 | 13 ms | 143 MB |
| small_v2 embed | 7.8 / 7.8 | 3.6 / 3.6 | 2.2 / 2.2 | 16 ms | 144 MB |
| small_v2 int8 | 10.1 / 12.3 | 4.2 / 6.6 | 3.6 / 5.7 | 17 ms | 135 MB |
| full fp32 | 13.4 / 15.7 | 4.2 / 6.0 | 3.7 / 7.0 | 13 ms | 192 MB |

At 4 threads, single runs swing between about 2.3 and 3.7 ms for the raw-bytes models (the p90 shows it); 8
threads are steadier. Peak memory is the whole Python process. About 72 MB of it is Python, NumPy and ONNX Runtime
before the model loads; the session adds 53 to 121 MB, almost all of it buffers for a 720p frame's activations, not
weights.
int8 is slower than fp32 at every size: these networks are tiny and memory-bound, and the quantize and dequantize
steps cost more than they save. With the raw-bytes model, 4 threads give 435 frames a second: a 60-second 120 fps
VOD (7,200 frames) takes about 17 s on 4 threads; at 60 fps, half that.

### Native GPU: PyTorch, the review app's path

| Model | Batch 1 | Batch 16 | VRAM at batch 16 |
| --- | --- | --- | --- |
| tiny | 1.4 ms | 0.29 ms / frame | 482 MB |
| small_v2 | 2.5 ms | 0.71 ms / frame | 686 MB |
| full | 3.6 ms | 1.37 ms / frame | 880 MB |

On whole VODs, decoding the video is now the limit, not the model. The review reads frames with `readinto` into its
own buffers and decodes in a thread while the model runs. That cut the model path on the 1w4ts VOD (AV1, 2560 × 1440,
120 fps, 7,933 frames) from 43.4 s to 19.2 s. ffmpeg alone needs 10.2 s to decode it.

Later (2026-10-01) the frames go straight into pinned batch buffers on a thread, and the peaks of a whole batch are
found on the GPU at once: the whole review of that VOD now takes 13.3 s, with every result the same (matched,
confirmed and measured kills on all four end-to-end VODs). Faster options were tried and dropped because they
changed results:
- fp16 or fp32 inference instead of bf16 autocast (the precision the model was trained in): Pokeball 1 confirmed
  55 kills against 61, the same kills matched;
- decoding on the GPU, or sending NV12 and converting the colours on the GPU: 50 confirmed. The model is sensitive
  to small colour differences on an extra-small target half hidden by the crosshair, so it gets ffmpeg's own RGB.
Decoding on the GPU was also slower next to the detector (16.2 s against 11.9 s): the two share the GPU.

### Native in Rust (KovOBS)

`rust/` is a small Rust program (edition 2024, like KovOBS) with one `Detector` trait and two runtimes. Both give
the Python detections to 0.0004 px. Details and the KovOBS wiring plan are in [rust/README.md](rust/README.md).

| Runtime | Load | ms per frame (small fp32) | Peak memory | Adds to the exe |
| --- | --- | --- | --- | --- |
| **ONNX Runtime 1.28 via the `ort` crate**, static | 37 ms | 7.6 (1 thread), 5.0 (4), 4.1 (auto) | 109 MB | about 22 MB, no DLL |
| tract 0.23 (pure Rust) | 17 ms | 96 (1 thread); 18 per frame with 16 frames in parallel | 74 MB | about 25 MB, no DLL |
| tract in WASM (Node), SIMD | about 20 ms | 115 | 61 MB | 8.8 MB file, 2.4 MB gzipped |

- tract is correct but 12 times slower: its depthwise convolution is a scalar loop, and this network is mostly
  depthwise convolutions. Building for AVX2 gained 10%.
- fp16 is no faster anywhere on the CPU, and in tract it is software half floats (1.6 s per frame). int8 is slower
  than fp32 in both runtimes.
- `ort` downloads a 341 MB static ONNX Runtime library once at build time, and its crate version is a release
  candidate (2.0.0-rc.13).
- For a 60-second 60 fps VOD with every frame analysed: about 15 s with `ort`, about 65 s with tract on 16 workers.
- Recommendation: `ort` by default, tract behind a cargo feature as the fallback with no native library. The
  model file can live in the binary (`include_bytes!`, 135 KB). The fixed map (`review.fixed_map`) must be ported to
  Rust; it is a few lines of array code.

### Local web server (B)

`serve.py`: standard library HTTP, ONNX Runtime, no PyTorch, 127.0.0.1 only. `POST /detect` takes a raw RGB frame
and the fixed map and returns the boxes as JSON. With small fp32 and 4 threads, 200 requests of a 3.7 MB frame took
**7.8 ms** per round trip (median; p90 8.3 ms), of which 6.3 ms was detection. `GET /health` names the model.

### Browser (C): onnxruntime-web 1.30

In the Claude desktop app's Chromium pane, 8 WASM threads (cross-origin isolated), whole frame in ms (median of 20),
the JavaScript conversion of the frame's bytes included:

| File | WASM | WebGPU |
| --- | --- | --- |
| tiny fp32 | 8.5 | 12.7 |
| small_v2 fp32 | 14.2 (3.9 of it converting) | 14.0 |
| **small_v2 u8in** | **11.1** | 15.0 to 19.0 |
| small_v2 embed | 11.0 | 14.1 |
| small_v2 fp16 | 14.6 | 14.0 |
| small_v2 int8 | 28.5 | wrong output |
| full fp32 | 24.1 | 14.1 |

- The browser gives the same boxes as Python to 0.01 px, on both backends (fp32, fp16 and u8in); embed finds the same
  two targets.
- WebGPU costs 12 to 19 ms at every model size, so the time is dispatch and read-back overhead, not compute. The first
  WebGPU session also pays about 1.8 s of setup.
- **Download:** the model is 73 to 143 KB. The runtime is the big part: the WASM-only build is 13.9 MB (3.7 MB
  gzipped); the build with WebGPU is 27.6 MB (6.6 MB gzipped).
- Reading back 100 boxes (embed) instead of the two maps saves WebGPU a few ms, but WASM is still faster.
- Recommendation for the browser: WASM with the embed (or u8in) model, about 90 frames a second.

## Every scenario kind: one model (2026-10-02)

The user asked for tracking and switching runs (and dynamic clicking) in the training, so the review can handle every
kind of scenario, and whether that needs a model of its own.

**The data.** `review.scenario_kinds()` sorts the KovOBS library by the game's tags: 290 tracking folders, 236
static, 142 dynamic and 67 switching. The hand-written labeller misses most moving targets: it looks for compact spots
up to 120 px, so it skips capsules, close spheres and a target under the crosshair, and small_v11 learned the same
blind spot (on held-out tracking frames it marked clouds and health bars and left the bot unmarked). So the moving
recordings get their own labeller, `build_data.dark_labels`: in a recording that shows dark targets on light walls
(the user's usual theme turns targets black), every blob dark in all three channels, at least 6 px, filling a third
of its box, up to 400 px a side and up to 25 times taller than wide. A recording with more labels than its scenario
has targets (a dark grid, dark props) is dropped. The 2 newest recordings of every moving scenario gave 475 of 593
training VODs (337 scenarios, 34,043 crops), with val and test split by folder as before.

**Three models, trained side by side** (10 epochs each, all from earlier checkpoints):
- small_v13: small, on everything (the small_v11 data plus the moving data);
- full_v3: full (80,765 parameters), on everything, from full_v2;
- small_mv1: small, on the moving data alone.

**Crops** (against the automatic labels; threshold 0.3):

| Model | Moving test crops F1 | Static test crops F1 |
| --- | --- | --- |
| small_v11 | 0.811 | 0.912 |
| small_mv1 | 0.885 | 0.902 |
| small_v13 | 0.891 | 0.915 |
| full_v3 | 0.889 | 0.917 |

**Whole recordings against the stats files** (`eval_moving.py`). Static: the four end-to-end VODs, the valorant run and
three 1wall 6targets extra small runs (854 kills). Dynamic and switching: the newest recording with a stats file of
the first 6 test-split folders of each kind (1,112 kills; never trained on). Tracking: 12 test-split folders; the
review's time on the target against the stats file's accuracy, the game's own measure of the same thing.

| Check | small_v11 | small_mv1 | small_v13 | full_v3 |
| --- | --- | --- | --- | --- |
| Static: kills matched, flicks measured | 854, 846 | 854, 837 | 854, 844 | 854, 844 |
| Dynamic and switching: kills matched, flicks | 1092, 1009 | 1091, 1045 | 1096, 1048 | 1097, 1060 |
| Tracking: time on target minus accuracy, mean and mean size | -0.161, 0.211 | -0.054, 0.073 | -0.035, 0.086 | -0.025, 0.075 |
| Uploads and Aim Lab (HUD kills): kills, flicks (of 997) | 997, 990 | | 997, 984 | 997, 982 |

**One model is enough.** The static-only small_v11 already matches 98% of dynamic and switching kills: a moving target
is a target, and the flick to it is measured the same way. What it could not do was tracking. Trained on everything,
both new models keep static as it was and do best on dynamic and switching, and on tracking they come as close to the
stats files as the moving-only model (full_v3 0.075, small_mv1 0.073, small_v13 0.086; small_v13's gap is mostly one
run, Air Tracking 180: 0.39 for an accuracy of 0.62, full_v3 0.48). So dynamic clicking and switching need no model of
their own, and neither does tracking. full_v3 is the more accurate of the two on moving runs (12 more flicks measured,
tracking closer), at 2.3 times the compute, which the GPU review does not notice; on the uploads it measures 2 fewer
flicks than small_v13 and 8 fewer than small_v11, all on the Aim Lab run (194 of 206; every kill is matched). The
user put accuracy before speed, so full_v3 is the review's model; small_v13 is the choice where speed counts (the
CPU, the browser, KovOBS).

**Splitting stays possible.** The review knows each run's kind from its scenario name before it tracks, so a
kind-specific model could be picked per run without any other change. The other way to combine two models into one is
the one used here: train one model on both datasets (or on both models' labels). Neither is needed now.

## The app's pipeline, 2026-10-03

The figures above come from `python/review.py`: the PyTorch checkpoint on the GPU with bf16 autocast, and KovOBS's
default areas (`review.MASK`) on every recording. The app reviews another way. `aimview-tool review`
(`service/src/bin/aimview-tool.rs`) decodes with ffmpeg, runs the Rust core, and runs the model's `_u8in` export in
ONNX Runtime on DirectML, in fp32. It leaves out the areas the user saved for the recording (else KovOBS's layout), and
watches each area for pop-ups. `eval_vods.py` and `eval_moving.py` now review this way by default. These numbers are
the baseline for later detector work.

The recordings and settings are those of the section above. `eval_moving.py`: the same 32 recordings. Run today, it
would pick Plaza Palace Easy (recorded 2026-10-03) in place of Pokeball Frenzy Auto Small Wide, so these runs kept the
earlier set. `eval_vods.py`: the four end-to-end VODs. The tool was built at commit 7f494f5.

**The review code changed since the figures above.** Scored again with today's `review.py`, the cached Python-path
tracks (`moving_<name>.pkl`) give the same kills and flicks, but other tracking numbers: full_v3 -0.016 and 0.077
(was -0.025 and 0.075), small_v13 -0.026 and 0.084 (was -0.035 and 0.086). `track_summary` now ends the run at its
last frame on a target and starts it the time limit before. It used to start at the first frame with a target, which
took in KovaaK's countdown. So the native numbers are compared with the Python path scored today.

**Every scenario kind** (`eval_moving.py`, cells: kills matched, flicks measured):

| Check | full_v3 Python | full_v3 native | small_v13 Python | small_v13 native |
| --- | --- | --- | --- | --- |
| Static, 854 kills | 854, 844 | 854, 848 | 854, 844 | 854, 847 |
| Dynamic, 707 kills | 694, 672 | 694, 670 | 693, 666 | 693, 669 |
| Switching, 405 kills | 403, 388 | 403, 388 | 403, 382 | 403, 380 |
| Tracking: time on target minus accuracy, mean and mean size | -0.016, 0.077 | -0.016, 0.076 | -0.026, 0.084 | -0.027, 0.084 |

The tracking runs, native (Python is within 0.004 on every run):

| Run | Accuracy (stats file) | full_v3 on target | small_v13 on target |
| --- | --- | --- | --- |
| AA4V OW Easy | 0.75 | 0.81 | 0.81 |
| Aethercontrol Easy | 0.75 | 0.84 | 0.85 |
| Air Tracking 180 | 0.62 | 0.48 | 0.40 |
| cA FBS Easy S1 | 0.67 | 0.69 | 0.63 |
| Centering II 180 No Strafes Fixed | 0.59 | 0.43 | 0.48 |
| cloverRawControl Big | 0.78 | 0.87 | 0.87 |
| Easy Throne Strafe | 0.54 | 0.55 | 0.55 |
| Flower 50cm | 0.65 | 0.68 | 0.60 |
| Pasu Track Smaller rAim | 0.46 | 0.33 | 0.35 |
| Pokeball 1w2ts Pasu Perfected | 0.62 | 0.49 | 0.49 |
| Pokeball 1w4ts 30% | 0.24 | 0.27 | 0.29 |
| Pokeball Frenzy Auto Small Wide | 0.21 | 0.24 | 0.25 |

**The four stats-file VODs** (`eval_vods.py`, cells: kills matched, confirmed, flicks measured; the hand-written
detector is today's Python review too):

| VOD | Hand-written | full_v3 Python | full_v3 native | small_v13 Python | small_v13 native |
| --- | --- | --- | --- | --- | --- |
| 1w4ts Voltaic, 143 kills | 143, 131, 143 | 143, 143, 141 | 143, 143, 142 | 143, 143, 141 | 143, 143, 142 |
| 10 Sphere Hipfire XS, 155 kills | 155, 138, 155 | 155, 134, 155 | 155, 134, 155 | 155, 129, 154 | 155, 129, 154 |
| Pokeball 5 Sphere XS, 114 kills | 114, 74, 114 | 114, 67, 111 | 114, 68, 114 | 114, 72, 113 | 114, 76, 114 |
| Pokeball 1 Sphere XS, 84 kills | 84, 41, 83 | 84, 56, 83 | 84, 55, 83 | 84, 62, 83 | 84, 62, 83 |
| **Total, 496 kills** | 496, 384, 495 | 496, 400, 490 | 496, 400, 494 | 496, 406, 491 | 496, 410, 493 |

The native report (the Rust core's) and `eval_moving.py`'s scoring of the same native tracks agree on every kill and
flick of these four VODs.

**What differs, and why.** The native path gives the Python path's tracks exactly when the model runs the same way.
The check: the two recordings that differ most (cA FBS Easy S1 and 360 Tracking OW2, the same boxes in about 70% of
frames) tracked in Python with the `_u8in` export in ONNX Runtime (fp32, CPU) and the native review's areas. Every
frame then has the same boxes as the native review (count, and places within 0.1 deg), and the numbers are the same.
So decoding, the fixed map, `keep`, the area watch and `link` add nothing. Two inputs differ, and they explain every
difference:

- **The model's arithmetic.** Python runs the checkpoint with bf16 autocast, the training precision. The app runs the
  fp32 export. Scores near the 0.3 threshold fall on either side, so a box shows in one and not the other. On
  recordings with the default areas, the two paths give the same boxes in 89% to 99% of frames, and in 67% to 71% on
  cA FBS Easy S1 (full_v3: Python 1.85 boxes a frame, native 1.82) and 360 Tracking OW2. The results move little:
  time on target by 0.004 at most, flicks by up to 3 (small_v13 on Smoothbot Switch Robots: 35 in Python, 32 native;
  full_v3 on Pokeball 5: 111 and 114). Earlier, fp32 inference cost six confirmed kills on Pokeball 1 (55 against 61,
  Deployment above). Here it costs full_v3 one there (55 against 56) and gains elsewhere: 400 confirmed on both paths
  for full_v3, 410 against 406 for small_v13.
- **The user's areas.** Python tracked every recording with KovOBS's layout. The app uses the areas the user saved:
  here, on seven of the eight static recordings (all but 10 Sphere), Air Tracking 180 and Pokeball Frenzy. Their
  hand-cam box starts at 83% of the frame's width, KovOBS's webcam box at 75%, so the detector sees more of the
  screen: about 0.1 more boxes a frame on the 1wall 6targets runs (4.24 against 4.35 on 889.26). Tracked in Python
  with full_v3 and the same saved areas, these recordings give the same boxes as the native review in 97% to 99% of
  frames (86% to 93% without), and their flicks move to the native counts on four of five (1w4ts 141 to 142, 889.26
  97 to 98, 886.15 95 to 96, 849.91 98 to 96). Pokeball 5 stays at 111: there the arithmetic makes the difference.
  On KovOBS's layout the area watch changed at most 0.2% of frames, on the six runs checked.

So the app's pipeline matches as many kills as the Python path on every kind (854, 1,097 and 1,096 of 1,112) and
tracks as close to the stats files. The gaps between the two paths are small: 4 flicks at most per kind, 4 confirmed
kills in all, and 0.001 in the tracking mean size.

**The open items in these numbers.**

- **Moving targets on themes other than dark-on-light: the check hardly tests it.** By `build_data.dark_scene`, the
  test the moving labeller used, 23 of the 24 moving recordings show dark targets on light walls. The one that does
  not, 360 Tracking OW2 (4% of its screen dark, at most 1.2% on the others), is the weakest clicking run: 7 of 10
  kills matched, 5 flicks, with both models on both paths. One run of 10 kills says little. The check needs moving
  runs on other themes before it can measure this item. (Two static runs have a dark wall, 1wall 6targets 889.26 and
  849.91: every kill matched.)
- **Thin capsules: still the largest under-reading.** Centering II 180 (a capsule a few pixels wide) reads 0.43 on
  target with full_v3 for an accuracy of 0.59, the largest gap below the stats file of the 12 runs (small_v13 0.48).
  Small targets held under the crosshair still read low too: Pasu Track Smaller 0.33 and 0.35 for 0.46, Pokeball
  1w2ts 0.49 for 0.62. Air Tracking 180 is small_v13's worst run (0.40 for 0.62; full_v3 0.48).
- **Tiled-wall seams: no kill or flick lost here, but small_v13 still sees a seam.** The valorant run matches 66 of 66
  kills and measures 64 flicks on both paths (the two unmeasured kills have a track of one or two frames before the
  kill). The 1wall 6targets runs match every kill and measure all but 2 flicks (849.91: 96 of 98). On 889.26's dark
  tiled wall, small_v13 keeps 5.05 boxes a frame against full_v3's 4.34; on 886.15's light wall both keep 4.4. So
  small_v13 still marks something on 889.26's wall, most likely the corner seam under "Tiled walls" below, and the cap
  on the target count is what keeps it out of the kills.

Other weak runs, all dark-on-light: Smoothbot Switch Robots (54 of 56 kills; 41 flicks with full_v3, 32 with
small_v13: the killed robot's track mostly starts 1 to 3 frames before the kill) and Bounce 180 Sparky Jumbo (100 and
98 of 107 kills).

The commands (results in `test_out/vod_model/eval/`: `vods_detector_full_v3_u8in_native.json`,
`vods_detector_small_v13_u8in_native.json`, `moving_full_v3_native.pkl`, `moving_small_v13_native.pkl`):

```bash
python python/model/eval_vods.py python/model/exports/detector_full_v3_u8in.onnx
python python/model/eval_vods.py python/model/exports/detector_small_v13_u8in.onnx
python python/model/eval_moving.py full_v3=python/model/exports/detector_full_v3.pt small_v13=python/model/exports/detector_small_v13.pt
# the Python path, for the comparison (eval_moving.py scores its cached tracks again)
python python/model/eval_vods.py test_out/vod_model/runs/full_v3/best.pt --python
python python/model/eval_vods.py test_out/vod_model/runs/small_v13/best.pt --python
python python/model/eval_moving.py full_v3=python/model/exports/detector_full_v3.pt small_v13=python/model/exports/detector_small_v13.pt --python
```

`eval_vods.py` takes the `_u8in` file here because it names its results after a `.pt` file's folder: two models in
`exports/` would both write `vods_exports_native.json`. A native review took 21 s on average with full_v3 and 18 s
with small_v13 over the 32 recordings, on a GPU shared with other work (not a benchmark). The uploads and Aim Lab row
of the section above (kills from the HUD) was not measured again.

## full_v4: the checked crops (2026-10-04)

full_v3 fine-tuned on the crops the user checked by eye. It fails the gate, and so do the control full_v4c and
full_v5 (below); full_v3 stays the best model.

**Data** (REPRODUCE.md step 1, `checked_data.py`). The two sets with the user's boxes (Known limitations below):
`data_themes_checked`, 405 crops of moving targets on other themes (train 189, val 143, test 73; 353 boxes, 89 crops
without a target), and `data_mined_checked`, 109 of the 110 mined crops (train 99, val 5, test 5; 156 boxes, 3
without a target; one left out for a slip on the phone page, a 51.4 x 2.8 px box). Splits as before; the target masks
made again from the checked boxes. The crops are counted 3 times, through tags in their names (a recording's hash
would also weight 2,428 other training crops of 27 of the same recordings).

**Training** (REPRODUCE.md step 2, `full_v4.json`). From full_v3's best.pt, its model and augmentation, 4 epochs at
lr 0.0005 (full_v3's 0.0015 / 3), on 71,881 training crops (full_v3: 71,017) and 8,707 val crops (8,559 + the new
148). About 100 s an epoch (the first 126 s), 7 minutes in all.

| Epoch | 1 | 2 | 3 | 4 |
| --- | --- | --- | --- | --- |
| Val F1 | 0.9427 | 0.9428 | **0.9444** (best.pt) | 0.9429 |

On the same crops, full_v3 and full_v4 score alike: val F1 0.9443 and 0.9444 (full_v3's own val sets: 0.9445 both).
On the checked themes crops full_v4 gains a little (val 0.932 to 0.946, 21 to 15 false finds; test 0.959 both), and
its boxes sit closer (median center error 0.89 to 0.78 px on val, 1.00 to 0.89 on test). The mined val and test hold
only 14 boxes (F1 0.786 to 0.769).

**The gate** (`accept.py <name>`, the app's native review; raw outputs in `test_out/baselines/<name>/`). full_v4c
and full_v5 are below; "Allowed" is the drop from full_v3 the gate lets through.

| Check | full_v3 | full_v4 | full_v4c | full_v5 | Allowed |
| --- | --- | --- | --- | --- | --- |
| Contract | meets it | crosshair FAIL | crosshair FAIL | meets it | every check |
| Static kills, flicks (854) | 854, 847 | 854, 850 | 854, 850 | 854, 850 | 0, 5.5 |
| Dynamic kills, flicks (707) | 704, 695 | 704, 701 | 706, 698 | **702**, 694 | 0, 7.2 |
| Switching kills, flicks (405) | 403, 392 | **400, 384** | **400, 384** | **400**, 391 | 0, 7.4 |
| Tracking gap: mean size, mean | 0.0875, -0.0325 | 0.0857, -0.0222 | 0.0819, -0.0410 | 0.0800, -0.0247 | 0.031, 0.055 |
| Report (4 static runs) kills, flicks (496) | 496, 493 | 496, 494 | 496, 494 | 496, 494 | 0, 3.6 |
| Video alone, all: recall, precision | 0.945, 0.957 | **0.928, 0.934** | 0.939, **0.945** | 0.943, 0.956 | 0.007, 0.006 |
| static | 0.926, 0.948 | **0.900, 0.907** | 0.917, **0.924** | 0.926, 0.948 | 0.010, 0.009 |
| dynamic | 0.980, 0.988 | **0.972, 0.980** | 0.978, 0.988 | 0.978, 0.986 | 0.007, 0.005 |
| switching | 0.913, 0.903 | 0.911, 0.904 | 0.915, 0.906 | 0.911, 0.900 | 0.024, 0.025 |
| Gate | (the best) | FAIL | FAIL | FAIL | |

full_v4's losses sit in a few runs. The contract's crosshair check: full_v4 boxes the user's crosshair in 5.9% of the
turning pairs of 1wall 6targets extra small 849.91 (full_v3 0.9%; 1.26% more over all, 0.5% allowed). Switching: all
on Smoothbot Switch Robots (54 to 51 kills, 43 to 35 flicks). Video alone: VT ww5t Advanced S5 1520 (151 to 100
kills found) and 1wall 2targets xsmall valorant 558.46 (40 to 4 found, 88 video kills against 69). The gains are
small: 3 more static flicks, 6 more dynamic ones, and tracking a little closer to the stats files.

**The control, full_v4c.** full_v4's recipe without the checked crops (REPRODUCE.md step 2). It fails too, so the
recipe itself caused most of full_v4's losses. It has the crosshair failure (5.2% on 849.91), the same Smoothbot run
(51 kills, 35 flicks, though it has no robot labels) and valorant 558.46 (9 kills found, 97 video kills). It does not
have VT ww5t's (142 found): that one, and part of the dynamic loss, came from the checked crops. Val F1 by epoch
0.9432, 0.9409, 0.9469 (best.pt), 0.9448, on full_v3's own val set (full_v3 0.9445).

**full_v5.** Half full_v4's learning rate (0.00025), because the control showed 0.0005 alone fails. Two data changes
came from the diagnosis of full_v4. In the user's second pass on the mined crops (`checked_phone_2.jsonl`), 11 boxes
that covered only the crosshair's dot move to "covered". Those are bots hidden under the crosshair, so train.py now
takes them as ignore regions: no heatmap loss (positive or negative) on the grid cells such a box covers, with a cell
of slack, and no regression there (`train.kept_cells`). The 3 Switching Humanoid crops with whole-robot boxes are left
out. `data_mined_checked2`: 106 crops (train 96, val 5, test 5; 141 boxes, 11 ignore boxes, 5 crops without a
target). 71,872 training crops; val F1 0.9379, 0.9437, 0.9451 (best.pt), 0.9451.
The ignore regions were checked (`test_out/baselines/full_v5/ignore_check.json`). On 4 batches of 32 crops with no
ignore field, the loss equals the old train.py's bit for bit. On a batch with the 11 ignore crops (180 cells ignored),
random outputs in the ignored cells leave the loss unchanged to the bit. The loss equals the old formula summed over
the other cells (1.3321577 against 1.3321576), against 1.3575 without the mask. After the flips and turns (all 8
seen), the ignore boxes equal the boxes they were copied from, and a marker pixel follows them 64 times of 64.

full_v5 meets the contract (849.91: 1.1%, full_v3 0.86%). The video-alone check is back to full_v3's level (valorant
558.46: 41 found; VT ww5t still 136 against 151). It fails only on kills matched. Dynamic: 360 Tracking OW2 matches 5
of its 10 kills (full_v3 7, full_v4c 9). Switching: Smoothbot Switch Robots matches 51 of 56 (full_v3 54), though its
flicks are back to 42 (43). The settings file got a score map (0.2865 on full_v3's 0.3 scale).

## full_v6: the robots (2026-10-05)

full_v5's recipe with the robot set: 309 crops of 29 robot runs the user played, checked on the phone page with one
box around each whole robot (REPRODUCE.md step 1, `data_robots_checked`), counted 3 times. 72,799 training crops;
val F1 0.9466 (best.pt, epoch 1), 0.9426, 0.9436, 0.9454. The val set holds no robots.

The first gate failed on one kill: Smoothbot Switch Robots matched 54 of 55 (full_v3 55). At kill 47 both models box
the robot about 2.5 degrees from the crosshair, a head hit at the box's top, past the circle the matcher allowed (the
blob's radius from its area plus 0.25 degrees: 1.9 for full_v6's whole robot, 1.7 for full_v3's torso). full_v3 got
the kill from a small piece's track that ran on into the crosshair's own box. The matcher now also takes a track
whose box is within 0.25 degrees of the crosshair on a frame around the kill (`src/matching.rs`, 01214c5). With it,
on the same tracks, full_v6 matches 55 of 55 and full_v3's numbers do not change.

| Check | full_v6 | full_v3 | Allowed |
| --- | --- | --- | --- |
| Contract | meets it | meets it | every check |
| Static kills, flicks (854) | 854, 849 | 854, 847 | 0, 5.5 |
| Dynamic kills, flicks (797) | 796, 788 | 796, 789 | 0, 5.9 |
| Switching kills, flicks (404) | 404, 396 | 404, 396 | 0, 5.8 |
| Tracking gap: mean size, mean | 0.141, -0.092 | 0.117, -0.068 | 0.047, 0.074 |
| Report (4 static runs) kills, flicks (496) | 496, 494 | 496, 493 | 0, 3.6 |
| Video alone, all: recall, precision | 0.9455, 0.9606 | 0.9452, 0.9575 | 0.0066, 0.0058 |
| Gate | PASS | | |

Tracking is the one step back (within its limit). The gate's reports: `test_out/baselines/full_v6/`.

## Current best model

**full_v6** (2026-10-05, the section above), threshold 0.3. 80,765 parameters; 324.4 KB as fp32 ONNX. Static,
dynamic, switching, tracking and robots. small_v13 (32,037 parameters, 134.5 KB) is the small one, for speed. full_v3
was the best before it.

- To embed (KovOBS, the browser, any language): `python/model/exports/detector_full_v6_embed.onnx` (or
  `detector_small_v13_embed.onnx`). Raw RGB bytes and the fixed map in, the 100 best boxes out.
- With the plain float input (any ONNX runtime, the HTTP server): `detector_full_v6_fp32.onnx`.
- The checkpoint: `test_out/vod_model/runs/full_v6/best.pt`.

The checks of earlier models below were made with small_v11 as the best.

small_v11 (2026-10-02) is small_v10 trained with KovaaK's own crosshairs: the 45 PNGs in the install's `crosshairs`
folder, drawn into 60% of the training crosshairs, 5 to 40 px across, half of them tinted (`train.crosshair_real`;
the user pointed to the crosshairs and themes on kvk-hub.app, of which these are the installed ones). It also draws a
crosshair in 70% of the crops (was 60%), on a target 60% of the time (was 70%). small_v12 keeps v10's two shares and
only adds the real crosshairs. The review now also keeps a crosshair-spot track from being taken for the killed
target (`python/README.md`, "Which track is the killed target"), which all four rows below use:

| Check | small_v7 | small_v10 | small_v11 | small_v12 |
| --- | --- | --- | --- | --- |
| Stats-file VODs: matched (of 496), confirmed, flicks | 494, 391, 490 | 496, 412, 492 | 493, 391, 489 | 496, 390, 491 |
| of which Pokeball 5 confirmed | 75 | 70 | 59 | 64 |
| Five YouTube uploads: flicks measured (of 985 kills) | 979 | 975 | 980 | 966 |
| Video only, precision and recall: 1w4ts | 0.99, 0.99 | 0.71, 0.71 | 0.96, 0.95 | 0.94, 0.94 |
| 10 Sphere | 0.81, 0.82 | 0.89, 0.90 | 0.85, 0.86 | 0.84, 0.86 |
| Pokeball 1 | 0.67, 0.70 | 0.73, 0.75 | 0.73, 0.75 | 0.76, 0.77 |
| Pokeball 5 | 0.64, 0.61 | 0.64, 0.63 | 0.58, 0.56 | 0.58, 0.57 |
| Held-out hand crops (40 bots): found, false finds | 26, 86 | 31, 27 | 31, 18 | 31, 21 |
| Aim Lab, video only: right, extra, missed (of 206) | 194, 17, 11 | 195, 20, 10 | 189, 18, 16 | 193, 13, 12 |

small_v11 fixes what small_v10 broke (1w4ts from the video alone; the crosshair marked in 17% of ww5t 1920's turning
frames, against v10's 24%, and the review rule handles the rest) and keeps the hand labels' gains. The cost is
Pokeball 5: KovaaK's red-dot crosshairs over its tiny targets look like the training's real crosshairs, so the target
is confirmed to the frame less often (59 of 114). Its flicks are all measured.

small_v10 (2026-10-02) is small_v7's recipe plus the first hand labels: 207 crops the user checked on the label page
(`hand_crops.py`, `label_check.py`), each counted 20 times in training. 100 come from the two valorant runs, whose
tiled walls passed for targets, and 107 from the weak cases: two Jumbo1wall9000targets runs (dense fields), Pokeball 5
and 1w4ts runs cropped round the crosshair (targets under it), two ClickTrack 2t runs (faint targets), and two
YouTube runs by another player. None of the runs below are among them. 80 crops are held out as the test (valorant
558.46, Jumbo 283, ww5t 2040). small_v9 used the valorant crops only.

| Check | small_v7 | small_v9 | small_v10 |
| --- | --- | --- | --- |
| Held-out hand crops, every find (40 bots): found, false finds | 26, 86 | 25, 15 | 31, 27 |
| of which Jumbo 283 (17 bots): found, false | 3, 23 | 2, 5 | 9, 12 |
| Valorant runs, false targets a frame (no cap) | 13 | 1.5 to 2 | 2.3 to 2.9 |
| Four stats-file VODs: matched (of 496), confirmed | 494, 391 | 494, 383 | 496, 412 |
| Video only, precision and recall: 1w4ts | 0.99, 0.99 | 0.90, 0.91 | 0.71, 0.71 |
| 10 Sphere | 0.81, 0.82 | 0.83, 0.85 | 0.89, 0.90 |
| Pokeball 1 | 0.67, 0.70 | 0.71, 0.71 | 0.73, 0.75 |
| Aim Lab, video only: right, extra, missed (of 206) | 194, 17, 11 | 192, 18, 13 | 195, 20, 10 |

small_v10 sees targets under the crosshair better (10 Sphere confirmed 120 to 135, Pokeball 1 54 to 64), but it
marks KovaaK's crosshair again in 16% of 1w4ts's turning frames (small_v7: none), so 1w4ts's kills from the video
alone come 4 to 10 frames late. On the YouTube uploads it measures fewer flicks on ww5t (1920: 142 against 191; 2040:
141 against 155) and more on 1902 (175 against 162). Next: draw the 45 crosshairs KovaaK's installs (`crosshairs/`,
PNG) in training instead of the made-up dot, plus and ring.

small_v7 (2026-10-02) stops taking KovaaK's crosshair for a target. small_v4 to v6 marked the user's crosshair as a
target in a quarter to two thirds of the frames where the camera turns (small_v2 never did), so in a review from the
video alone a dead target stayed "alive" on the crosshair for a few frames (1w4ts: 0.32 precision, 0.54 recall). Two
changes:
- **The labeller is small_v2** (`build_kills.LABELLER`), not the current best model: the kill-moment crops were
  labelled by a model that already marked the crosshair, so the mistake fed itself.
- **A target the labeller lost is labelled only where some of it shows beside the crosshair, unlike the wall round
  it** (the median colour of its visible pixels at least 40 away from a ring of wall). A label on the crosshair alone
  (the target already gone, or wholly covered) taught the crosshair as a target; small_v6's rule (skip only when 60%
  is covered) still let small crosshairs through.
- **Crosshairs with an outline** in the augmentation (`crosshair_outline` 0.5: a 1 or 2 px edge, mostly dark).

Compared on the four stats-file VODs, the video-only check and the Aim Lab upload (206 kills, the hits read from
Aim Lab's POINTS number):

| Model | Flicks measured (4 VODs) | Confirmed | 1w4ts video only (precision, recall) | 10 Sphere video only | Aim Lab right, extra, missed |
| --- | --- | --- | --- | --- | --- |
| small_v6 | 484 | 436 | 0.32, 0.54 | 0.96, 0.97 | 168, 24, 37 |
| small_v7 | 490 | 391 | 0.99, 0.99 | 0.81, 0.82 | 194, 17, 11 |
| small_v8 (v7 without the outlines) | 473 | 394 | 0.68, 0.66 | 0.93, 0.94 | 188, 22, 17 |

(The Aim Lab numbers include the review's crosshair-spot rule, `review.crosshair_spots`.) The outlined crosshairs are
what fixes KovaaK's crosshair: small_v8, trained without them, marks it again in 36% of 1w4ts's turning frames. They
also cost 10 Sphere: a KovaaK's red dot over a tiny black target looks like an outlined crosshair, so small_v7 loses
the target just before the click (the kill count stays right, 156 for 155, but 4 to 6 frames early at 120 fps).
small_v6's higher "confirmed" count was partly the crosshair itself, seen as the target up to the kill. Every model
marks Aim Lab's crosshair in about half the turning frames (it is bigger than Aim Lab's targets, so one frame cannot
tell it from a covered target); the review handles that (`python/README.md`). Validation F1 0.961.

small_v6 (2026-10-01) is small_v4's recipe with two label fixes in `build_kills.py`:
- **Other targets are capped by the scenario's target count.** small_v4 took the other targets in each kill-moment
  crop from small_v2's detections, which included wall seams on some themes. small_v4 then marked those seams as
  targets ("Failed miserable here": the valorant wall). small_v5 kept at most the scenario's target count, by score.
- **No label for a target the crosshair covers.** small_v5 still labelled the killed target where it could not be
  seen, with 60% or more of it under the crosshair. It learned the crosshair as a target: on the uploaded Aim Lab run
  (206 kills) it marked Aim Lab's crosshair. small_v6 drops those labels; it still marked both crosshairs (above).

On the four stats-file VODs (with the review's track joining, below) small_v6 matched all 496 kills, confirmed 436
(small_v5: 425) and measured 484 flicks (small_v5: 492; 133 against 141 on 1w4ts). Its validation F1 is 0.956. On the
valorant wall both still find about 14 false targets a frame; the review's cap on the target count removes them.

The review now joins a killed target's tracks when the tracker lost it for a few frames (under the crosshair, behind a
hit effect). Without that, a kill whose last track started under the crosshair was not measured: small_v6 measured
only 72 flicks on 1w4ts, and the hand-written detector 67. With it, 133 and 130. The target radius now comes from the
whole joined track, not the last piece: on 1w4ts it reads 0.45 deg with both detectors (0.43 measured by hand; it was
0.33 with small_v6 and 0.31 with the hand-written detector).

small_v4 (2026-10-01) fine-tunes small_v2 for 10 epochs on the key-frame crops plus 10,214 crops from the moments just
before kills (`build_kills.py`): the clock lined up with the stats file from each VOD's first kills, a third of a
second decoded before about 10 kills, and the killed target labelled even where the model lost it under the
crosshair (its place follows the camera's turn). Outlines and another decoder's colours were added to the
augmentation, and the user's 12 new runs (2026-10-01, other themes, target colours, crosshairs and outlines) were
counted three times. On the four stats-file VODs it matched all 496 kills (small_v2: 494) and confirmed 414
(small_v2: 391, the hand-written detector: 396); 10 Sphere went from 127 to 142 confirmed, Pokeball 5 from 61 to 69.
On the general test crops its F1 is 0.939 against 0.946 at the same threshold: it now marks targets under the
crosshair that the automatic labels miss, which those crops count as false.

`infer.BEST` names it, and the review app and `serve.py` use it by default.

## Known limitations

- **Labels come from the hand-written detector.** The model learned what that detector finds on frames where it was
  reliable. Where it fails without its filters noticing, the labels are wrong, and so are the scores. There is no
  hand-labelled ground truth yet; the stats-file check is the independent one.
- **Dense fields of big targets.** Jumbo1wall9000targets has the lowest crop score (F1 0.49), but mostly because its
  labels are wrong: the labeller merges touching spheres into one box and skips most of the rest, because its texture
  filter takes a dense field for wall texture. In the stats-file check the model does well there (299 of 299 kills
  matched, 280 confirmed, 257 flicks measured; the hand-written detector confirms 293 and measures 158). Still, on
  crops it often leaves spheres in a dense field unmarked, so it may have learned some of the labeller's blind spot.
- **Faint, very small targets.** ClickTrack 2t scenarios (crop F1 0.79 to 0.85) have a second target that is small and
  pale, and the model misses some. Several of its "false positives" there are real targets the labels missed, and
  some are the pre-run countdown overlay (harmless: the review only reads the run). In the stats-file check on
  ClickTrack Vertical 2t Long, the model matches and confirms all 59 kills.
- **A target hidden by the crosshair.** When an extra-small target sits fully under the crosshair, neither detector sees
  it, and the kill cannot be confirmed to the frame. v2 narrowed the gap (Pokeball 5: 48 to 61 confirmed; the
  hand-written detector confirms 74).
- **The review's numbers differ between detectors.** The model's boxes include a target's anti-aliased edge, so the
  review's target radius can be larger with the model. On 1w4ts both now give 0.45° (since the review joins a killed
  target's tracks; the hand-written detector gave 0.31° before), against 0.43° measured by hand. On Pokeball the model
  gives 0.26° and the hand-written detector 0.19°. Pokeball
  5's median kill interval reads 0.42 s with the model and 0.18 s with the hand-written detector; that one has not been
  checked.
- **Moving targets: dark ones on light walls only.** The moving data comes from recordings with dark targets on light
  walls (`dark_labels`); other themes' moving targets come only through the recolouring augmentation. A set for the
  other themes is checked by eye and trained on in full_v4 (failed the gate: section above): `data_moving_themes`
  (2026-10-04; REPRODUCE.md step 1), labelled by full_v3. Few such recordings exist: of 1,087 moving recordings, 986 are
  dark targets on light walls and 49 are the checks' runs. Of the other 52, 29 have too few key frames (5-second runs)
  and 16 have labels that are not steady or more labels than targets (among them a black game screen and another game).
  7 are kept: 405 crops (train 189 from 4 recordings, val 143 from 2, test 73 from 1). Two of the 7 have wrong labels:
  773TS 90 (the model misses the big capsule and marks the marker above it) and voxTS-Huge Jumbo static 5s (its key
  frames are mostly the results screen; the boxes are on the score chart). The user checked 150 of the crops by eye
  (2026-10-04, on a phone page; `check_moving_themes/checked.jsonl` in label_check's format, the raw answers in
  `phone_answers/`): 103 right and 47 wrong, every wrong one fixed. Of the 117 boxes judged, 47 were wrong (40%), and 19
  targets had no box (18 boxed by hand, 1 marked by a tap and sized from the recording's other boxes). The picks lean
  toward uncertain crops, so the whole set's error rate is lower, but full_v3's labels on other themes cannot be trained
  on unchecked. Worst: 773TS 90 (17 of 25 wrong) and VT Controlsphere Intermediate S5 (10 of 20); best: VT Frogtagon
  Advanced S5 (2 of 25). Then the user checked the other 255 the same day, with zoom and an outline guide, and tightened
  the first 150's boxes they had kept: every one of the 405 crops is checked (`data_moving_themes/checked_phone.jsonl`,
  316 with targets, 89 without; the raw answers in `check_moving_themes/phone_answers_405/`). With zoom, the user shrank
  241 of the 252 model boxes they kept in the 255: on these moving spheres full_v3's boxes are too big (about 10% on
  Controlsphere Advanced, 5% on Controlsphere Intermediate and Frogtagon) and sit about 0.7 px up and left, in every
  recording (from the boxes fixed by hand only). The user's earlier desktop hand labels of small static targets show no
  such offset (154 pairs: median 0.05 px, size ratio 0.99).
- **Crops mined from full_v3's own mistakes** (`data_mined`, 2026-10-04; `build_mined.py`, REPRODUCE.md step 1). Trained
  on in full_v4 (failed the gate: section above). 215 recordings with a stats file were reviewed natively (3,106 s: 131
  dynamic, 57 switching, 27 tracking; the checks' runs left out; static not reached), and strict rules mined 110
  crops from 45 of them: `kill` 23 (the killed target placed where the model lost it before a kill, from 4
  recordings), `gap` 76 (a steadily tracked target missed for 1 or 2 frames), `false_static` 7 (the crosshair's dot
  boxed while the view turns) and `false_lone` 4 (a box in one frame with nothing near or like it). Train 100, val
  5, test 5. Each crop's rule is its `mined` field. full_v3 rarely makes a clear-cut mistake there: most candidates
  were dropped as unclear (another box touching the place, more boxes than the scenario's targets, a clock that
  KovaaK's countdown does not confirm). Known wrong ones: AngelClick Revolving Avasive Easier (10 crops) and one
  VT DriftTS crop place a box on overlapping spheres (two in one box, or one with the other unlabelled); Switching
  Humanoid (3 `false_lone` crops) takes out a box on a robot's head, and the robots have no labels. The user checked all
  110 by eye on the phone page the same day (`data_mined/checked_phone.jsonl`, raw answers in
  `check_mined/phone_answers/`): every `false_static` crop was right (the crosshair's dot is no target), 3 of the 4
  `false_lone` were wrong (the robot heads: boxed as whole robots), all 23 `kill` placements and 51 of the 76 `gap`
  boxes needed moving or resizing, and 3 crops have no target left.
- **Robots.** Only 2 recordings of robot targets exist: Smoothbot Switch Robots is a check run, and Close Fast
  Colosseum Robots was dropped for more labels than targets. More robot recordings are needed.
- **Thin capsules.** The model splits a thin capsule into short boxes and can leave its end unboxed; on Centering II
  (a capsule a few pixels wide) the time on target reads 0.46 for an accuracy of 0.59. Small targets held under the
  crosshair in tracking (Pasu Track Smaller, Pokeball 1w2ts) also read low, by 0.12 to 0.15.
- **Tiled walls.** small_v13 marks far fewer seams than small_v11 on 1wall 6targets extra small 889.26, but one
  corner seam still scores 0.35, and when fewer targets than the scenario's count are on screen, the cap keeps it.
- **One resolution tested.** Frames are scaled to 1280 × 720. Other sizes work if both sides are multiples of 16, but
  were not evaluated.

## Next steps

1. **Ground truth.** Label a few hundred crops by hand (or check the automatic labels by eye), most of them from the
   weak cases: dense fields, faint targets, targets under the crosshair. Then the crop scores mean something.
2. **Dense fields.** Paste extra targets into training crops (copy and paste from the target mask), touching and in
   rows, and label dense scenarios with a labeller that has no texture filter.
3. **Check the review's numbers.** Take the target radius from the scenario's `.sce` (size and distance) as a second
   check, and find out why Pokeball 5's median kill interval differs between the detectors.
4. **Tracking scenarios.** Done (full_v3, small_v13). Next: moving targets on other themes (labelled with full_v3
   in `data_moving_themes`; all 405 crops checked by eye: `checked_phone.jsonl`; full_v4 trained on them and failed
   the gate), thin capsules, and more
   tracking runs in the check.
5. **WebGPU.** The embed file already reads back only 100 boxes; the rest of the 12 ms floor is per-layer dispatch.
   A hand-written WebGPU shader for this small network, or fused layers, could cut it. Until then, WASM is the
   browser path.
6. **KovOBS.** Wire the chosen Rust runtime into KovOBS behind a setting, with the fixed map from the first seconds of
   key frames.
