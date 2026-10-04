# Aim View

Review aim trainer recordings (VODs). The app tracks every target frame by frame with a trained detector, matches
the kills with the run's stats file (or reads the game's HUD in the video), and measures each flick. For tracking runs
it measures the time on the target, how the crosshair followed the bot, and what would raise the accuracy. It works on
KovaaK's recordings from KovOBS, other players' uploads, and Aim Lab runs.

Aim View started inside the Flow Fix project (`D:\Projects\flowfix`, folder `vod/`) and was copied here on 2026-10-02.

## Where it's going

Aim View will run three ways from one code base:

1. As a web app, with the detector and the review running in the browser.
2. As a web app with the review server doing some of the work.
3. As a desktop app.

The parts:

- **Review core:** Rust. It is built natively for the desktop app and as WebAssembly for the browser. It is checked
  against KovaaK's stats files, the ground truth. Python's review is a cross-check while it lasts; `python/model/`
  stays for training the detector.
- **UI:** Angular 22, shared by all three. Each way of running supplies its own services for recordings, stats files,
  models and the review (`ui/src/app/platform/` holds what they must do, `ui/src/app/modes/` how each mode does it).
  A build carries only its own mode's code.
- **Desktop app:** Tauri 2.
- **Python:** the server, training and evaluation.

Bun runs the JavaScript tools.

### Target layout

```
aim-view/
├── Cargo.toml     Cargo workspace, and the review core crate
├── src/           the review core in Rust (wasm.rs: browser bindings, WebAssembly builds only)
├── tests/         parity tests: Rust reports checked against Python's
├── benches/       speed tests
├── desktop/       the Tauri 2 app
├── ui/            the Angular 22 app
├── models/        the ONNX files every build ships
├── python/        the review, server and readers in Python; model/ for training and evaluation
├── package.json   the build scripts, run with Bun
├── retired/       old code that has been replaced
└── test_out/      data, not in git
```

Built so far: `python/`, the Rust crate (`Cargo.toml` and `src/`, empty for now) and the Angular app in `ui/` (its top
bar). The other folders arrive as they are built. `package.json` becomes a Bun workspace once there is a second
JavaScript package (the core's WebAssembly build).

## Layout today

| Path | What it holds |
| --- | --- |
| `python/retired/review.py` | The old Python review pipeline (retired 2026-10-04: the Rust core in `src/` is the review). The parity tests compare with its stored outputs in `test_out/parity/`; the training scripts use its frozen parts, `python/model/old_review.py`. |
| `service/`, `server/` | The review API in Rust (shared by every server) and the HTTP server (port 8770). The service's `aimview-tool` gives Python's scripts the library and the native review (`python/aimview_tools.py`). |
| `python/retired/app/` | The old web page (plain HTML, CSS and JavaScript), retired: the Angular app in `ui/` does what it did. |
| `python/hud.py`, `python/areas.py` | The HUD readers (KovaaK's session HUD, Aim Lab's POINTS) and the overlay-area finder. |
| `python/model/` | The target detector: training, evaluation, export, and the exported models (`exports/`, every version). |
| `python/README.md` | How the review works, in detail. |
| `ui/` | The new web app in Angular 22 (in progress): zoneless, OnPush, signals. |
| `Cargo.toml`, `src/` | The review core in Rust (started, empty). |
| `python/model/MODEL_STATUS.md`, `python/model/REPRODUCE.md` | The detector's results and limits, and every command to rebuild it. |
| `test_out/` | Not in git: each recording's review cache (`vod_app/`), uploads (`vod_uploads/`), the training data, runs and hand labels (`vod_model/`), and the HUD readers' test data (`hud/`). |

`python/` sits beside `test_out/`, as `vod/` did in Flow Fix, so the code's paths to its data work as before.

## Run the app

```bash
bun run build:server
bun run server
```

Then open http://127.0.0.1:8770/. It lists the recordings in `E:\OBS\KovOBS` and finds their stats files in KovaaK's
`stats` folder. The detector runs through ONNX Runtime: DirectML on Windows, CUDA on Linux (the `cuda` feature), or the CPU.

The new Angular app runs in browser mode (everything in the browser) or in server mode (its data from that server):

```bash
bun install --cwd ui
bun run dev           # browser mode
bun run dev:server    # server mode
```

Then open http://localhost:4200/. `bun run build` builds every mode into `ui/dist/browser`, `ui/dist/server` and
`ui/dist/desktop`.

## Needs

Python 3 with NumPy, SciPy and Pillow; PyTorch (GPU) or ONNX Runtime (CPU) for the detector; ffmpeg and ffprobe on
the PATH. Training and the deployment prototypes need more: see `python/model/REPRODUCE.md`. The Angular app needs Bun;
the Rust core needs Rust with the `wasm32-unknown-unknown` target.
