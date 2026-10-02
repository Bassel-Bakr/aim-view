# aimview: agent guide

aimview reviews aim trainer recordings: `vod/review.py` is the pipeline, `vod/server.py` and `vod/app/` the web app,
`vod/model/` the trained target detector. Read `README.md` first, then `vod/README.md` (how the review works) and
`vod/model/MODEL_STATUS.md` (the detector's results and limits). Every command to rebuild the detector is in
`vod/model/REPRODUCE.md`.

It was copied from the Flow Fix project (`D:\Projects\flowfix`, folder `vod/`) on 2026-10-02, with its caches and
training data in `test_out/` (ignored by git). The layout is kept for now, so paths work as before. The planned stack
and layout are in `README.md` ("Where it's going"). Moving `vod/` to `python/` comes first, as its own commit with
only moves and path fixes.

## Commands

```bash
python vod/server.py --port 8770            # the review app, http://127.0.0.1:8770/
python vod/model/test_model.py              # the detector's tests
python vod/model/eval_vods.py <model.pt>    # static runs against their stats files
python vod/model/eval_moving.py name=<model.pt> ...   # every scenario kind against the stats files
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
- **Spelling and style.** Write "center", not "centre". Docs in plain, simple English: short sentences, active voice,
  no arrows.
- **Nothing gets deleted.** Old files move to a `retired/` folder; the user's own data is never overwritten.
- **Data the user labelled** (in `test_out/`): hand-labelled crops (`test_out/vod_model/hand/`), area labels and types
  (`test_out/vod_app/area_examples.jsonl`, `area_kinds.json`), cut-offs (`faint.json`) and run marks (`run.json`) per
  recording. Keep them.

## State (2026-10-02)

- The detector is full_v3 (`infer.BEST`), trained on every scenario kind; small_v13 is the small one for speed.
- Decided, not started (wait for the user's go): the app runs three ways from one code base (browser only, browser
  with the Python server, desktop). The UI is Angular 22 and carries the redesign from the 2026-10-02 mockup. The
  review core is Rust, built natively for the desktop app (Tauri 2) and as WebAssembly for the browser. Python stays
  the reference: the core replaces nothing until its reports match Python's on every recording.
- Open: showing as much information as possible (after the redesign); moving targets on themes other than
  dark-on-light; thin capsules; tiled-wall seams; hand-checked ground truth; wiring the detector into KovOBS (the
  Rust prototype in `vod/model/rust/` becomes the start of the core's detector).
