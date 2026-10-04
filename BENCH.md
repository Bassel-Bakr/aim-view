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
- A change on a function's path reruns that function's bench (`cargo bench -- <name>`, "Function benchmarks" below)
  against the saved baseline. Whole reviews are timed only for changes to decode, convert or the detector.

## Accuracy: the stats-file checks

Baseline: commit b8c56ef (the stats-file matching; the tracks are 4b7ddc4's), full_v3, the native review. Raw output:
`test_out/baselines/b8c56ef/` (`before_after.txt`: eval_moving.py's runs before and after; eval_vods.py's numbers
from the tracks its 4b7ddc4 run kept). Before it: `test_out/baselines/4b7ddc4/`. Unchanged at 9b04bc5 (the arrival
between frames moves no count): `test_out/baselines/9b04bc5/`, with `arrival_changes.txt`, how each flick's arrival moved.
At 5a66b5c (the crosshair spot at the screen's center, crosshair ends cut) the first two rows are unchanged and the
video alone moves (all from kept tracks): `test_out/baselines/5a66b5c/` (`notes.txt`; there too the HUD's check of
the stats file's clock, which is 9 frames late on valorant 558.46 with every model).

| Check | Command | Result |
| --- | --- | --- |
| Static runs | `python python/model/eval_vods.py full_v3` | 1w4ts Voltaic 143/143 kills, 142 flicks; 10 Sphere Hipfire Extra Small 155/155, 155; Pokeball 5 114/114, 113; Pokeball 1 84/84, 83 |
| Every scenario kind | `python python/model/eval_moving.py full_v3=python/model/exports/detector_full_v3_u8in.onnx` (`name=path`: a bare name is read as the model's name and retracks) | static kills 854/854, flicks 847; dynamic and switching kills 1107/1112, flicks 1087 (Bounce 180 Sparky Jumbo 107/107, 105; Falling Targets 54/54, 54; Smoothbot Switch Robots 54/56, 43); tracking on target minus accuracy: mean -0.033, mean abs 0.087 |
| Video alone (48 runs) | `python python/model/eval_video_alone.py full_v3` | all: recall 0.945, precision 0.958 (5,120 of 5,417 kills); held out: 0.976, 0.974; switching: 0.913, 0.903 (5a66b5c; before: 0.957, and 0.974, 0.972 held out). full_v4: 0.944, 0.960 (0.928, 0.934 before) |

The video-alone benchmark's harness, caches and notes: `test_out/baselines/vbench/` (`bench.py`, `final_all.txt`).

## Correctness

| Check | Command | Baseline |
| --- | --- | --- |
| The core against Python | `cargo test --profile quick` | every test passes; fixtures in `test_out/parity/` |
| The native review, byte for byte | `cargo run -p aimview-service --release --example track -- "<video>" python/model/exports/detector_full_v3_u8in.onnx <out> 0 2 4 - - <stats.csv>` (DirectML, 2 runs, 4 frames a call) | `test_out/baselines/5a66b5c/native/`: av1 (1wall 2targets xsmall, with test_out/parity/av1/review/stats.csv, and `no_stats/` without it) and flower (Flower Easier); tracks, readings, hud and report. `python test_out/baselines/native_compare.py 5a66b5c` reviews all three and compares. At 5a66b5c av1's two reports moved (the crosshair spot, and the measures from the cut tracks); they come from the replay below, and the other files are 9b04bc5's (no native review ran). Tracks, readings and hud equal to 4b7ddc4's through 9b04bc5; the report's `appeared` changed at b8c56ef (the matching's joins), and av1's measures at 9b04bc5 (the arrival: arrive, dwell, settle, hold and parts, and the summary's arrive, budget, holding and what-if; flower's report is the same). The script builds the checkout it is run from (a worktree too) |
| The review after the detector, byte for byte | `cargo test --profile quick --test replay` (0.8 s) | the native review's parts before the join (`test_out/baselines/parts/`, kept once by the track example's `--parts <folder>`; `saved.txt` there) joined and reported again: all 12 files equal to the byte-compare's baseline above (tests/replay.rs `NATIVE` names it; move it with this row). A change after the detector (keep, link, the camera's readings, the HUD's reading, matching, measures, the report) needs only this. A change before the join (decoding, the conversion, the detector, a watch's reading of each frame) needs the native byte-compare, then the parts kept again |

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
| The recordings list, browser | the browser build on a stand-in VODs folder in the browser's storage (the real 3,351 names in 786 folders, 1-byte files; `test_out/baselines/browser_list/`) | the full list 0.5 s after the quick one, 2.6 to 2.9 s from page load (4b7ddc4: 2.4 s after, 4.5 s from load) |

## Function benchmarks

The hot paths (`HOT_PATHS.md`) timed one function at a time with criterion, on real inputs: the parity fixtures
(`test_out/parity/`) and the native review's kept outputs (`test_out/baselines/4b7ddc4/native/`). The code is in
`benches/hot_paths/`. A bench whose input is missing is skipped with a message. The whole suite takes about 80 s
(each bench 0.5 s of warm-up and 2 s of samples), after a build of about 2 minutes (the release profile, with LTO).

- Compare with the baseline: `CRITERION_HOME=test_out/baselines/criterion cargo bench -- <name> --baseline 9cbcbf7`
  (`<name>`: part of a bench's name, such as `track/link`; leave it out for every bench). In PowerShell, set
  `$env:CRITERION_HOME = 'test_out/baselines/criterion'` first.
- Save a new baseline: the same with `--save-baseline <commit>`. Then update the table below.
- Each bench's median: `python test_out/baselines/criterion_medians.py test_out/baselines/criterion <commit>`.

Runs of the same code differ by up to about 10% on this machine (more on the cluttered links), so criterion reports a
change only beyond 5%, and a change under 10% needs a second run before it counts.

Baseline: commit 9cbcbf7 (2026-10-04), `test_out/baselines/criterion/` (each bench's `9cbcbf7/` folder). The
matching and report benches were saved again at 326eb95, after b8c56ef changed the matching (their `326eb95/`
folders), and with the measure bench at 9b04bc5 (the arrival), and again at 5a66b5c (the crosshair ends): compare
those three with `--baseline 5a66b5c`.

| Bench | What one call does | Median |
| --- | --- | --- |
| `fixed/contrast` | `fixed::contrast` of one av1 key frame (1280 x 720, YUV 4:2:0) | 2.78 ms |
| `fixed/add` | `FixedMap::add`: one key frame into the map, av1's 25 in turn | 4.82 ms |
| `convert/rgb24_2560` | `Converter::rgb24`, av1's frame (2560 x 1440, full range): the 2:1 shortcut | 316 µs |
| `convert/luma_2560` | `Converter::luma`, the same frame | 562 µs |
| `convert/rgb24_1920` | `Converter::rgb24`, a 1080p upload (limited range): swscale's full pipeline | 5.54 ms |
| `convert/luma_1920` | `Converter::luma`, the same frame | 2.82 ms |
| `track/keep_av1` | `track::keep` on every av1 frame (6,038), with its areas and target count | 3.82 ms |
| `track/link_av1` | `track::link` (and its view shift) on av1's kept targets | 21.2 ms |
| `track/link_2007_1w6ts_aimlab` | `track::link`, a cluttered Aim Lab run's targets (3,801 frames) | 59.7 ms |
| `track/link_10_sphere_hipfire` | `track::link`, 10 Sphere Hipfire's targets (4,006 frames) | 68.6 ms |
| `camera/add` | `CameraWatch::add`: one 720p luma frame of flower | 1.04 ms |
| `camera/reading` | `CameraWatch::reading`: one frame's reading from its tiles' shifts | 303 ns |
| `camera/countdown_showing` | `camera::countdown_showing` on av1's 720p RGB frame | 1.30 µs |
| `camera/excluded` | `camera::excluded`: the tiles' excluded pixels from flower's fixed map | 4.42 ms |
| `hud/keys_2560` | `HudWatch::add_key` on 25 key frames of av1 (its two decoded frames in turn), then `keys` | 34.2 ms |
| `hud/add_2560` | `HudWatch::add`: one av1 frame's Y plane (2560 x 1440) | 206 µs |
| `hud/add_1920` | `HudWatch::add`: one frame of the 1080p upload | 317 µs |
| `popup/add_look` | `AreaWatch::add`, a frame it looks at (every second frame), av1's areas | 330 µs |
| `areas/finish_av1` | `AreaFinder::finish` on av1's 25 key frames | 48.6 ms |
| `matching/match_times_av1` | `matching::match_times`: av1's 66 kills from the stats file (with `clock_offset`) | 9.20 ms; 8.17 ms at 326eb95; 9.61 ms at 5a66b5c |
| `matching/match_video_av1` | `matching::match_video`: av1's kills from the video alone | 12.4 ms; 8.53 ms at 326eb95; 10.0 ms at 5a66b5c |
| `measure/measure_av1` | `measure::measure`: av1's matched flicks | 482 µs; 311 µs at 9b04bc5 (309 µs just before it, at f096454) |
| `report/clicks_av1` | `review::review_clicks` with the stats file: matching, measures, summary and checks | 14.5 ms; 13.1 ms at 326eb95; 14.1 ms at 5a66b5c |
| `report/hud_av1` | `review::review_clicks` without it: the kills from the HUD's reading | 13.0 ms; 12.5 ms at 326eb95; 13.4 ms at 5a66b5c |
| `report/json_av1` | `review::review_json`: the service's report request for av1, JSON in and out | 21.6 ms; 17.4 ms at 326eb95; 18.2 ms at 5a66b5c |
| `report/tracking_flower` | `review::review_tracking`: flower with its stats file and camera readings | 3.40 ms; 2.14 ms at 326eb95 |
