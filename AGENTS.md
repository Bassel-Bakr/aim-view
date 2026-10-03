# aimview: agent guide

aimview reviews aim trainer recordings: `python/review.py` is the pipeline, `python/server.py` and `python/app/` the web app,
`python/model/` the trained target detector. Read `README.md` first, then `python/README.md` (how the review works) and
`python/model/MODEL_STATUS.md` (the detector's results and limits). Every command to rebuild the detector is in
`python/model/REPRODUCE.md`.

It was copied from the Flow Fix project (`D:\Projects\flowfix`, folder `vod/`) on 2026-10-02, with its caches and
training data in `test_out/` (ignored by git). Its `vod/` folder became `python/` here. The planned stack and layout
are in `README.md` ("Where it's going").

## Commands

```bash
python python/server.py --port 8770            # the review app, http://127.0.0.1:8770/
python python/model/test_model.py              # the detector's tests
python python/model/eval_vods.py <model.pt>    # static runs against their stats files
python python/model/eval_moving.py name=<model.pt> ...   # every scenario kind against the stats files
bun run dev                                    # the Angular UI in browser mode, http://localhost:4200/
bun run dev:server                             # the same in server mode (needs the server above)
bun run build                                  # every mode's build: ui/dist/browser, server, desktop
bun run test:ui                                # the UI's tests
bun run lint:ui                                # ESLint (angular-eslint's recommended set, plus the rules below)
bun run format                                 # Prettier, over ui/
cargo test --release                           # the Rust core, checked against Python's results (test_out/parity/)
python tests/fixtures.py <video> [--areas exclude.json]   # Python's results stage by stage, for those checks
bun run assets                                 # the core as WebAssembly and the models, into ui/generated/
```

Paths: recordings in `E:\OBS\KovOBS` (one folder per scenario); KovaaK's stats in
`C:\Program Files (x86)\Steam\steamapps\common\FPSAimTrainer\FPSAimTrainer\stats` (more than 70k CSVs: use a Python
glob, not `ls`); scenarios in `...\FPSAimTrainer\Saved\SaveGames\Scenarios`.

## Working with this user

- **Commits.** Conventional Commits (`feat:`, `fix:`, `docs:`, `refactor:`, `chore:`, with an optional scope). Commit
  only when the user asks, on the branch that is checked out. Never create a branch unless asked.
- **Ask before changing.** Say what you will change and wait for a yes; answer questions directly. For clear requests
  the user wants the recommended option done without asking; ask when a request is unclear.
- **Large jobs need an explicit go.** Training a model, long GPU runs or a new subsystem start only after the user says
  so.
- **Correctness first.** The user wants near-100% accuracy, even at the cost of speed. Judge a detector by the
  stats-file checks (`eval_vods.py`, `eval_moving.py`), not only crop scores.
- **Bun for JavaScript** tools, not Node.
- **Angular for speed.** No zone.js. OnPush everywhere (Angular 22's default: never set `Eager`). Prefer signals for
  state (RxJS is allowed where it fits better). Data comes through `HttpClient` (which sends with `fetch`, Angular's
  default), as `httpResource()` for reads, so every request passes the interceptors in `app.config.ts`; tests answer
  requests with `provideHttpClientTesting()` (`fake-api.ts`). Prefer signal forms (`@angular/forms/signals`); the lint
  config warns on the older forms. Anything that changes every frame (the video overlay,
  timelines) is drawn on a canvas in `requestAnimationFrame` or `requestVideoFrameCallback`, never through a template.
  A resource's `value()` throws in its error state: check `error()` or `hasValue()` first.
- **Three modes, one app.** The UI runs in browser mode (everything in the browser), server mode (the Python server
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
- The plan: the app runs three ways from one code base (browser only, browser with the Python server, desktop). The UI
  is Angular 22 and carries the redesign from the 2026-10-02 mockup. The review core is Rust, built natively for the
  desktop app (Tauri 2) and as WebAssembly for the browser. Python stays the reference: the core replaces nothing
  until its reports match Python's on every recording.
- Done: the layout (`python/`, the Rust crate at the root, `ui/`). In `ui/`: the recordings list, and the run page
  (review button and progress, the video with its overlay, seek bar, controls, keys, and a tracking run's timeline),
  and both reports (a clicking run's cards, time budget, checks, tables, flick list and speed chart; a tracking run's
  cards, how the bot was followed, and the what-if estimates; for both, "The run at a glance": a clicking run's kill
  times, distance against kill time, where the clicks landed and every flick's speed, a tracking run's time on the
  bot 10 s at a time, distance from its center line, and where the crosshair sat around it, from the motion's
  per-frame `around` offsets), and the fastest-path analysis (path cost per kill, the
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
  (2560x1440): 178 frames a second with the detector on the GPU (WebGPU, the default, 4 frames in each call, its
  outputs read back while the next call is sent; 32.2 s for the whole review), 31 on the CPU (one frame a call; measured before the camera worker;
  test_out/browser_check/profile.html times each stage). The model panel lets the user pick the frames at once (1, 2,
  4, 8), kept for each of GPU and CPU: machines differ. The core is built with WebAssembly SIMD (.cargo/config.toml); the 2:1 RGB conversion takes 16 pixels at a time
  there (0.93 ms a frame, the same bytes: test_out/browser_check/rgb-bench.html).
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
  Not in the browser yet: runs without a stats file (KovaaK's HUD, Aim Lab's, the video alone), and the user's marks
  (run window, faint cut-off, areas).
- The old page (`python/app/`) stays the working UI until the Angular app does everything it does.
- Open: showing as much information as possible (after the redesign); moving targets on themes other than
  dark-on-light; thin capsules; tiled-wall seams; hand-checked ground truth; wiring the detector into KovOBS (the
  Rust prototype in `python/model/rust/` becomes the start of the core's detector).
