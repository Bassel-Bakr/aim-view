# Benchmarks and their baselines

Every benchmark and check, its command, what it measures, and its latest result. The raw outputs are kept in
`test_out/baselines/<commit>/` (ignored by git), so a change is compared with them instead of with a fresh run of the
code before it.

## Rules

- Look here before running a benchmark. A baseline stands until a change touches what it measures: the code on its
  path, the model, or its data. Then rerun it; otherwise don't.
- Run only the benchmarks whose answer can change the decision. A refactor that must not change any result needs the
  parity tests and the byte-compare against the kept outputs, not the stats-file checks. A speed benchmark runs only
  for a change on the path it times.
- A run that sets a new baseline keeps its raw output in `test_out/baselines/<commit>/`, and its row here is updated
  in the same commit as the change.

## Accuracy: the stats-file checks

Baseline: commit 4b7ddc4, full_v3, the native review. Raw output: `test_out/baselines/4b7ddc4/`.

| Check | Command | Result |
| --- | --- | --- |
| Static runs | `python python/model/eval_vods.py full_v3` | 1w4ts Voltaic 143/143 kills, 142 flicks; 10 Sphere Hipfire Extra Small 155/155, 155; Pokeball 5 114/114, 113; Pokeball 1 84/84, 83 |
| Every scenario kind | `python python/model/eval_moving.py name=full_v3` | static kills 854/854, flicks 847; dynamic and switching kills 1097/1112, flicks 1062; tracking on target minus accuracy: mean -0.033, mean abs 0.087 |
| Video alone (48 runs) | `python python/model/eval_video_alone.py full_v3` | all: recall 0.945, precision 0.957 (5,117 of 5,417 kills); held out: 0.974, 0.972; switching: 0.913, 0.903 |

The video-alone benchmark's harness, caches and notes: `test_out/baselines/vbench/` (`bench.py`, `final_all.txt`).

## Correctness

| Check | Command | Baseline |
| --- | --- | --- |
| The core against Python | `cargo test --profile quick` | every test passes; fixtures in `test_out/parity/` |
| The native review, byte for byte | `cargo run -p aimview-service --release --example track -- "<video>" python/model/exports/detector_full_v3_u8in.onnx <out> 0 2 4 - - <stats.csv>` (DirectML, 2 runs, 4 frames a call) | `test_out/baselines/4b7ddc4/native/`: av1 (1wall 2targets xsmall, with test_out/parity/av1/review/stats.csv, and `no_stats/` without it) and flower (Flower Easier); tracks, readings, hud and report. Equal through 38101cb (the Vec refactor) |

## Speed

Measured from 2026-10-02 to 2026-10-04, before 4b7ddc4. Remeasure only for a change on the path a row times.

| What | Where | Result |
| --- | --- | --- |
| av1 (2560x1440), whole review, native | the desktop app, DirectML | 12.7 s |
| av1, whole review, native, with the HUD | the service, DirectML | 11.0 s |
| The byte-compare's reviews (the track example above) | DirectML, at 4b7ddc4 / at 38101cb | av1 with stats 13.0 and 12.7 s / 11.7 s; without stats 11.4 s; flower 13.9 s (`timing.txt` beside each) |
| av1, whole review, browser | WebGPU, 4 frames a call | 15.6 to 16 s |
| av1, 0:20 to 0:40 window | browser / native | 7.2 s / 6.7 s |
| h264_1920_tv, whole review, browser | WebGPU | 14.0 s |
| 2:1 RGB conversion, a frame | `test_out/browser_check/rgb-bench.html`; native AVX2 | 0.93 ms / 0.29 ms |
| The browser's stages | `test_out/browser_check/profile.html`, `decode-bench.html` | 380 frames a second on the GPU; one decoder about 430 |
| KovaaK's files into the browser | browser mode, 3,000 files | shown in 14 ms, copied in 1.0 s |
| The recordings list, native | `/api/vods?quick=1` / `/api/vods` | 0.04 s / 0.16 s |
