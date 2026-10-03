# Aim View: agent guide

Aim View reviews aim trainer recordings. The review core is Rust (`src/`), the shared service that answers the review
API is `service/` (the desktop app and the Rust server serve it; Python's scripts reach it through its `aimview-tool`),
the UI is Angular (`ui/`), and
`python/model/` trains the target detector. `python/review.py` is the old Python pipeline, kept as the parity tests'
reference. Read `README.md` first, then `python/README.md` (how the review works) and
`python/model/MODEL_STATUS.md` (the detector's results and limits). Every command to rebuild the detector is in
`python/model/REPRODUCE.md`.

It was copied from the Flow Fix project (`D:\Projects\flowfix`, folder `vod/`) on 2026-10-02, with its caches and
training data in `test_out/` (ignored by git). Its `vod/` folder became `python/` here. The planned stack and layout
are in `README.md` ("Where it's going").

## Commands

```bash
bun run server                                 # the review server (server/, aimview-server), http://127.0.0.1:8770/: the
                                               # server-mode build at /, the old page at /old/ (bun run build:server
                                               # first; flags or aimview-server.toml: server/README.md)
cargo run -q --release -p aimview-service --bin aimview-tool -- help   # the library and the native review for
                                               # scripts, JSON on stdout (python/aimview_tools.py runs it)
python python/model/test_model.py              # the detector's tests
python python/model/eval_vods.py <model>       # static runs against their stats files (the app's native review of the
                                               # model's _u8in export; --python: python/review.py with a .pt or .onnx)
python python/model/eval_moving.py name=<model> ...      # every scenario kind against the stats files (--python too)
python python/model/eval_video_alone.py [model]  # the video-alone kill finder on 48 runs against their stats files
                                               # (tracks kept per model; --retrack after a change to the tracking)
python python/model/accept.py <name> [--list]  # the acceptance gate: the contract and the three checks above against
                                               # the best model's; --list adds a passing model to models.json
bun run dev                                    # the Angular UI in browser mode, http://localhost:4200/
bun run dev:server                             # the same in server mode (needs the server above)
bun run build                                  # every mode's build: ui/dist/browser, server, desktop
bun run app                                    # the desktop app (Tauri 2, desktop/) on the desktop build's dev server
bun run build:app                              # its installer: target/release/bundle/nsis/
bun run test:ui                                # the UI's tests
bun run lint:ui                                # ESLint (angular-eslint's recommended set, plus the rules below)
bun run format                                 # Prettier, over ui/
cargo test --profile quick                     # the Rust core, checked against Python's results (test_out/parity/)
                                               # (--release gives the same results; its builds take 40 s, quick's 3 s)
python tests/fixtures.py <video> [--areas exclude.json]   # Python's results stage by stage, for those checks
python tests/fixtures.py --faint                          # the faint-target cut-off in Python (--test faint_parity)
bun run assets                                 # the core as WebAssembly, the models and the area finder's data,
                                               # into ui/generated/ (--no-data: without the data; see below)
```

Paths: recordings in `E:\OBS\KovOBS` (one folder per scenario); KovaaK's stats in
`C:\Program Files (x86)\Steam\steamapps\common\FPSAimTrainer\FPSAimTrainer\stats` (more than 70k CSVs: use a Python
glob, not `ls`); scenarios in `...\FPSAimTrainer\Saved\SaveGames\Scenarios`.

Browser mode starts from the user's latest training data. `bun run assets` copies the area finder's examples and
types (`test_out/vod_app/area_examples.jsonl`, `area_kinds.json`) into `ui/generated/data/`, and `AreaExamples` starts
from them: a recording's examples learnt or loaded in the browser replace its shipped ones, and a type changed in the
browser replaces the shipped type with the same id. The examples name the user's recordings, so a build for others
must not ship them. `bun run assets` and the `dev*` scripts copy them (`--no-data` leaves them out). `--release`, which
every `build:*` script uses, leaves them out unless `--data` is given. For a browser build with them, run
`bun run assets --release --data`, then `bun run --cwd ui build --configuration production,browser`. The browser runs
every model `python/model/models.json` lists, from its `_u8in` export in `python/model/exports/`. A newly trained model
shows up once it is exported (`python python/model/export.py <best.pt>`, or `--u8in` for that file only), listed in
models.json, and `bun run assets` has run again; `assets` names any listed model with no export.

## Working with this user

- **Commits.** Conventional Commits (`feat:`, `fix:`, `docs:`, `refactor:`, `chore:`, with an optional scope). Commit
  only when the user asks, on the branch that is checked out. Never create a branch unless asked.
- **Ask before changing.** Say what you will change and wait for a yes; answer questions directly. For clear requests
  the user wants the recommended option done without asking; ask when a request is unclear.
- **Large jobs need an explicit go.** Training a model, long GPU runs or a new subsystem start only after the user says
  so.
- **Correctness first.** The user wants near-100% accuracy, even at the cost of speed. Judge a detector by the
  stats-file checks (`eval_vods.py`, `eval_moving.py`, `eval_video_alone.py`), not only crop scores.
- **Bun for JavaScript** tools, not Node.
- **Angular for speed.** No zone.js. OnPush everywhere (Angular 22's default: never set `Eager`). Prefer signals for
  state (RxJS is allowed where it fits better). Data comes through `HttpClient` (which sends with `fetch`, Angular's
  default), as `httpResource()` for reads, so every request passes the interceptors in `app.config.ts`; tests answer
  requests with `provideHttpClientTesting()` (`fake-api.ts`). Prefer signal forms (`@angular/forms/signals`); the lint
  config warns on the older forms. Anything that changes every frame (the video overlay,
  timelines) is drawn on a canvas in `requestAnimationFrame` or `requestVideoFrameCallback`, never through a template.
  A resource's `value()` throws in its error state: check `error()` or `hasValue()` first.
- **Three modes, one app.** The UI runs in browser mode (everything in the browser), server mode (the review server
  does the work) or desktop mode (Tauri 2; the browser mode's services until `desktop/` exists). Features and
  `services/` inject only the contracts in `ui/src/app/platform/` (`RecordingSource`, `StatsFiles`, `ReviewEngine`,
  `ModelCatalog`), never `/api` or a mode's class. The implementations are in `ui/src/app/modes/`, in folders named
  for what they wrap (`http/`, `web-files/`, `wasm/`, later `tauri/`), and each `mode.<name>.ts` picks one per
  contract. The build configurations (`browser`, `server`, `desktop`) swap `modes/mode.ts` for it, so a build
  carries only its own mode's code. Each contract has one spec that runs against every mode
  (`platform/*.spec.ts`).
- **Named types.** In TypeScript, every object or tuple type gets a name (an interface or a type alias). No inline
  anonymous types such as `{ gpu: number; cpu: number }` in a field or a signature. ESLint enforces it.
- **Styles are SCSS, and every design value is a token.** A token is a CSS variable (so it can be edited live in the
  browser), with an SCSS name for it: `$surface-0: var(--surface-0)`. The main tokens are in `ui/src/themes/theme.scss`;
  page and module tokens (values only one part uses) are in `ui/src/themes/<page or module>.scss`. Each file has a
  `tokens` mixin, which `styles.scss` includes in `:root`. Component styles `@use 'themes/...'` and use only `$tokens`:
  no raw colors, sizes, spaces, fonts or durations. Keywords and layout values (`flex`, `solid`, `0`, `100%`, `1fr`)
  are fine. Canvas drawings read their colors and fonts from the same CSS variables.
- **Tailwind on the tokens.** `ui/src/tailwind.css` maps Tailwind 4's theme onto the tokens (`@theme inline
  reference`, with Tailwind's own scales switched off), so a class can only reach a token: `bg-surface-1`, `text-muted`,
  `p-4` (4 x `--space-1`), `w-(--sidebar-width)`. No arbitrary values such as `p-[13px]`. Page-wide element styles are
  in `@layer base`, named classes in `@layer components`, so a utility on an element always wins.
- **Named classes in SCSS, variants as data attributes.** Each part of a component is a class in its own SCSS
  (`.screen { @apply relative overflow-hidden rounded-lg; }`, scoped by Angular's view encapsulation), and the
  template names it (`class="screen"`). A component's SCSS starts with `@reference` to `tailwind.css`, so `@apply`
  reaches the token theme: Sass compiles first, then Tailwind. The shared controls are global classes in
  `ui/src/themes/controls.scss` (`.button`, `.badge`, `.chip`, `.card`, `.pill`, `.segmented`, `.switch`,
  `.section-note`, `.color-swatch`). A variant is a data attribute (`&[data-intent='primary']`), set by the control's
  directive in `ui/src/app/controls/` from a typed input (`<button appButton intent="primary">`, `<span appBadge
  tone="good">`), so templates get completion and type checks. A variant's selector is more specific than the base,
  so it overrides it with no class merging. A class name must not be a Tailwind utility (`table`, `grid`, `hidden`):
  Tailwind would add the utility too. The old tailwind-variants modules are in `ui/retired/themes/`.
- **Angular's style guide** (angular.dev/style-guide, the 2025 one, still current in Angular 22). Folders by feature
  (`recordings/`, `run/`, with each part in its own folder); a service that more than one feature uses goes in
  `services/`, a service only one feature uses stays in that feature. File names follow their class (`stamp-pipe.ts`
  for `StampPipe`), never generic (`utils.ts`, `helpers.ts`). Event handlers are named for what they do
  (`selectRow`, not `onClick`). `inject()`, `protected` for template-only members, `readonly` for inputs and queries.
- **Format and lint** the UI before calling a change done: `bun run format`, then `bun run lint:ui`.
- **Spelling and style.** Write "center", not "centre". Docs in plain, simple English: short sentences, active voice,
  no arrows.
- **Nothing gets deleted.** Old files move to a `retired/` folder; the user's own data is never overwritten.
- **Data the user labelled** (in `test_out/`): hand-labelled crops (`test_out/vod_model/hand/`), area labels and types
  (`test_out/vod_app/area_examples.jsonl`, `area_kinds.json`), cut-offs (`faint.json`), run marks (`run.json`) and the
  stats file picked for it (`stats.json`) per recording. Keep them.

## State (2026-10-02)

- The detector is full_v3 (`infer.BEST`), trained on every scenario kind; small_v13 is the small one for speed.
- The plan: the app runs three ways from one code base (browser only, browser with the review server, desktop). The UI
  is Angular 22 and carries the redesign from the 2026-10-02 mockup. The review core is Rust, built natively for the
  desktop app (Tauri 2) and as WebAssembly for the browser. The ground truth is KovaaK's stats files, not Python: a
  new feature is checked against them (for a run without one, on runs that have one, with the file left out). Python's
  review stays a cross-check while it lasts; `python/model/` stays for training the detector. The Python server has
  retired (the Rust server serves server mode); the old page retires once the Angular app does what it does.
- Done: the layout (`python/`, the Rust crate at the root, `ui/`). In `ui/`: the recordings list, and the run page
  (review button and progress, the video with its overlay, seek bar, controls, keys, and a tracking run's timeline),
  and both reports (a clicking run's cards, time budget, checks, tables, flick list and speed chart; a tracking run's
  cards, how the bot was followed, and the what-if estimates; for both, "The run at a glance": a clicking run's TTKs
  (times to kill), distance against TTK, where the clicks landed and every flick's speed, a tracking run's time on the
  bot 10 s at a time, distance from its center line, and where the crosshair sat around it, from the motion's
  per-frame `around` offsets), and the fastest-path analysis (Pathing per kill, the
  Pathing check, the fastest and your-path overlays; checked equal to the old page on 1wall 6targets 889.26), the
  model panel, upload and the stats file panel, and the three modes. Browser mode: files added stay in the browser
  (a video that is not an MP4 is remuxed into one with Mediabunny, streams copied; a stats .csv is read there). The
  user opens a folder of recordings (VODs folder: Chrome's folder picker, remembered across visits; a recording's id
  is its path there, and a non-MP4 one is remuxed when it is first opened), and Clear list empties the list. KovaaK's
  folders are chosen as files (Chrome's picker refuses folders under Program Files): with Stats folder in the top bar,
  in the stats file panel, or on the run page when the review needs them. FPSAimTrainer gives the stats and the
  user's scenarios, workshop\content\824270 the workshop's. The browser keeps a copy of the stats files (IndexedDB,
  `StatsCache`; choosing the folder again copies only new or changed files), each VOD's stats file, and the scenario
  facts. The stats folder lets each run find its stats file by name and time. The scenario folders
  give each scenario's kind, time limit and target count: the review waits for them unless the scenario is in neither
  folder.
  Server mode: files added are sent to the server, and the stats file panel lists KovaaK's stats files (`/api/stats`).
- The review in the browser (ui/src/app/modes/wasm/, the Rust core in src/). The track step runs in a worker
  (review.worker.ts): decode (Mediabunny and the browser's decoder, its software one where it has one: the same YUV
  as ffmpeg once the edit list's pre-roll, the frames before time 0, is skipped), ffmpeg's exact `scale=1280:720:flags=area` to RGB and YUV
  (src/convert.rs, byte for byte), the fixed map, the detector (onnxruntime-web, within 0.00002 px of ONNX Runtime on
  the CPU), `keep`, pop-up areas (`AreaWatch`) and `link`. All are equal to Python's to the bit except the detector's
  float noise (16 of 6,038 frames differ by one pixel of area on the CPU, 27 on the GPU). full_v3 on av1
  (2560x1440): 380 frames a second with the detector on the GPU (WebGPU, the default, 4 frames in each call, its
  outputs read back while the next call is sent; 15.9 s for the whole review), 31 on the CPU (one frame a call; measured before the camera worker;
  test_out/browser_check/profile.html times each stage). On the GPU, with 8 threads or more, a recording of 1,200
  frames or more is split at the key frame nearest its middle into two runs (split-runs.ts), each with its own review
  and camera workers, so two software decoders work at once (one was the limit: about 430 frames a second on av1;
  test_out/browser_check/decode-bench.html). Each run's tracker and camera watch hand back parts (src/wasm.rs:
  `tracker_part`, `camera_part`; the area watch counts from the run's first frame, and a run reads the next run's first
  frame for the camera's turn into it), which the page joins (core-module.ts: `joinRuns`). Two runs give the same
  tracks and readings as one, byte for byte, on av1, flower, h264 and hevc (`popup` and `camera_same` test the joins). On the GPU the session uses graph capture (onnxruntime records
  the model's GPU work once and replays it), NHWC convolutions, no extra validation and a fixed input size
  (review.worker.ts, `gpuOptions`); graph capture needs every node on the GPU, so the _u8in exports cast the fixed map
  to float before its Unsqueeze (onnxruntime's WebGPU build has none for uint8). The model panel lets the user pick the frames at once (1, 2,
  4, 8), kept for each of GPU and CPU: machines differ. The core is built with WebAssembly SIMD (.cargo/config.toml); the 2:1 RGB conversion takes 16 pixels at a time
  there (0.93 ms a frame, the same bytes: test_out/browser_check/rgb-bench.html), and 32 at a time natively where
  the CPU has AVX2 (0.29 ms; `avx2_rows_give_the_tables` checks every value against the tables). Other sizes go through
  swscale's full pipeline, its buffers kept from frame to frame and its kernel picked once a row: 1080p to RGB in 3.3
  ms natively (8.5 ms before), h264_1920_tv's review 14.0 s in the browser (24.5 s before), the same bytes
  (`convert_parity`).
  The clicking
  review with a stats file (src/stats_file.rs, matching.rs, measure.rs, summary.rs, review.rs) runs on the page and
  gives the report. On the test runs it equals Python's: every kill, frame, count and check text, and every number
  within 1e-9 (`cargo test --release --test review_parity`). After the track step the core uses plain floating point:
  it copies Python's logic, not its last bits. Tracking runs with a stats file too: the review worker sends each
  frame to the camera worker (camera.worker.ts, src/camera.rs: the camera's turn by phase correlation, and KovaaK's
  countdown bar), which runs beside it so the detector never waits for it: it gets the decoded Y plane and the rows of
  the RGB the countdown test reads, and makes only the 720p luma (the same bytes as the Y of yuv420p;
  `--test camera_same` checks the watch's shifts to the bit), and the page's core gives
  the summary (src/tracking.rs). Equal to Python's on 5 tracking runs (`--test tracking_parity`), the camera within
  1e-8 degrees on the same frames (`--test camera_parity`). Two inputs differ from Python's on purpose: the camera
  reads the frame's Y plane, where Python reads ffmpeg's `format=gray` (which goes through the colors: readings differ
  by a median 0.001 degrees), and the countdown test does not depend on the HUD color (Python's looks for teal only).
  The run window (the run page's Run window: start and end, typed or from the playhead) works in every mode: the server
  keeps it as run.json and measures again; the browser (SavedMarks) and the desktop app (run_window.rs, run.json) also
  track only the window with a second either side (split-runs.ts `trackedWindow`, `windowFrames`: from the key frame
  before it; the first run's tracker and camera watch start part way in, and the joins fill the frames before it with
  empty ones), and track again when a new window reaches past the tracked one. The core measures a tracking run from it
  as Python does (review.rs `run_window`). av1 with 0:20 to 0:40: the same boxes and camera readings inside the window
  as the whole review, 7.2 s against 14.2 s in the browser, 6.7 s against 10.4 s natively.
  Runs without a stats file, in the browser and the desktop app: every review also reads the HUD (src/hud.rs: KovaaK's
  session box, else Aim Lab's POINTS and TIME boxes, the digits learned from the recording) beside the camera watch,
  from each frame's Y plane; the runs' HUD parts are joined like the camera's. Without a stats file the kills, shots
  and hits come from the HUD, else from the video alone (matching.rs `match_video`; review.rs `KillTimes`), and the
  run page says which. Checked against stats files: on 27 recordings the HUD's kill count equals the file's on all
  27, its hits and shots on 22 (the rest: a lightning gun's last redraw, a bot whose hits KovaaK counts apart, a last
  miss after the last redraw), its kill frames within a frame on 25. The video alone (matching.rs `match_video`, its
  `Paths`: a false camera turn at a kill repaired, a target found again only where and as big as it was) finds 94.5%
  of the stats files' kills within 3 frames on 47 runs (97.3% on the 18 held out; was 83%), precision 95.5% (held out
  96.7%); switching runs are the weakest (90% precision held out). The benchmark and its notes: the session scratchpad's vbench/ (2026-10-04). The HUD costs nothing measurable (av1: 15.6 s in the browser, 11.0 s natively). `examples/hud.rs` reads a
  recording's HUD; `examples/review.rs` reviews one request. A review keeps the version that made it (src/track.rs
  `REVIEW_VERSION`, 2 since the HUD): a report from an older one says `outdated` and the run page asks for a new
  review.
  The faint-target cut-off (the run page's Cut-off and the top bar's Cut-off queue; platform/faint-cutoffs.ts, the
  core's src/faint.rs) works in every mode: the browser keeps it as SavedFaint, and the core measures a tracking run
  without the tracks it cuts (`faint` in the review request: equal to Python's on 5 tracking runs at 3 offsets,
  `--test faint_parity`). A submit's labels in the browser are the crops Python picks (its random numbers, seeded
  alike: the same files, boxes and rows), each read from its own frame (Python's ffmpeg -ss lands a frame late on 6 of
  flower's 20), kept in the browser and downloaded as cutoff.zip (checked.jsonl and train/).
  The excluded areas (the run page's Excluded areas editor: draw, move, type; Find areas and Detect fresh; KovOBS's
  layout; area types; platform/area-labels.ts) work in every mode, and a review tracks with the recording's areas
  (tracked again when they change). The area finder (src/areas.rs, python/areas.py's port) reads the key frames the
  review decodes for the fixed map (90 frames over the run when it has fewer than 24; the browser reads them in a
  worker of its own, area-finder.worker.ts, after the review or for Find areas without one, and keeps what it found),
  and learns from saved areas: equal to Python's on 17 recordings (every area, map and kind) and on the 1,788
  examples' leave-one-out. An area of type challenge_results (the end screen, at the end or between runs) is left out only while it shows (src/popup.rs
  `END_SCREEN`; left out all the time it hid VT FlyTS: 0 of 5 kills, now 5 of 5). The labelling tools (the area queue,
  Skip, Not an aim trainer; platform/labelling.ts) too; in the browser the examples and types are kept there and load
  from and download to area_examples.jsonl and area_kinds.json. The raw mouse log (src/mouse.rs, mouse_read.py's port,
  equal to it on 31 logs, `--test mouse_parity`): the browser reads a log the user adds; the desktop app logs in the
  background (a switch in the top bar; desktop/src/mouse.rs, Windows raw input) and finds a run's log itself; the run
  page shows the measures. Windows throttles a background logger to about 125 events a second unless
  RawMouseThrottleEnabled is 0. Measured (stats files): the user's areas against KovOBS's change nothing on 14 of 15
  runs; no exclusion is worse on uploads; the cut-off does not improve accuracy.
- The service (service/, aimview-service): the review server's API without Tauri, `api::handle` over a `Library`
  opened with a `Config` (the data folder in the app's layout or Python's test_out/ layout, the VODs, stats and
  scenario folders, the models, the device: DirectML, CUDA behind the `cuda` feature, or the CPU). Two servers serve
  it: the desktop app over its own protocol, and server/ (aimview-server: HTTP, the server-mode UI build, the old page
  at /old/, a token for anything beyond this machine). Python's scripts (areas.py, model/build_kills.py, eval_vods.py,
  eval_moving.py, tests/find_popups.py, tests/fixtures.py) use it through its command-line tool, aimview-tool
  (service/src/bin/: `recordings`, `lookup`, `review`, JSON on stdout), which python/aimview_tools.py runs (cargo run
  --release, so a Rust change is built first). The Python bindings and the thin Python server over them retired
  (retired/python-bindings/, python/retired/server_thin.py; the old Python server is python/retired/server.py); the
  tool gives what the bindings gave, checked on every recording's pairing and on two native reviews, byte for byte.
  Checked against the old Python server on a copy of test_out: the same answers and byte-equal files (scratchpad
  apicheck scripts).
- The desktop app (desktop/, Tauri 2): the desktop build in a WebView2 window, with the server mode's services
  (modes/tauri/: their requests go to http://api.localhost). The app answers the review server's API itself
  (desktop/src/protocol.rs over a custom protocol, to the service: no network port, nothing outside the app reaches
  it; the service's library: the VODs folder chosen in the system's dialog, KovaaK's stats files and scenarios read from disk, the models it ships, each
  recording's reviews in the app's data folder, the report worked out by the core as the browser does). The review
  runs natively (service/src/review.rs): ffmpeg's frames through a pipe (the video's own YUV, as python/review.py reads them), the
  core, ONNX Runtime with DirectML (1.84 ms a frame for full_v3; the CPU when there is no GPU), split into two runs
  as in the browser. av1: 12.7 s in the app (the browser 16 s), 20 frames apart from Python's (GPU noise), the same 66
  kills. `cargo run -p aimview-service --release --example track -- <video> <model> <out>` reviews without the app.
  ffmpeg is not shipped: as KovOBS does, the review uses the PATH's ffmpeg and ffprobe when both run, else the first
  review downloads them into the app's local data folder (service/src/
  ffmpeg.rs, ffmpeg-sidecar), from BtbN's GPL build, which has dav1d (gyan.dev's essentials build decodes AV1 with
  libaom: 2.5 times slower, and it ignores `-skip_frame nokey`, so the fixed map decodes each key frame on its own;
  python/review.py still uses `-skip_frame` and would break the same way on such an ffmpeg). The installer ships what
  the review needs beside the exe (desktop/installer-hooks.nsh): DirectML.dll (the ort crate's, newer than Windows'
  own) and the VC++ runtime ONNX Runtime loads (msvcp140, msvcp140_1, vcruntime140, vcruntime140_1; desktop/build.rs
  copies them from the newest Visual Studio). ONNX Runtime is linked into the exe. Checked: installed silently into
  a folder, the app reviewed a recording on DirectML with every one of these DLLs loaded from that folder.
- The Angular app does everything the old page (`python/app/`, now at /old/ on the Rust server) did, the player's
  full screen (F, Escape) included; the old page stays until the user retires it. Server mode stays (a stronger
  machine can run the reviews). Training (`python/model/`) stays in Python; eval_vods.py and eval_moving.py review
  through the app's native pipeline by default (--python: the old one).
- Open: showing as much information as possible (after the redesign); moving targets on themes other than
  dark-on-light; thin capsules; tiled-wall seams; hand-checked ground truth; wiring the detector into KovOBS (the
  Rust prototype in `python/model/rust/` becomes the start of the core's detector); with a stats file, the flick paths
  and the count of flicks measured could gain from the video-alone finder's repairs (matching.rs `Paths`: a false camera
  turn at a kill, a target merged with the next): untested; measure on the stats-file runs (Smoothbot Switch Robots:
  41 flicks for 54 kills); the camera's turn at a kill fixed at its source (src/track.rs `view_shift` lines another
  target up with a dead one on a plain wall), for every review; the video-alone benchmark has only 8 switching runs
  (VT DriftTS breaks into many tracks); the UI does not show the device a review ran on (/api/job `device`); a partial
  upload a server crash leaves (`.incoming-*.part` in the uploads folder) is never cleaned up; the flick table (16
  columns, 1,190 px) scrolls in its box at every width; at 1024 px the run's title is squeezed by the toolbar;
  python/model/infer.py still uses the fixed threshold, not the model's settings file (the `--python` path).
