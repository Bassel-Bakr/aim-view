# Reproduce the target detector

Every command runs from the repository root (`D:\Projects\flowfix`) in Git Bash. The results in
[MODEL_STATUS.md](MODEL_STATUS.md) come from exactly these commands.

## What you need

- Python 3.14 with `torch` (CUDA build; 2.14 + cu130 was used), `numpy`, `scipy`, `onnx` 1.23, `onnxruntime` 1.30,
  `onnxscript`, `psutil` and `pillow`. Only ONNX Runtime and NumPy are needed to *run* an exported model.
- `ffmpeg` and `ffprobe` on the PATH.
- The KovOBS library at `E:\OBS\KovOBS` (one folder per scenario) and KovaaK's installed, because the dataset builder
  reads each scenario's `.sce` file to keep only static scenarios, and the end-to-end check reads the stats files.
- An NVIDIA GPU for training (an RTX 5070 Ti was used). Without one, training runs on the CPU, much slower.
- For the browser test: Bun (or npm) to install `onnxruntime-web`. For the native test: Rust (cargo 1.96 was used).

Generated files go to `test_out/vod_model/` (ignored by git). The exported models go to `python/model/exports/`.

## 1. Prepare the dataset

```bash
python python/model/build_data.py --vods "E:/OBS/KovOBS" --per-folder 4
```

This takes the 4 newest recordings of every static scenario (530 VODs), decodes only their key frames, labels the
targets with the hand-written detector in `python/review.py`, keeps only VODs whose labels look steady, and saves
256 × 256 crops with their boxes. Splits are by scenario folder (a stable hash; the four end-to-end VODs' folders are
always in test). The last build wrote its crops over about 3 minutes, on 14 processes (one per logical core, less 2).

```bash
python python/model/validate_data.py
```

This checks that no scenario is in two splits, counts crops, boxes and box sizes per split, writes
`test_out/vod_model/data/validation.json` and draws a contact sheet of labelled crops at `test_out/vod_model/sheet.png`.

Check a sample of the automatic labels by hand (the first ground truth; the weak cases come first):

```bash
python python/model/label_check.py --n 400
```

Open `http://127.0.0.1:8773/`. Each checked crop is saved to `test_out/vod_model/checked.jsonl`.

Crops from the moments just before kills (incremental: VODs already done are skipped):

```bash
python python/model/build_kills.py --out test_out/vod_model/data_kills --per-folder 1
```

The current kill-moment set (small_v7's: labelled by small_v2, other targets capped at the scenario's target count,
a lost target labelled only where some of it shows beside the crosshair), with the user's new runs from 2026-10-01
listed in `new_runs.txt`. small_v6 used the same command into `data_kills3`, before those label fixes:

```bash
python python/model/build_kills.py --out test_out/vod_model/data_kills4 --per-folder 1 --also test_out/vod_model/new_runs.txt
```

## 2. Train

```bash
python python/model/train.py python/model/configs/tiny.json
python python/model/train.py python/model/configs/small.json
python python/model/train.py python/model/configs/full.json
python python/model/train.py python/model/configs/tiny_v2.json
python python/model/train.py python/model/configs/small_v2.json
python python/model/train.py python/model/configs/full_v2.json
python python/model/train.py python/model/configs/small_v4.json --data test_out/vod_model/data_v3   --extra test_out/vod_model/data_kills --init test_out/vod_model/runs/small_v2/best.pt
python python/model/train.py python/model/configs/small_v6.json --data test_out/vod_model/data_v3 --extra test_out/vod_model/data_kills3 \
  --init test_out/vod_model/runs/small_v2/best.pt --repeat test_out/vod_model/new_runs.txt --times 3
python python/model/train.py python/model/configs/small_v7.json --data test_out/vod_model/data_v3 --extra test_out/vod_model/data_kills4 \
  --init test_out/vod_model/runs/small_v2/best.pt --repeat test_out/vod_model/new_runs.txt --times 3
```

small_v8 is the same as small_v7 with `small_v8.json` (no outlined crosshairs).

Hand labels (small_v9 and v10). Cut crops round what the model finds (or round the crosshair) in chosen runs, check
them on the label page (`label-valorant` and `label-batch2` in `.claude/launch.json`: Correct saves the rings, Skip
means no target there, Can't tell leaves the crop out), then turn the checked crops into datasets, holding out whole
runs as the test:

```bash
python python/model/hand_crops.py "<valorant 467.53>.mp4" "<valorant 558.46>.mp4" --out test_out/vod_model/hand/valorant --per-vod 50
python python/model/label_check.py --n 100 --port 8774 --data test_out/vod_model/hand/valorant --out test_out/vod_model/hand/valorant/checked.jsonl
python python/model/hand_crops.py --labels test_out/vod_model/hand/valorant/checked.jsonl --out test_out/vod_model/hand_data --test 558.4
python python/model/hand_crops.py --labels test_out/vod_model/hand/batch2/checked.jsonl --out test_out/vod_model/hand_data2 --test "2040,Jumbo1wall9000targets_-_283"
python python/model/train.py python/model/configs/small_v10.json --data test_out/vod_model/data_v3 --extra test_out/vod_model/data_kills4 \
  --extra test_out/vod_model/hand_data --extra test_out/vod_model/hand_data2 --init test_out/vod_model/runs/small_v2/best.pt \
  --repeat test_out/vod_model/repeat_v9.txt --times 3
```

Batch 2's crops came from `hand_crops.py` runs with `--centre find` (Jumbo, ClickTrack), `--centre crosshair`
(Pokeball 5, 1w4ts 134) and `--centre mixed` (the 1931 WR and ww5t 2040 uploads). `repeat_v9.txt` is `new_runs.txt`
plus the line `hand_crop_ 20` (hand crops count 20 times). small_v9 is the same without `hand_data2`.

small_v11 and small_v12 are the small_v10 command with `small_v11.json` or `small_v12.json`: both set
`crosshair_real` 0.6, which draws KovaaK's installed crosshair images (`train.KOVAAKS_CROSSHAIRS`, the install's
`crosshairs` folder; without it, only the made-up crosshairs are drawn).

Moving targets (dynamic clicking, tracking and switching; 2026-10-02). The dataset takes the 2 newest recordings of
every such scenario and labels them with `build_data.dark_labels` (dark blobs on light walls, any size or shape), only
in recordings that show dark targets on light walls (`dark_scene`), and drops a recording with more labels than the
scenario has targets. About 10 minutes on 14 processes:

```bash
python python/model/build_data.py --kinds dynamic,tracking,switching --per-folder 2 --dark --out test_out/vod_model/data_moving_dark
python python/model/validate_data.py --data test_out/vod_model/data_moving_dark --sheet test_out/vod_model/sheet_moving_dark.png
```

Then three models, trained side by side (about an hour together on the RTX 5070 Ti; the CPU's data loading is the
limit): `small_v13` and `full_v3` on everything (static and moving), `small_mv1` on the moving data alone:

```bash
D=test_out/vod_model
COMMON="--extra $D/data_kills4 --extra $D/hand_data --extra $D/hand_data2 --extra $D/data_moving_dark --repeat $D/repeat_v9.txt --times 3"
python python/model/train.py python/model/configs/small_v13.json --data $D/data_v3 $COMMON --init $D/runs/small_v2/best.pt
python python/model/train.py python/model/configs/full_v3.json --data $D/data_v3 $COMMON --init $D/runs/full_v2/best.pt
python python/model/train.py python/model/configs/small_mv1.json --data $D/data_moving_dark --init $D/runs/small_v2/best.pt
```

Then the check on whole recordings of every kind against the stats files, and the export of the chosen ones
(`infer.BEST` is full_v3):

```bash
python python/model/eval_moving.py small_v11=python/model/exports/detector_small_v11.pt small_mv1=$D/runs/small_mv1/best.pt   small_v13=$D/runs/small_v13/best.pt full_v3=$D/runs/full_v3/best.pt
cp $D/runs/full_v3/best.pt python/model/exports/detector_full_v3.pt && python python/model/export.py $D/runs/full_v3/best.pt
cp $D/runs/small_v13/best.pt python/model/exports/detector_small_v13.pt && python python/model/export.py $D/runs/small_v13/best.pt
```

A run can be paused (create `PAUSE` in its folder, or Ctrl+C), resumed with `--resume test_out/vod_model/runs/<name>`,
and forked from any snapshot with `<new config> --fork test_out/vod_model/runs/<name>/snapshots/<snapshot>.pt`.

Each run writes `test_out/vod_model/runs/<name>/` with `config.json`, `metrics.jsonl` (one line per epoch, validation
scores included), `best.pt` (best validation F1) and `last.pt`. Seeds are fixed in the config. 20 epochs take about
7 minutes for small on the GPU. `--epochs N` overrides the config for a quick try.

## 3. Evaluate

Crops of the held-out test scenarios, against the automatic labels (the threshold is chosen on val, never on test):

```bash
python python/model/eval.py test_out/vod_model/runs/tiny/best.pt test_out/vod_model/runs/small/best.pt \
  test_out/vod_model/runs/full/best.pt test_out/vod_model/runs/tiny_v2/best.pt \
  test_out/vod_model/runs/small_v2/best.pt test_out/vod_model/runs/full_v2/best.pt
```

Whole KovOBS recordings of held-out scenarios, scored against the stats file's kill times (the independent check):

```bash
python python/model/eval_vods.py test_out/vod_model/runs/small_v2/best.pt
```

The two extra VODs from the weakest crop scenarios:

```bash
python python/model/eval_vods.py test_out/vod_model/runs/small_v2/best.pt --out test_out/vod_model/eval/extra --vods \
  "E:/OBS/KovOBS/Jumbo1wall9000targets/Jumbo1wall9000targets - 292 - 2026.07.04-22.03.46.mp4" \
  "E:/OBS/KovOBS/ClickTrack Vertical 2t Long/ClickTrack Vertical 2t Long - 58 - 2026.06.26-19.41.13.mp4"
```

Results land in `test_out/vod_model/eval/`.

## 4. Export to ONNX

```bash
python python/model/export.py test_out/vod_model/runs/small_v2/best.pt
```

This writes `python/model/exports/detector_<name>_fp32.onnx`, `_fp16.onnx` (the fp16 one needs CUDA to trace), `_u8in.onnx`
(raw uint8 frames in, any number at once) and `_embed.onnx` (raw bytes in, the 100 best boxes out). It checks the
fp32 file against PyTorch on a real frame, and the u8in and embed files against fp32 (the u8in one also with the frame
in a batch of 4), and stops if they differ. Then it prints the sizes. `--u8in` writes and checks the u8in file
only. To ship a model under the plain name, copy the checkpoint to `python/model/exports/detector_<name>.pt`.

## 5. Quantize

The same command also writes `detector_<name>_int8.onnx`: static QDQ quantization (per-channel int8 weights, uint8
activations) calibrated on 64 validation crops. Evaluate it like any other model:

```bash
python python/model/eval.py python/model/exports/detector_small_v2_fp32.onnx python/model/exports/detector_small_v2_int8.onnx
```

## 6. Run inference

On a VOD, printing the first frames' detections and the speed:

```bash
python python/model/infer.py python/model/exports/detector_small_v2_fp32.onnx "E:/OBS/KovOBS/1w4ts Voltaic/1w4ts Voltaic - 143 - 2026.09.30-04.55.23.mp4"
```

In code (NumPy and ONNX Runtime only):

```python
import infer                                   # python/model/infer.py
det = infer.OnnxDetector("python/model/exports/detector_small_v2_fp32.onnx", threads=4)
boxes = det(rgb, fixed, 0.3)                   # rgb (720, 1280, 3) uint8, fixed (720, 1280) 0/1 -> (n, 5) cx, cy, w, h, score
```

The same call works with every export (`_fp32`, `_fp16`, `_int8`, `_u8in`, `_embed`). Without `infer.py`, the embed
file needs nothing but ONNX Runtime:

```python
import onnxruntime as ort
sess = ort.InferenceSession("python/model/exports/detector_small_v2_embed.onnx")
(dets,) = sess.run(None, {"rgb": rgb[None], "fixed": fixed[None]})   # both uint8; dets (1, 100, 5), best first
boxes = dets[0][dets[0][:, 4] > 0.3]
```

The whole review with the model (the review app picks the model on its own when the exports exist):

```bash
bun run server
```

## 7. Benchmarks and tests

```bash
python python/model/bench.py --onnx python/model/exports/*.onnx --pt test_out/vod_model/runs/*/best.pt
python python/model/test_model.py
```

The benchmark runs each ONNX file in a fresh process at 1, 4 and 8 threads (load time, latency, peak memory, cores)
and each checkpoint on the GPU at batch 1 and 16, on a real frame. It writes `test_out/vod_model/bench.json`, and the
raw frame files the browser and native tests read.

## 8. Deployment prototypes

**Local web server (B).** Standard library HTTP, ONNX Runtime, no PyTorch:

```bash
python python/model/serve.py --port 8771
```

From a second shell, send the benchmark frame 200 times and print the round trip:

```bash
python python/model/serve.py --client --n 200
```

**Browser (C).** ONNX Runtime Web on WebGPU and WASM:

```bash
cd python/model/web && bun install && cd ../../..
python python/model/web/serve_static.py --port 8772
```

Open `http://127.0.0.1:8772/python/model/web/`. The table fills in; the same numbers are in `window.results`.

**Native (A).** Rust, from `python/model/rust`. tract (pure Rust, the default build):

```bash
cargo build --release
./target/release/kovobs-detect.exe ../exports/detector_small_fp32.onnx
```

ONNX Runtime through the `ort` crate (downloads a static ONNX Runtime once, about 341 MB):

```bash
cargo build --release --features ort
./target/release/kovobs-detect.exe --backend ort --threads 4 ../exports/detector_small_fp32.onnx
```

Each run prints the detections, whether they match Python's (`bench_expected.json`), load time, latency, cores and
peak memory. The WASM build and every option are in [rust/README.md](rust/README.md).
