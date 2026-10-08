# Aim View: agent guide

Aim View reviews aim trainer recordings. The review core is Rust (`src/`). The review service (`service/`) answers the
review API; the desktop app and the Rust server serve it, and Python's scripts reach it through `aimview-tool`. The UI
is Angular (`ui/`). `python/model/` trains the target detector. The default detector is large_v13e4 (`infer.BEST`,
models.json's default).

The old Python review retired on 2026-10-04 (`python/retired/review.py`). The parity tests compare with its frozen
outputs (`test_out/parity/`), and the training scripts use its frozen parts (`python/model/old_review.py`), so a change
to the review is made once, in Rust. The project was copied from Flow Fix (`D:\Projects\flowfix`, folder `vod/`, now
`python/`) on 2026-10-02, with its caches and training data in `test_out/` (ignored by git).

## Where to look

Docs that grow (history, costs, benchmarks, maps) live in `docs/`; a new one goes there too. Read only the part you
need.

| File | What it holds | When to read it |
|---|---|---|
| `README.md` | The project and its layout | First |
| `docs/STATE.md` | What each part does today, how it was checked against Python and the stats files, what is open | Before working on a part |
| `ui/AGENTS.md` | The UI's rules (Angular, the three modes, styles, tables) | Before UI work (Claude Code loads it under `ui/`) |
| `docs/COSTS.md` | Every command's time by model and configuration, with date and commit | Before running any command |
| `docs/BENCH.md` | Every benchmark, its baseline, when to rerun it | Before running a benchmark |
| `docs/HOT_PATHS.md` | Where a review spends its time, stage by stage | Before optimizing |
| `docs/CODEMAP.md` | Every Rust file's header and public items (made by `tests/code_map.rs`) | Its file list before a Rust change |
| `docs/GLOSSARY.md` | Every domain word and the type that holds it | Before naming or shortening |
| `python/README.md` | How the review works | Before changing the review's logic |
| `python/model/MODEL_STATUS.md` | The detector's results and limits | Before detector work |
| `python/model/REPRODUCE.md` | Every command to rebuild the detector | Before training |
| `docs/storage-design.md` | The storage plan (SQLite, the export zip) | Before storage work |

Never run a command only to learn its cost or what it does. Time a new one with
`bun scripts/costs.ts run <name> [--config a=b] -- <command>` so its cost lands in `docs/COSTS.md` (the build scripts time
themselves). In Rust, search `docs/CODEMAP.md`, then follow calls with rust-analyzer's call hierarchy instead of reading
whole files.

## Commands

```bash
bun run server                          # the review server, http://127.0.0.1:8770/ (bun run build:server first; server/README.md)
bun run server:dev                      # the same, open to the local network, no token
cargo run -q --release -p aimview-service --bin aimview-tool -- help   # the service for scripts, JSON on stdout
python python/model/test_model.py       # the detector's tests
python python/model/eval_vods.py <model>                  # static runs against their stats files
python python/model/eval_moving.py name=<model> ...       # every scenario kind against the stats files
python python/model/eval_video_alone.py [model]           # the video-alone kill finder on 46 runs (--retrack after a tracking change)
python python/model/accept.py <name> [--list]             # the acceptance gate; --list adds a passing model to models.json
python python/model/crop_check/make_page.py <page> <set> <crops>   # crops for the Crops page (crop_check/README.md)
bun run dev                             # the UI in browser mode, http://localhost:4200/
bun run dev:server                      # the UI in server mode (needs the server)
bun run dev:server:lan                  # the same, open to the local network
bun run build                           # every mode's build into ui/dist/ (build:<mode> for one; --quick: quick WebAssembly, for this computer)
bun run costs                           # docs/COSTS.md's measured table again, from test_out/costs.jsonl
bun run app                             # the desktop app (Tauri 2, desktop/)
bun run build:app                       # its installer, in target/release/bundle/nsis/
bun run test:ui                         # the UI's tests
bun run lint:ui                         # ESLint and the style checks (bun run build runs it too)
bunx lefthook install                   # the git hooks (lefthook.yml; bun install runs it): format, lint, commit messages
bun run format                          # Prettier, over ui/
cargo clippy --workspace --all-targets  # the Rust lints
bacon                                   # the lints on every save (t: the quick tests), output in .bacon-locations
CODE_MAP_WRITE=1 cargo test --profile quick --test code_map   # docs/CODEMAP.md again, after a header or public item changes
python -m ruff check python             # the Python lints
cargo test --profile quick              # the Rust tests, against Python's results in test_out/parity/
python scripts/extract_module.py <src.py> <out.py> <name> ...  # copies a module's definitions and what they use
bun run assets                          # the core as WebAssembly, the models and the area data into ui/generated/
bun run types                           # the UI's types from the Rust structs, after changing one the UI reads
bun run tokens                          # ui/src/app/tokens/tokens.ts from the stylesheets, after changing a token
bun scripts/next-version.ts [--notes]   # the next release's version (release.yml publishes on a push to main)
```

## Paths

No folder is written in the code. `aimview.defaults.json` (in git) holds the project's layout, the server's address
and KovaaK's folders under Steam's. This computer's `aimview.json` (out of git) holds the recordings' folder (`vods`)
and anything else this computer needs. Rust reads them through `src/local_config.rs`, the scripts through
`python/local_config.py`. Steam's folder comes from the registry unless `aimview.json` names `steam`.

On this machine, recordings are in `E:\OBS\KovOBS` (one folder per scenario). KovaaK's stats are in
`...\steamapps\common\FPSAimTrainer\FPSAimTrainer\stats` (over 70k CSVs, so use a Python glob, not `ls`), and its
scenarios in `...\FPSAimTrainer\Saved\SaveGames\Scenarios`.

## Facts about the recordings

From the user, 2026-10-04:

- The crosshair is always at the screen's center and never changes during a run (no hit flash, color change or
  expanding). It differs between runs and players.
- One click kills at most one target. Kills are mostly hits at the crosshair, but some bots lose health on a timer, so
  a kill need not be at a hit.
- A recording can hold several runs. The countdown can be turned off, so never rely on it.
- FOV and KovaaK's HUD layout stay put within a run but can change between runs. The frame rate varies between
  recordings.
- Targets are spheres, pills, humanoid bots or squares. Most vanish on death; some settings show a death animation.
  Targets in a run mostly look alike. Their size changes with depth, and in some static scenarios. A target's color
  stands out from the wall's.
- The spawn delay and the number of targets on screen are up to the scenario.
- Health bars are a game setting and can show on any bot. A health bar is never a target.
- Games are KovaaK's, Aim Lab, Valorant and Aim Beast. KovaaK's runs mostly have a stats file; other people's
  recordings may not.
- Some maps show a sky that moves on its own. Zoom (ADS) is very rare, but some scenarios allow it. OBS overlays can
  sit on the play area.

## Working with this user

- **Commits.** Conventional Commits (`feat:`, `fix:`, `docs:`, `refactor:`, `chore:`, with an optional scope), on the
  branch that is checked out. Never create a branch unless asked.
- **Ask before changing.** Say what you will change and wait for a yes; answer questions directly. For a clear
  request, do the recommended option without asking. Ask when a request is unclear.
- **Large jobs need an explicit go.** Training a model, long GPU runs or a new subsystem start only after the user says
  so.
- **Correctness first.** The user wants near-100% accuracy, even at the cost of speed. Judge a detector by the
  stats-file checks (`eval_vods.py`, `eval_moving.py`, `eval_video_alone.py`), not only by crop scores.
- **Bun for JavaScript** tools, not Node.
- **No hard-coded configuration.** Paths, folders, URLs, ports and anything a user or machine might change go in the
  JSON settings (see Paths) or are discovered (Steam's folder from the registry), never in code. Flag any you find.
  Algorithm parameters stay named constants or the model's settings file.
- **Readable code.** Names say what a thing is, with its unit where it has one (`shift_deg`, `kill_frame`,
  `radius_px`). Single letters only for loop counters (`i`, `j`), coordinates (`x`, `y`) and a comparison's two sides
  (`a`, `b`). Short forms only from `docs/GLOSSARY.md`. A number with a meaning is a named constant
  (`const MAX_GAP_FRAMES: usize = 2`). Functions stay under about 60 lines.
- **Comments.** Every file starts with a comment on what it does, where its data comes from and where it goes. Every
  declaration has a doc comment, and a function's doc gives its units. Comments say why, not what; if a comment says
  what, rename instead. The lints warn on all of this (rustc and clippy, ESLint, Ruff; specs and generated types are
  left out). `python scripts/comments_only.py <commit>` checks that a change touched only comments.
- **Refactors keep behavior identical.** Use refactoring.guru's catalog (Rename Variable, Extract Method, Introduce
  Parameter Object, Replace Magic Number with Symbolic Constant, Decompose Conditional). Clear lint warnings file by
  file, and add none in new code. After every commit, the replay check, the parity tests and the UI tests pass
  unchanged.
- **Spelling and style.** Write "center", not "centre". Docs are plain, simple English: short sentences, active voice,
  no arrows.
- **Nothing gets deleted.** Old files move to a `retired/` folder. The user's own data is never overwritten.
- **Keep the data the user labelled**, all in `test_out/`:
  - hand-labelled crops (`vod_model/hand/`)
  - crops checked on the phone (`vod_model/check_moving_themes/checked.jsonl`, `phone_answers/`,
    `phone_answers_405/`; `vod_model/data_moving_themes/checked_phone.jsonl`, `vod_model/data_mined/checked_phone.jsonl`,
    `vod_model/check_mined/phone_answers/`)
  - area labels and types (`vod_app/area_examples.jsonl`, `area_kinds.json`)
  - per recording: cut-offs (`faint.json`), run marks (`run.json`) and the stats file picked for it (`stats.json`)
