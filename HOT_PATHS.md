# Hot paths

Where a review spends its time, stage by stage, with how often each stage runs and what it costs. Check this before
optimizing anything, and update a row when a change or a measurement moves it. The benchmarks that time these, and
their baselines, are in `BENCH.md`.

The numbers are for av1 (2560x1440, about 6,000 frames at 60 fps, 25 key frames) unless a row says otherwise. The ones
marked "survey" come from a scratch bench at 9eef1f4 on 2026-10-04, run while another build was going (about ±30%).

## Every frame (about 6,000 a review)

The review is a pipe: decode, convert, detect, track. The slowest stage sets the pace. Two runs are reviewed at once
on a recording of 1,200 frames or more (src/session.rs), so two decoders work in parallel.

| Stage | Where | Cost a frame | Notes |
| --- | --- | --- | --- |
| Decode | ffmpeg through a pipe (service/src/review.rs); Mediabunny and the browser's software decoder (modes/wasm/review.worker.ts) | browser: one decoder about 430 frames a second | the browser's limit, which is why runs are split |
| Convert to 720p RGB and YUV | src/convert.rs | 2:1: 0.29 ms native (AVX2), 0.93 ms browser (SIMD); other sizes through swscale's pipeline: 1080p 3.3 ms native | byte-equal to ffmpeg; filters of 1 to 4 taps, nothing to gain from a transform |
| Detector | ONNX Runtime (service/src/detector.rs); onnxruntime-web (review.worker.ts) | full_v3: 1.84 ms native (DirectML); browser about 380 frames a second (WebGPU, 4 frames a call) | the biggest single cost; the CPU is 31 frames a second in the browser |
| Keep and pop-up areas | src/track.rs `keep`, src/popup.rs | small (not measured on its own) | |
| Camera watch | src/camera.rs `CameraWatch::add`, `reading` | 0.95 ms (survey), of which the FFTs are 0.14 ms | its own thread or worker: off the critical path (about 2.9 s a run against about 6 s) |
| Countdown test | src/camera.rs `countdown_showing` | 3 times a frame, 84 samples | |
| HUD watch | src/hud.rs `HudWatch::add` | nothing measurable (the whole av1 review with it: 15.6 s in the browser, 11.0 s natively) | |

## Every key frame (25 on av1)

| Stage | Where | Cost | Notes |
| --- | --- | --- | --- |
| Contrast map for the fixed map | src/fixed.rs `contrast`, `blur_up`; `FixedMap::add` | 4.9 ms a key frame (survey) | the upsampling divides per pixel; reading the block directly measured 2.9 ms, bit-equal |
| The same contrast for the area finder | src/areas.rs `AreaFinder::add` | 4.9 ms a key frame (survey) | natively computed a second time on the same frame with the same counts; the browser's area finder runs in its own worker |

## Once a review

| Stage | Where | Cost | Notes |
| --- | --- | --- | --- |
| Link: the view shift | src/track.rs `view_shift` (in `link`, from `Tracker::finish`) | av1 1.2 to 1.6 ms; 10 Sphere Hipfire 46 to 49 ms; 2007_1w6ts_aimlab 118 to 122 ms (survey) | O(d^2) over pairs of spots; a sorted sweep measured 20 ms on the Aim Lab run, bit-equal; grows with clutter |
| Matching | src/matching.rs `match_times` | 9 ms on av1 (survey) | `clock_offset` is 4 to 5 ms of it (40 x 40 offsets, a linear nearest search per kill); a binary search measured 1.8 to 2.0 ms |
| HUD layout median | src/hud.rs, the per-pixel median over the key frames | 17 ms (survey) | a 256-bin histogram measured 11.6 ms |
| Camera's excluded pixels | src/camera.rs `excluded` | 4 to 7 ms (survey) | |

## Per request (the review service)

| Request | Where | Cost | Notes |
| --- | --- | --- | --- |
| The recordings list | service/src/library/recordings.rs `recordings` | native: 0.04 s quick, 0.16 s full | in the browser every file call waits on the page (Asyncify): about 5 calls a recording for the full list (metadata twice, the pairing, the review check) |
| KovaaK's stats index | service/src/library/stats.rs `with_stats` | lists the stats folder once (more than 70,000 files) | in the browser the files' times are read later, per scenario, in `history` |
