# Costs

What each command costs: how long it takes, and what else it takes (the GPU, the CPU, disk). Read this before running
a command. Do not run a command only to learn what this file already says; run it when its inputs changed or its
number is missing, and its time lands here.

The scripts keep the measured table current themselves. `scripts/costs.ts` times each step of the build scripts
(`bun run build`, `build:<mode>`, `assets`) and the commands run through it (`test:ui`, `lint:ui`, `build:app`), and
any other command run as `bun scripts/costs.ts run <name> -- <command ...>`. Each timing is a line of `costs.jsonl`
in the data folder (aimview.json's `data`: `test_out/costs.jsonl` here), and every timing rewrites the table below
from that log. `bun run costs` rewrites it by hand. Benchmarks and their baselines stay in docs/BENCH.md, and where a
review spends its time in docs/HOT_PATHS.md.

Cold means after a change that the step has to redo (a Rust change for a cargo build); warm means nothing changed.

## Measured by the scripts

<!-- costs:start -->
| Step | Configuration | Latest | Median of the last 5 | Runs | Latest run (date, commit; + uncommitted changes) |
| --- | --- | --- | --- | --- | --- |
| assets | profile release, modes browser | 0.3 s | 68.7 s | 2 | 2026-10-08, 4303844+ |
| assets | profile release, modes browser,server,desktop | 57.2 s | 57.2 s | 3 | 2026-10-07, 78c5844+ |
| assets | profile release, modes server | 38.5 s | 37.3 s | 4 | 2026-10-08, ac709f4+ |
| assets | profile wasm-dev, modes browser | 7.3 s | 49.8 s | 5 | 2026-10-08, f61190c+ |
| assets | profile wasm-dev, modes browser,server,desktop | 34.5 s | 19.5 s | 22 | 2026-10-09, 3b73a35+ |
| assets | profile wasm-dev, modes server | 12.4 s | 11.0 s | 5 | 2026-10-09, 3cf8d5a+ |
| assets: core and service wasm (cargo) | profile release | 0.2 s | 28.7 s | 4 | 2026-10-08, 4303844+ |
| assets: core and service wasm (cargo) | profile wasm-dev | 21.9 s | 12.0 s | 27 | 2026-10-09, 3b73a35+ |
| assets: core wasm (cargo) | profile release | 38.5 s | 37.3 s | 4 | 2026-10-08, ac709f4+ |
| assets: core wasm (cargo) | profile wasm-dev | 12.2 s | 10.9 s | 5 | 2026-10-09, 3cf8d5a+ |
| assets: core wasm (cargo, built alone) | profile release | 36.1 s | 36.1 s | 1 | 2026-10-07, 78c5844+ |
| assets: service wasm (cargo, built alone) | profile release | 65.5 s | 65.5 s | 1 | 2026-10-07, 78c5844+ |
| assets: wasm-opt (Asyncify) | level -O0 | 12.1 s | 7.4 s | 10 | 2026-10-09, 3b73a35+ |
| assets: wasm-opt (Asyncify) | level -O1 | 44.3 s | 44.3 s | 8 | 2026-10-08, f61190c+ |
| assets: wasm-opt (Asyncify) | level -O2 | 64.7 s | 62.5 s | 2 | 2026-10-08, 4303844+ |
| build | modes browser | 143.3 s | 143.3 s | 1 | 2026-10-08, 4303844+ |
| build | modes browser,server,desktop, angular at once | 63.3 s | 34.2 s | 2 | 2026-10-07, 78c5844+ |
| build | modes browser,server,desktop, angular one at a time | 182.7 s | 182.7 s | 1 | 2026-10-07, 78c5844+ |
| build | modes server | 44.5 s | 42.3 s | 3 | 2026-10-08, ac709f4+ |
| build | modes server, assets quick | 35.5 s | 35.5 s | 3 | 2026-10-09, 1eb73ed+ |
| build: Angular (production) | mode browser | 6.2 s | 6.1 s | 4 | 2026-10-08, 4303844+ |
| build: Angular (production) | mode desktop | 6 s | 4.6 s | 3 | 2026-10-07, 78c5844+ |
| build: Angular (production) | mode server | 21 s | 6.2 s | 9 | 2026-10-09, 1eb73ed+ |
| build: lint | - | 11.9 s | 5.0 s | 6 | 2026-10-09, 1eb73ed+ |
| export | model large_v15e4 | 24.1 s | 24.1 s | 1 | 2026-10-09, 38ba8e9+ |
| label_batch | - | 4535.7 s | 4535.7 s | 1 | 2026-10-08, a1144b8 |
| label_score | - | 21.9 s | 25.1 s | 2 | 2026-10-08, edafedc+ |
| lint:ui | - | 5.9 s | 5.9 s | 53 | 2026-10-09, 8fe2b6c+ |
| merged_pairs | binaries prebuilt | 1747.8 s | 1747.8 s | 1 | 2026-10-09, ac62697+ |
| merged_pairs | binaries prebuilt, reviews cached | 1163.9 s | 1163.9 s | 1 | 2026-10-09, 3c9d214+ |
| review-av1 | profile local | 19.2 s | 19.1 s | 2 | 2026-10-08, 433f4a8+ |
| review-av1 | profile local-thin | 11.8 s | 26.6 s | 2 | 2026-10-08, 433f4a8+ |
| review-av1 | profile release | 11.9 s | 12.4 s | 2 | 2026-10-08, 433f4a8+ |
| rust-tests | profile quick | 28.4 s | 28.4 s | 9 | 2026-10-08, 6d5bc9d+ |
| rust-tests-build | profile quick, linker link.exe, change edit | 13.1 s | 13.1 s | 1 | 2026-10-08, 433f4a8+ |
| rust-tests-build | profile quick, linker rust-lld, change edit | 12.2 s | 12.2 s | 1 | 2026-10-08, 433f4a8+ |
| server-build | profile local, linker link.exe | 2.3 s | 2.3 s | 1 | 2026-10-08, 433f4a8+ |
| server-build | profile local, linker link.exe, change edit | 6 s | 6.0 s | 1 | 2026-10-08, 433f4a8+ |
| server-build | profile local, linker rust-lld, change edit | 5.6 s | 5.6 s | 1 | 2026-10-08, 433f4a8+ |
| server-build | profile local-thin, linker link.exe, change edit | 22.3 s | 22.3 s | 1 | 2026-10-08, 433f4a8+ |
| server-build | profile release, linker link.exe | 53.8 s | 53.8 s | 1 | 2026-10-08, 433f4a8+ |
| server-build | profile release, linker link.exe, change edit | 77.7 s | 77.7 s | 1 | 2026-10-08, 433f4a8+ |
| server-build | profile release, linker rust-lld, change edit | 76.7 s | 76.7 s | 1 | 2026-10-08, 433f4a8+ |
| storage-backends | build quick | 159.7 s | 168.5 s | 2 | 2026-10-07, 75e908f+ |
| test:ui | - | 12.1 s | 11.9 s | 44 | 2026-10-09, 8fe2b6c+ |
| tokens | - | 0.5 s | 0.4 s | 8 | 2026-10-09, 8fe2b6c+ |
| train | model large_v15 | 197.8 s | 197.8 s | 1 | 2026-10-09, 38ba8e9 |
<!-- costs:end -->

## Recorded by hand

Measured before the scripts timed themselves. Each row gives its date and the commit that recorded the number (a
commit with + had uncommitted changes then), and where it is written. A row moves to the table above once a script
times that step.

### Builds and tools

| Command | What it does | Cost | Measured (date, commit) | Source |
| --- | --- | --- | --- | --- |
| `cargo build --profile release --target wasm32-unknown-unknown` (the core, inside `bun run assets --release`) | the core as WebAssembly with whole-program optimization | about 55 s after a Rust change; `wasm-dev` (the dev assets) about 9 s; measured since: 36.1 s alone, 57.1 s with the service in the same cargo run (the table above) | 5fac078 | Cargo.toml, `[profile.wasm-dev]` |
| `wasm-opt --asyncify` on the browser's service (inside `bun run assets`) | Binaryen's Asyncify, so the service's file calls can wait on the page | `-O2` (release) 67 s, `-O1` (dev) 37 s, every run until 2026-10-07 (60.3 s measured since); now skipped when its input is unchanged | edc88e7 | scripts/ui-assets.ts |
| Browser mode: choosing KovaaK's stats folder the first time | each of the 72,127 stats files (401 MB) read once and its run sent to the service in batches (kovaak-batch.ts), kept in the database | 6.1 s to send and keep, once the files were in the page (reading them from the folder not included); choosing it again 0.8 s, nothing sent; the database 14.5 MB (8.3 MB of it the stats files' rows) against 401 MB of packs before; /api/kovaak_files 0.35 s, a scenario's history (117 runs) 0.6 s | 2026-10-08, d12d258 | the built-in browser (Chromium), files served from the stats folder |
| `bun run build` before scripts/build.ts | the assets with `--release` three times (one per mode), each wiping ui/generated/ and running `wasm-opt -O2` again, then the three Angular builds one after another | about 306 s after a Rust change (the first assets 162 s, two more `wasm-opt` runs, the Angular builds 20 s); about 205 s with nothing changed | 2026-10-07, 78c5844+ (worked out from the measured steps) | package.json before scripts/build.ts |
| `bun run build` now | the assets once (one cargo run for the core and the service, `wasm-opt` only when the service changed), the three Angular builds at once | 5.1 s with nothing changed; 63.3 s after a Rust change that leaves the service's module as it was; about 60 s more when `wasm-opt` has to run | 2026-10-07, 78c5844+ | the measured table above |
| `bun run build:server`, `build:desktop` now | the core alone (no service, no `wasm-opt`), one Angular build | 39.9 s after a Rust change (server); the browser-only files (onnxruntime 40 MB, the service, the models) no longer go in: ui/dist/server 51 to 5.2 MB, ui/dist/desktop 68 to 5.2 MB | 2026-10-07, 78c5844+ | the measured table above |
| `cargo test --release` (a build) | the Rust tests with the release profile | about 40 s a build; `--profile quick` about 3 s | 78863f2 | AGENTS.md, Commands |
| `cargo bench` (a build) | the criterion benches, release profile with LTO | about 2 min to build | fd03a47 | docs/BENCH.md |
| `cargo clippy --workspace --all-targets` | the Rust lints | 0.5 s with nothing changed | 2026-10-07, b7e39e9+ | this session |
| `python scripts/storage_check.py <name> <exe>` | copies test_out's app data and the desktop app's data, asks the API, hashes every file | about 3 min a run, 40 s of it waiting for a cut-off's labels; two runs and a compare about 6 min | 2026-10-07, 55460ce+ | this session (04:36 to 04:42) |

### The detector, by model and configuration

A frame's detector time depends on the model, the device and how many frames go in one call. On this machine (RTX
5070 Ti). The native review is not set by it: the CPU sets its pace (docs/HOT_PATHS.md, Detector).

| Model | Native, DirectML | Native, other | Browser, WebGPU (4 frames a call) | Browser, CPU (8 threads) | Measured (date, commit) | Source |
| --- | --- | --- | --- | --- | --- | --- |
| large_v13e4 (the default) | 1.55 ms a frame | TensorRT fp32 1.41 ms (not kept: the review was slower) | 5.50 ms | 31.0 ms | browser 2026-10-06, d19e766; native 2026-10-07, f593028 | docs/BENCH.md (model speed), docs/HOT_PATHS.md |
| full_v3 | 1.84 ms | PyTorch CUDA 1.7 to 2.2 ms, fp16 1.0 ms (b794f53) | 5.31 ms | 20.6 ms | native 4600bbe; browser 2026-10-06, d19e766 | docs/BENCH.md, docs/HOT_PATHS.md |
| small_v13 (left the picker) | not timed | | 4.19 ms | 11.1 ms | 2026-10-06, d19e766 | docs/BENCH.md |
| a 1,405-parameter network | the native review of av1 in 11.7 s against full_v3's 11.9 s | | | | b095354 | docs/HOT_PATHS.md |

Frames a call (the model panel's 1, 2, 4, 8): natively large_v13e4 took the same 5.9 s on the 0:20 to 0:40 window at
batches of 2, 4 and 8, and in fp16 (2026-10-07, 36648ee); 4 is the default. In the browser on the CPU one frame a
call.

### Reviews (av1: 2560 x 1440 AV1, 6,038 frames, on this machine's RTX 5070 Ti)

| Command | What it does | Cost | Measured (date, commit) | Source |
| --- | --- | --- | --- | --- |
| a native review, whole video (the desktop app, aimview-tool `review`) | decode, detector, camera, HUD | 11.1 s with the GPU's frames (CPU 49% busy), 14.1 s through ffmpeg (88%) | 756b1c5 | docs/HOT_PATHS.md, Decode |
| a native review with the kill check (no stats file), large_v13e4 | the review, then the kill check's second read | whole video 15.5 s (16.1 before the parts); 0:20 to 0:40 8.1 s (5.9 without the check); through ffmpeg 9.4 s | 2026-10-07, 96001b7 | docs/HOT_PATHS.md, Kill check |
| a browser review (WebGPU), full_v3 | the same in the page | 15.9 s (380 frames a second); on the CPU 31 frames a second | df303b3 | AGENTS.md, State; docs/HOT_PATHS.md |
| a native review, 0:20 to 0:40, large_v13e4 | the run window only | 5.48 s on DirectML (6.26 s with TensorRT, 49 s of CPU against 7); 5.9 s at batches 2 to 8 | 2026-10-07, f593028 | docs/HOT_PATHS.md |
| the native reviews of the benchmark's 32 recordings | eval_vods.py's reviews, on a GPU shared with other work | 21 s a recording on average with full_v3, 18 s with small_v13 | 0d6aa7e | python/model/MODEL_STATUS.md |
| a review's fixed cost | the model, DirectML's session, the key frames | about 1.5 s | 2026-10-07, 36648ee | docs/HOT_PATHS.md |
| a native review's heap | the allocations a review makes | 2.39 GB over the review (8.45 GB before); peak memory 683 MB for a window, 1,224 MB for the whole video | 2026-10-07, 31c790b | this session's before and after |

### Checks and training (python/model/)

| Command | What it does | Cost | Measured (date, commit) | Source |
| --- | --- | --- | --- | --- |
| `cargo test --profile quick --test replay` | the review after the detector, byte for byte | 0.8 s to run | 326eb95 | docs/BENCH.md |
| `cargo run --profile quick --example review_runs -- <out>` | 1,006 reports from kept tracks | 1.5 min | d1214f0 | docs/BENCH.md |
| `python python/model/eval_vods.py`-style full reviews of the benchmark's recordings | a native review of each recording by one model | about 15 min for full_v3 on the 46 runs | b27d615 | python/model/REPRODUCE.md |
| `python python/model/build_data.py` | the detector's crops from the recordings | about 3 min of writing crops on 14 processes | c8e8f90 | python/model/REPRODUCE.md |
| a training run of the small model | python/model/train.py | about 100 s an epoch (126 s the first), 7 min in all | 8646808 | python/model/MODEL_STATUS.md |

Not timed yet, and timed by the scripts from their next run: the UI's tests and lint, the installer (`build:app`).
Not timed at all yet: `bun run types`, `cargo test --profile quick` over the workspace, `eval_moving.py`,
`eval_video_alone.py`; time them through `bun scripts/costs.ts run <name> --config model=<name> -- <command>` the next
time they run for a reason.
The acceptance gate (`accept.py`) is timed as `accept` in the log (test_out/costs.jsonl). The measured table above
keeps only runs that exit 0, and the gate exits 1 when the model fails, so a failed gate is in the log only:
large_v15e4 (2026-10-09, 38ba8e9) took 282 s to its first failed stage and 1,073 s with `--all` (every stage, with its
video-alone tracks made fresh).
