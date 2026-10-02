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
bun run dev                                    # the Angular UI, http://localhost:4200/ (needs the server above)
bun run test:ui                                # the UI's tests
bun run lint:ui                                # ESLint (angular-eslint's recommended set, plus the rules below)
bun run format                                 # Prettier, over ui/
cargo check                                    # the Rust core
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
  state (RxJS is allowed where it fits better) and `resource()` with `fetch` for data. Prefer signal forms
  (`@angular/forms/signals`); the lint config warns on the older forms and HttpClient. Anything that changes every frame (the video overlay,
  timelines) is drawn on a canvas in `requestAnimationFrame` or `requestVideoFrameCallback`, never through a template.
  A resource's `value()` throws in its error state: check `error()` or `hasValue()` first.
- **Named types.** In TypeScript, every object or tuple type gets a name (an interface or a type alias). No inline
  anonymous types such as `{ gpu: number; cpu: number }` in a field or a signature. ESLint enforces it.
- **Styles are SCSS, and every design value is a token.** A token is a CSS variable (so it can be edited live in the
  browser), with an SCSS name for it: `$surface-0: var(--surface-0)`. The main tokens are in `ui/src/themes/theme.scss`;
  page and module tokens (values only one part uses) are in `ui/src/themes/<page or module>.scss`. Each file has a
  `tokens` mixin, which `styles.scss` includes in `:root`. Component styles `@use 'themes/...'` and use only `$tokens`:
  no raw colors, sizes, spaces, fonts or durations. Keywords and layout values (`flex`, `solid`, `0`, `100%`, `1fr`)
  are fine. Canvas drawings read their colors and fonts from the same CSS variables.
- **Tailwind on the tokens.** Templates use Tailwind 4 utilities. `ui/src/tailwind.css` maps Tailwind's theme onto the
  tokens (`@theme inline reference`, with Tailwind's own scales switched off), so a class can only reach a token:
  `bg-surface-1`, `text-muted`, `p-4` (4 x `--space-1`), `w-(--sidebar-width)`. No arbitrary values such as `p-[13px]`.
  Component SCSS holds only what utilities cannot say, inside `@layer components`; page-wide element styles are in
  `@layer base`. That way a utility on an element always wins.
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
  (`test_out/vod_app/area_examples.jsonl`, `area_kinds.json`), cut-offs (`faint.json`) and run marks (`run.json`) per
  recording. Keep them.

## State (2026-10-02)

- The detector is full_v3 (`infer.BEST`), trained on every scenario kind; small_v13 is the small one for speed.
- The plan: the app runs three ways from one code base (browser only, browser with the Python server, desktop). The UI
  is Angular 22 and carries the redesign from the 2026-10-02 mockup. The review core is Rust, built natively for the
  desktop app (Tauri 2) and as WebAssembly for the browser. Python stays the reference: the core replaces nothing
  until its reports match Python's on every recording.
- Done: the layout (`python/`, the Rust crate at the root, `ui/`). In `ui/`: the recordings list, and the run page
  (review button and progress, the video with its overlay, seek bar, controls, keys, and a tracking run's timeline).
  Next in `ui/`: the report's cards and tables and the flick list, then the tool panels (cut-off, run marks, areas),
  the queues, upload and the model panel. After that: porting the review to Rust.
- The old page (`python/app/`) stays the working UI until the Angular app does everything it does.
- Open: showing as much information as possible (after the redesign); moving targets on themes other than
  dark-on-light; thin capsules; tiled-wall seams; hand-checked ground truth; wiring the detector into KovOBS (the
  Rust prototype in `python/model/rust/` becomes the start of the core's detector).
