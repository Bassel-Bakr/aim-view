# aimview

Review aim trainer recordings (VODs). The app tracks every target frame by frame with a trained detector, matches
the kills with the run's stats file (or reads the game's HUD in the video), and measures each flick. For tracking runs
it measures the time on the target, how the crosshair followed the bot, and what would raise the accuracy. It works on
KovaaK's recordings from KovOBS, other players' uploads, and Aim Lab runs.

aimview started inside the Flow Fix project (`D:\Projects\flowfix`, folder `vod/`) and was copied here on 2026-10-02.

## Where it's going

aimview will run three ways from one code base:

1. As a web app, with the detector and the review running in the browser.
2. As a web app with the Python server doing some of the work.
3. As a desktop app.

The parts:

- **Review core:** Rust. It is built natively for the desktop app and as WebAssembly for the browser. The Python code
  stays the reference: the core replaces nothing until its reports match Python's on every recording.
- **UI:** Angular 22, shared by all three. Each way of running supplies its own services for recordings, storage, the
  detector and the review.
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
├── package.json   Bun workspace and the build scripts
├── retired/       old code that has been replaced
└── test_out/      data, not in git
```

The Python code is in `python/`. The other folders arrive as they are built.

## Layout today

| Path | What it holds |
| --- | --- |
| `python/review.py` | The whole review pipeline: tracking, kill matching, flick and tracking measures, the report. |
| `python/server.py` | The review web app's server (port 8770) and its JSON API. |
| `python/app/` | The web app: `index.html`, `style.css`, `app.js` (plain HTML, CSS and JavaScript, no build step). |
| `python/hud.py`, `python/areas.py` | The HUD readers (KovaaK's session HUD, Aim Lab's POINTS) and the overlay-area finder. |
| `python/model/` | The target detector: training, evaluation, export, and the exported models (`exports/`, every version). |
| `python/README.md` | How the review works, in detail. |
| `python/model/MODEL_STATUS.md`, `python/model/REPRODUCE.md` | The detector's results and limits, and every command to rebuild it. |
| `test_out/` | Not in git: each recording's review cache (`vod_app/`), uploads (`vod_uploads/`), the training data, runs and hand labels (`vod_model/`), and the HUD readers' test data (`hud/`). |

`python/` sits beside `test_out/`, as `vod/` did in Flow Fix, so the code's paths to its data work as before.

## Run the app

```bash
python python/server.py --port 8770
```

Then open http://127.0.0.1:8770/. It lists the recordings in `E:\OBS\KovOBS` and finds their stats files in KovaaK's
`stats` folder. The detector runs on an NVIDIA GPU through PyTorch, or on the CPU through ONNX Runtime.

## Needs

Python 3 with NumPy, SciPy and Pillow; PyTorch (GPU) or ONNX Runtime (CPU) for the detector; ffmpeg and ffprobe on
the PATH. Training and the deployment prototypes need more: see `python/model/REPRODUCE.md`.
