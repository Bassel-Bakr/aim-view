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
| Decode | ffmpeg through a pipe (service/src/review.rs); Mediabunny and the browser's software decoder (modes/wasm/review.worker.ts) | browser: about 2.3 ms for one decoder (430 frames a second) | the browser's limit, which is why runs are split |
| Convert to 720p RGB and YUV | src/convert.rs | 2:1: 0.29 ms native (AVX2), 0.93 ms browser (SIMD); other sizes through swscale's pipeline: 1080p 3.3 ms native | byte-equal to ffmpeg; filters of 1 to 4 taps, nothing to gain from a transform |
| Detector | ONNX Runtime (service/src/detector.rs); onnxruntime-web (review.worker.ts) | full_v3: 1.84 ms native (DirectML); in the browser the whole pipeline runs at about 2.6 ms a frame (380 frames a second, WebGPU, 4 frames a call) | the biggest single cost; on the CPU the browser takes about 32 ms a frame (31 frames a second) |
| Keep and pop-up areas | src/track.rs `keep`, src/popup.rs | no time kept | |
| Camera watch | src/camera.rs `CameraWatch::add`, `reading` | 0.95 ms (survey), of which the FFTs are 0.14 ms | its own thread or worker: off the critical path (about 2.9 s a run against about 6 s) |
| Countdown test | src/camera.rs `countdown_showing` | no time kept; 3 times a frame, 84 samples | |
| HUD watch | src/hud.rs `HudWatch::add` | 0.19 ms at 2560x1440 (SCS 4L), 0.33 ms on a busy HUD (vox), 0.31 ms at 1080p (scaling and the range table) | its own thread or worker; the whole av1 review with it: 15.6 s in the browser, 11.0 s natively |

## Every key frame (25 on av1)

| Stage | Where | Cost | Notes |
| --- | --- | --- | --- |
| Contrast map for the fixed map | src/fixed.rs `contrast`, `walls`; `FixedMap::add` | 2.9 ms a key frame (survey, reading each pixel's block directly; 4.9 ms before, with full-size upsampled walls) | bit-equal to Python's |
| The same contrast for the area finder | src/areas.rs `AreaFinder::add`, `add_contrast` | natively none: the review shares the fixed map's (service/src/review.rs); the browser's area finder computes its own in its worker | an earlier measure of the whole `add`: 4.8 to 12.5 ms a frame natively, median 5.6, before both changes |

## Once a review

| Stage | Where | Cost | Notes |
| --- | --- | --- | --- |
| Link: the view shift | src/track.rs `view_shift` (in `link`, from `Tracker::finish`) | a sorted sweep (survey): 2007_1w6ts_aimlab 20 ms, 10 Sphere Hipfire 24 to 27 ms, av1 1.4 to 2.1 ms; before it, all pairs: 118 to 122, 46 to 49 and 1.2 to 1.6 ms | each pairing is compared only with those within 0.36 degrees in x (a binary search in the pairings sorted by x); grows with clutter |
| Matching | src/matching.rs `match_times` | 9 ms on av1 before `clock_offset`'s binary search (survey) | `clock_offset`: 40 x 40 offsets, a nearest end per kill by binary search, 1.8 to 2.0 ms on av1 (4 to 5 ms with the linear search before) |
| HUD layout median | src/hud.rs, the per-pixel median over the key frames | 17 ms (survey) | a 256-bin histogram measured 11.6 ms |
| Camera's excluded pixels | src/camera.rs `excluded` | 4 to 7 ms (survey) | |
| HUD layout and finish | src/hud.rs `HudWatch`: the layout at the first frame, `finish` | 20 to 40 ms each | |
| Area finder's finish | src/areas.rs `AreaFinder::finish` | 52 to 133 ms natively, median 81 | |
| Measuring and the report | src/review.rs, measure.rs, summary.rs (the service's report request) | av1 (66 kills): 283 ms in the desktop app, from the request to the report; 251 ms in the browser for a saved review (before the one backend) | |

## Per request (the review service)

| Request | Where | Cost | Notes |
| --- | --- | --- | --- |
| The recordings list | service/src/library/recordings.rs `recordings` | native: 0.04 s quick, 0.16 s full; browser, 1,701 recordings in 786 folders (a stand-in in the browser's storage): the full list 0.5 s after the quick one (2.4 s before the listing carried sizes and times) | in the browser every file call waits on the page (Asyncify): a folder's listing now carries each file's size and time, and one look at the data folder tells which recordings can have a review or a chosen stats file, so a recording without one costs no call |
| KovaaK's stats index | service/src/library/stats.rs `with_stats` | lists the stats folder once (more than 70,000 files) | in the browser the files' times are read later, per scenario, in `history` |
