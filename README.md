# Aim View

Aim View reviews aim trainer recordings. A small trained detector finds every target in every frame. The review then
matches the kills with KovaaK's stats file for the run, or reads the game's HUD in the video, or finds the kills from
the video alone. Then it measures how you moved between targets.

It works on KovaaK's runs, Aim Lab runs and other players' recordings, with or without a stats file.

## What it shows

For clicking runs:

- every kill's time to kill (TTK), split into reaction, flick, micro-corrections, confirmation and click;
- each flick's speed, and whether it fell short of the target (underflick) or went past it (overflick);
- where your clicks landed on the targets;
- the fastest path between the targets, beside the path you took;
- checks that point out what costs you the most time.

For tracking runs:

- your time on the bot, 10 seconds at a time;
- how far the crosshair was from the bot's center line, and where it sat around the bot;
- what-if estimates: how much your accuracy would rise if you fixed each weakness.

The run page plays the video with the detector's boxes and your path drawn over it.

## Three ways to run it

All three use the same review code (Rust) and the same page (Angular):

1. **In the browser.** Everything runs in the page: the video decoding, the detector (on the GPU with WebGPU) and
   the review. Your recordings never leave your computer. Chrome works best: it has the folder picker and WebGPU
   (without WebGPU the detector runs on the CPU, several times slower).
2. **With the review server.** The server reviews the recordings, and the page shows the results. Use this when
   another machine has the faster GPU.
3. **As a desktop app** (Windows, Tauri 2). It opens your recordings where they are and can log your mouse in the
   background.

The detector runs with ONNX Runtime: DirectML on Windows (any GPU), CUDA on Linux with an NVIDIA GPU (the `cuda`
feature), or the CPU.

## Getting started

You need [Bun](https://bun.sh), [Rust](https://rustup.rs) with the `wasm32-unknown-unknown` target, and ffmpeg and
ffprobe on your PATH (the desktop app downloads them on its first review when they are missing).

```bash
rustup target add wasm32-unknown-unknown
bun install
bun install --cwd ui
```

Browser mode, at http://localhost:4200/:

```bash
bun run dev
```

The review server, at http://127.0.0.1:8770/:

```bash
bun run build:server
bun run server
```

Set the server's recordings folder and KovaaK's folder with its flags or `aimview-server.toml` (see
[server/README.md](server/README.md)). Build it with `--features cuda` for an NVIDIA GPU on Linux.

The desktop app (needs `cargo install tauri-cli`):

```bash
bun run app        # run it
bun run build:app  # its installer, in target/release/bundle/nsis/
```

## The detector

The detector is trained in Python with PyTorch, in `python/model/`. Its exports (ONNX) are in `python/model/exports/`.
[MODEL_STATUS.md](python/model/MODEL_STATUS.md) gives each model's results and limits.
[REPRODUCE.md](python/model/REPRODUCE.md) has every command to rebuild it. A new model replaces the current one
as the default only after it passes the acceptance gate (`python/model/accept.py`), which checks it against KovaaK's
stats files. Training needs Python 3 with PyTorch.

## Layout

| Path | What it holds |
| --- | --- |
| `src/` | The review core in Rust: tracking, the HUD reader, matching the kills, the measures and the report. Built natively and as WebAssembly. |
| `service/` | The review API, shared by every mode, and `aimview-tool`, which gives the Python scripts the library and the review. |
| `server/` | The review server (HTTP). |
| `desktop/` | The desktop app (Tauri 2). |
| `browser-service/` | The service built as WebAssembly for browser mode. |
| `ui/` | The page, in Angular. |
| `python/model/` | Training, evaluating and exporting the detector. |
| `tests/`, `benches/`, `examples/` | The Rust tests (checked against stored results), benchmarks and command-line examples. |
| `retired/`, `python/retired/` | Old code that has been replaced, kept for reference. |
| `test_out/` | Data, not in git: review caches, uploads and training data. |

## Development

```bash
cargo test --profile quick       # the Rust tests
bun run test:ui                  # the page's tests
bun run lint:ui                  # ESLint
python python/model/test_model.py
```

[AGENTS.md](AGENTS.md) holds the project's rules and every command. [docs/GLOSSARY.md](docs/GLOSSARY.md) names the domain words,
[python/README.md](python/README.md) explains how the review works, [docs/BENCH.md](docs/BENCH.md) lists the benchmarks and
their baselines, and [docs/HOT_PATHS.md](docs/HOT_PATHS.md) shows where a review spends its time.

## License

MIT. See [LICENSE](LICENSE).
