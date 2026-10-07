# Hot paths

Where a review spends its time, stage by stage, with how often each stage runs and what it costs. Check this before
optimizing anything, and update a row when a change or a measurement moves it. The benchmarks that time these, and
their baselines, are in `docs/BENCH.md`.

The numbers are for av1 (2560x1440, about 6,000 frames at 60 fps, 25 key frames) unless a row says otherwise. A name
in backticks after a number is its criterion bench (`cargo bench --bench hot_paths -- <name>`): medians at 9cbcbf7,
or at 5a66b5c for matching and the report (docs/BENCH.md, "Function benchmarks"; runs differ by up to about 10%). Numbers
marked "survey" come from a scratch bench at 9eef1f4 run while another build was going (about ±30%).

## Profiling a whole review

Three tools, each on the native review of av1 0:20 to 0:40 (`cargo build --profile profiling`: release with its
symbols):
- the wall time: `hyperfine -N --warmup 1 --runs 5 "<track.exe or aimview-tool review ...>"`;
- the CPU: `cargo flamegraph --profile profiling -p aimview-service --bin aimview-tool -o flamegraph.svg -- review
  <video> --out <dir> --model <_u8in.onnx> --window 20 40 --no-report --kill-check`, from an administrator's terminal
  on Windows (it records through ETW);
- the heap: `cargo run --profile profiling -p aimview-service --example track --features dhat-heap -- <video> <model>
  <out> 0 2 4 20 40`, which writes dhat-heap.json (DHAT's viewer, dh_view.html).

What they showed (2026-10-07, large_v13e4, the GPU's frames): the review is GPU-bound (the 3D engine 98% busy while
tracking; fp16 and batches of 2 or 8 frames were no faster than fp32 in 4: 5.9 s). Of the CPU's samples, NVIDIA's
driver took about half on its own threads; reading each frame back from the GPU (`GpuFrames::next_into`) 14.5%, and
DirectML's upload of the same RGB back to the GPU 5.4% (a round trip); the 720p luma from the full Y plane
(`mean_2x2`) 10%; the camera's FFTs 11%; the HUD 2.7%; allocation 3.8%. dhat: 8.45 GB in 3.2 million blocks, the
detector's output copies (2.9 GB), the camera's tiles and buffers (2.3 GB), the decoder's countdown rows (0.3 GB) and
the HUD's small vectors (1.8 million blocks) first; with those buffers kept and the detector's maps read where ONNX
Runtime leaves them, 2.39 GB in 1.2 million blocks (the same review's files, byte for byte). The GPU's frames now also
give the 720p luma (the shader's means, which `mean_2x2` made again on the CPU) and only the Y plane's rows the HUD
reads (`Review::hud_rows`: 330 of 1,440 at 1440p), so a frame's Y reads back as 0.9 MB of luma and 0.8 MB of rows
instead of 3.7 MB. A review's fixed cost (the model, DirectML's session, the key frames) is about 1.5 s. TensorRT 10.16
(fp32, through ONNX Runtime's CUDA build) ran the detector alone 8.5% faster (1.41 ms a frame against DirectML's 1.55,
`detector_speed`), but the review of the window took 6.26 s against 5.48 and 49 s of CPU against 7, its tracks moved
by up to 0.09 degrees, and it needs 5.6 GB of NVIDIA's libraries and a 30 s engine build a model: not kept (2026-10-07).

## Every frame (about 6,000 a review)

The review is a pipe: decode, convert, detect, track. The slowest stage sets the pace. Two runs are reviewed at once
on a recording of 1,200 frames or more (src/session.rs), so two decoders work in parallel.

| Stage | Where | Cost a frame | Notes |
| --- | --- | --- | --- |
| Decode | ffmpeg through a pipe (service/src/review.rs); Mediabunny and the browser's software decoder (modes/wasm/review.worker.ts) | browser: about 2.3 ms for one decoder (430 frames a second); native: ffmpeg's libdav1d takes 11.1 s for av1 (41 s of CPU), and the native review is bound by the CPU (72 to 75% busy on average): decoding, the pipe (33 GB of raw frames for av1), converting, the camera and the HUD | the browser's limit, which is why runs are split. More parts do not help (3, 4, 6 parts: 12.7 to 13.2 s, the CPU 95% busy). The GPU's decoder itself does av1 in 5.0 s with almost no CPU when the frames stay on the GPU (ffmpeg's copying them back is what costs). The GPU frame source (service/src/gpu_frames.rs: Media Foundation and a shader, on by default for 2560 x 1440 AV1 and H.264 MP4s; `--gpu-frames off` for ffmpeg's, the RGB and Y plane read back) gives byte-equal reviews on four recordings with a third to a half of the CPU (av1 11.1 s, CPU 49%, against 14.1 s, 88%); full_v3's detector then sets the pace (`test_out/baselines/756b1c5/gpu_frames/`). Hardware decoding through ffmpeg is slower in the review (16.0 s against 11.9 s: it shares the GPU with the detector) and drops the first frame of OBS's AV1 files (`test_out/baselines/b095354/review_pace/`) |
| Convert to 720p RGB and YUV | src/convert.rs | 2:1 native: RGB 0.32 ms `convert/rgb24_2560`, luma 0.56 ms `convert/luma_2560`; 1080p through swscale's pipeline: RGB 5.5 ms `convert/rgb24_1920`, luma 2.8 ms `convert/luma_1920`; browser 2:1 RGB 0.93 ms (SIMD) | byte-equal to ffmpeg; filters of 1 to 4 taps, nothing to gain from a transform; the 2:1 luma costs about twice the RGB |
| Detector | ONNX Runtime (service/src/detector.rs); onnxruntime-web (review.worker.ts) | full_v3: 1.84 ms native (DirectML); in the browser the whole pipeline runs at about 2.6 ms a frame (380 frames a second, WebGPU, 4 frames a call) | the biggest single cost; on the CPU the browser takes about 32 ms a frame (31 frames a second). It does not set the native review's pace: av1 takes 11.9 s with full_v3, 11.7 s with a 1,405-parameter network, and the same at batch 1 to 16; the CPU does (see Decode). It is bound by memory, not math: 1.37 G multiply-adds but about 1 GB through its layers a frame (fp32); CUDA in PyTorch gives the same 1.7 to 2.2 ms, fp16 1.0 ms (`test_out/baselines/b794f53/detector_speed/`, `b095354/review_pace/`) |
| Keep | src/track.rs `keep` | 0.6 µs (3.8 ms for all 6,038 frames, `track/keep_av1`) | |
| Pop-up areas | src/popup.rs `AreaWatch::add` | 0.33 ms on a frame it looks at, every second frame (`popup/add_look`) | |
| Camera watch | src/camera.rs `CameraWatch::add`, `reading` | 1.04 ms `camera/add` (the FFTs about 0.14 ms of it, survey); `reading` 0.3 µs `camera/reading` | its own thread or worker: off the critical path (about 2.9 s a run against about 6 s) |
| Countdown test | src/camera.rs `countdown_showing` | 1.3 µs `camera/countdown_showing`, 3 times a frame | |
| HUD watch | src/hud.rs `HudWatch::add` | 0.21 ms at 2560x1440 `hud/add_2560`, 0.32 ms at 1080p `hud/add_1920` (scaling and the range table); an earlier measure gave 0.33 ms on a busy HUD (vox) | its own thread or worker; the whole av1 review with it: 15.6 s in the browser, 11.0 s natively |

## Every key frame (25 on av1)

| Stage | Where | Cost | Notes |
| --- | --- | --- | --- |
| Contrast map | src/fixed.rs `contrast`, `walls` | 2.8 ms `fixed/contrast` (4.9 ms before each pixel read its block's wall, survey) | bit-equal to Python's; natively computed once a key frame and shared by the fixed map and the area finder (service/src/review.rs); the browser's area finder computes its own in its worker |
| The fixed map's count | src/fixed.rs `FixedMap::add` | 4.8 ms with the contrast `fixed/add` (through the 25 key frames, so it pays their memory traffic) | |
| The HUD's key frames | src/hud.rs | 34 ms for the 25 key frames and the layout `hud/keys_2560` | |

## Once a review

| Stage | Where | Cost | Notes |
| --- | --- | --- | --- |
| Link | src/track.rs `link` (from `Tracker::finish`) | av1 21 ms `track/link_av1`; 2007_1w6ts_aimlab 60 ms `track/link_2007_1w6ts_aimlab`; 10 Sphere Hipfire 69 ms `track/link_10_sphere_hipfire` | the view shift is about 2 ms of av1's link since its sorted sweep (each pairing compared only with those within 0.36 degrees in x; survey: 20 ms on the Aim Lab run against 120 ms before), so the rest of `link` costs most; grows with clutter |
| Matching | src/matching.rs `match_times`, `match_video` | `match_times` 9.6 ms `matching/match_times_av1`; `match_video` 10.0 ms `matching/match_video_av1` (8.2 and 8.5 ms before 5a66b5c) | the crosshair-end cut (5a66b5c) adds about 1.4 ms to each: it copies the tracks where the detector marks the crosshair, as on av1. `clock_offset`: 40 x 40 offsets, a nearest end per kill by binary search, 1.8 to 2.0 ms on av1 (4 to 5 ms with the linear search before, survey). `appearances` (also once more for the report's `appeared`): 3.0 ms on av1, 3.2 on 10 Sphere Hipfire, 6.1 on Bounce 180, 11.1 on Smoothbot Switch Robots; its first pass alone 1.4, 0.5, 4.0 and 7.7 ms |
| Kill check | src/kill_check.rs `KillCheck`; service/src/review.rs `check_kills` | av1 with large_v13e4 (2026-10-07): 0:20 to 0:40 8.1 s with the check against 5.9 without (10.4 before the parts), through ffmpeg 9.4 s (16.5 before); the whole video 15.5 s (16.1 before) | the frames from the first the check needs to the last, decoded again in parts at once as the review's runs are (`split_runs`), the same evidence byte for byte; before, one decoder from the video's start (about 4.6 s a run, 245 s against 181 s for 14 of the video-alone runs with large_v11). The whole video gains little: the GPU's decoder is the limit there. Only without a stats file. It is the decoding: with large_v11, 1wall 2targets xsmall (6,038 frames) 11.5 s without the check, 15.9 s with it; through ffmpeg (`--gpu-frames off`) 11.8 s and 23.4 s. Decoding the frames it does not need without converting them or reading them back from the GPU, and ffmpeg's select filter piping only those it needs, saved 0 to 0.8 s (tried 2026-10-06, not kept). Checking in the first pass is not possible as it stands: the places after a kill move with the tracks' view shift, which `link` works out at the end from every frame's boxes |
| Measure | src/measure.rs | 0.31 ms `measure/measure_av1` (0.48 ms at 9cbcbf7; already 0.31 ms just before the arrival's segment test, which costs nothing measurable) | |
| The report | src/review.rs, summary.rs | with the stats file 14.1 ms `report/clicks_av1`; from the HUD 13.4 ms `report/hud_av1`; the service's request, JSON in and out, 18.2 ms `report/json_av1`; a tracking run 2.1 ms `report/tracking_flower` | the desktop app's whole report request on av1 (66 kills) took 283 ms, the browser's for a saved review 251 ms (before the one backend): mostly reading the files and the request, not the work |
| HUD layout median | src/hud.rs, the per-pixel median over the key frames | 17 ms (survey) | a 256-bin histogram measured 11.6 ms; inside `hud/keys_2560` |
| HUD finish | src/hud.rs `HudWatch::finish` | 20 to 40 ms (an earlier measure) | |
| Camera's excluded pixels | src/camera.rs `excluded` | 4.4 ms `camera/excluded` | |
| Area finder's finish | src/areas.rs `AreaFinder::finish` | 49 ms `areas/finish_av1` (52 to 133 ms natively in an earlier measure, median 81) | |

## Per request (the review service)

| Request | Where | Cost | Notes |
| --- | --- | --- | --- |
| The recordings list | service/src/library/recordings.rs `recordings` | native: 0.04 s quick, 0.16 s full; browser, 1,701 recordings in 786 folders (a stand-in in the browser's storage): the full list 0.5 s after the quick one (2.4 s before the listing carried sizes and times) | in the browser every file call waits on the page (Asyncify): a folder's listing now carries each file's size and time, and one look at the data folder tells which recordings can have a review or a chosen stats file, so a recording without one costs no call |
| KovaaK's stats index | service/src/library/stats.rs `with_stats` | lists the stats folder once (more than 70,000 files) | in the browser the files' times are read later, per scenario, in `history` |
