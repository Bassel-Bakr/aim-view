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
targets with the hand-written detector in `python/model/old_review.py`, keeps only VODs whose labels look steady, and saves
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

Moving targets on other themes (2026-10-04; checked by eye and trained on in full_v4: below). Every recording of a
dynamic, tracking or switching scenario (`--per-folder 0`) that is not dark targets on light walls (`--other-themes`:
the ones `--dark` leaves out), labelled by full_v3 instead of `dark_labels` (`--model`: its `_u8in` export on the CPU,
at the threshold in its settings file). Boxes on the HUD or KovOBS's boxes are dropped, and so are boxes more than 2.5
times wider than tall (health bars). The steadiness filter and the target-count filter are the same as for `--dark`.
`--skip-checks` leaves out every recording the stats-file checks use (eval_vods.py, eval_moving.py,
eval_video_alone.py). About 7 minutes on 14 processes. Then the contact sheet, and 150 crops picked for a check by eye
(`pick_checks.py`: spread over the recordings and kinds, most of them uncertain ones), and the check itself:

```bash
python python/model/build_data.py --kinds dynamic,tracking,switching --per-folder 0 --other-themes --model python/model/exports/detector_full_v3_u8in.onnx --skip-checks --out test_out/vod_model/data_moving_themes
python python/model/validate_data.py --data test_out/vod_model/data_moving_themes --sheet test_out/vod_model/sheet_moving_themes.png
python python/model/pick_checks.py --data test_out/vod_model/data_moving_themes --out test_out/vod_model/check_moving_themes --n 150
python python/model/label_check.py --data test_out/vod_model/check_moving_themes --n 150 --port 8774 --out test_out/vod_model/check_moving_themes/checked.jsonl
```

Crops mined from full_v3's own mistakes (2026-10-04; checked by eye and trained on in full_v4: below). `build_mined.py`
reviews recordings that have a stats file as the app does (full_v3's `_u8in` export on DirectML; each review kept in
`data_mined/reviews/`, so a rerun skips it): the newest recording of each scenario folder, dynamic and switching first,
then tracking, then static, the checks' runs left out, until `--budget` seconds of reviewing. Four rules, each strict
(the script's docstring has them in full): `kill` places the killed target where the model lost it in the third of a
second before a kill of a dynamic or switching run; `gap` fills a steadily tracked target that the model misses for 1 or
2 frames; `false_static` takes out a box that stays put on screen while the view turns; `false_lone` takes out a box
seen in one frame with nothing near it or like it. The 2026-10-04 build stopped after 215 reviews (3,106 s);
`--reviewed-only` mines only the recordings reviewed before. Then the contact sheet, 100 crops picked for a check by eye
(spread over the rules and the recordings), and the check itself:

```bash
python python/model/build_mined.py --out test_out/vod_model/data_mined --budget 3600
python python/model/build_mined.py --out test_out/vod_model/data_mined --reviewed-only
python python/model/validate_data.py --data test_out/vod_model/data_mined --sheet test_out/vod_model/sheet_mined.png
python python/model/build_mined.py --out test_out/vod_model/data_mined --pick 100 --check test_out/vod_model/check_mined
python python/model/label_check.py --data test_out/vod_model/check_mined --n 100 --port 8775 --out test_out/vod_model/check_mined/checked.jsonl
```

Both sets checked by eye, as training sets (2026-10-04, full_v4's data). The user checked every crop of both on the
phone page (`checked_phone.jsonl` in each set, label_check's format). `checked_data.py` copies each crop with its boxes
replaced by the checked ones ("skip": no target, no boxes) and its target mask made again from them (the ellipse that
fills each box), in the same split. Each name gets a 10-character tag in front (`chk_theme_`, `chk_mined_`), so
`train.py --repeat` can weight these crops alone: their recordings' hashes also start 2,428 training crops of the
same recordings in `data_v3`, `data_kills4` and `data_moving_dark`. One mined crop is left out: its checked box is
51.4 x 2.8 px on a target about 50 x 34 px (a slip on the phone page). Then the contact sheets:

```bash
python python/model/checked_data.py --labels test_out/vod_model/data_moving_themes/checked_phone.jsonl --out test_out/vod_model/data_themes_checked --tag chk_theme_
python python/model/checked_data.py --labels test_out/vod_model/data_mined/checked_phone.jsonl --out test_out/vod_model/data_mined_checked --tag chk_mined_ --leave-out train/5428426d1d_00863_g00.npz
python python/model/validate_data.py --data test_out/vod_model/data_themes_checked --sheet test_out/vod_model/sheet_themes_checked.png
python python/model/validate_data.py --data test_out/vod_model/data_mined_checked --sheet test_out/vod_model/sheet_mined_checked.png
```

full_v5's mined set is built from the user's second pass, `data_mined/checked_phone_2.jsonl` (`checked_phone.jsonl`
stays as it was). In it, 11 boxes that covered only the crosshair's dot (a bot hidden under it) moved from "boxes" to
"covered". `checked_data.py` writes those into the npz as "ignore", and `train.py` learns neither a target nor wall
there. The 3 Switching Humanoid crops (whole-robot boxes) are left out too:

```bash
python python/model/checked_data.py --labels test_out/vod_model/data_mined/checked_phone_2.jsonl --out test_out/vod_model/data_mined_checked2 --tag chk_mined_ --leave-out train/5428426d1d_00863_g00.npz,train/a1ccc534da_01368_l00.npz,train/a1ccc534da_01798_l01.npz,train/a1ccc534da_03890_l02.npz
python python/model/validate_data.py --data test_out/vod_model/data_mined_checked2 --sheet test_out/vod_model/sheet_mined_checked2.png
```

The robot set (2026-10-05): 312 crops of 29 robot runs the user played (`test_out/vod_model/hand_robots/`), checked
on the phone page with one box around each whole robot (`check_robots/`, crop_check/README.md). Two crops are left
out: one kept the model's box on a name tag beside the robot's box, one shows a robot's arm and leg at the crop's edge
with no box. One is unsure. That leaves 309 crops: 184 robots, 136 crops without one.

```bash
python python/model/crop_check/labels.py test_out/vod_model/check_robots test_out/vod_model/check_robots/answers/checks test_out/vod_model/hand_robots/checked_phone.jsonl robots
python python/model/checked_data.py --labels test_out/vod_model/hand_robots/checked_phone.jsonl --out test_out/vod_model/data_robots_checked --tag chk_robot_ --leave-out train/Switching_Humanoid_-_120529.50_-_2026.10_a3b9a0_002.npz,train/OW_Mirror_-_2591.96_-_2026.10.05-02.51.4_f649f4_006.npz
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

full_v4 (2026-10-04): full_v3 fine-tuned for 4 epochs at a third of its learning rate (`full_v4.json`: 0.0005), on
full_v3's data plus the two checked sets (step 1), their crops counted 3 times (`repeat_full_v4.txt`: `repeat_v9.txt`
plus the lines `chk_theme_ 3` and `chk_mined_ 3`). About 7 minutes on the RTX 5070 Ti (`best.pt` is epoch 3). Then the
export and the gate, without `--list` (MODEL_STATUS.md, "full_v4"):

```bash
D=test_out/vod_model
(cat $D/repeat_v9.txt; printf 'chk_theme_ 3\nchk_mined_ 3\n') > $D/repeat_full_v4.txt
python python/model/train.py python/model/configs/full_v4.json --data $D/data_v3 --extra $D/data_kills4 --extra $D/hand_data \
  --extra $D/hand_data2 --extra $D/data_moving_dark --extra $D/data_themes_checked --extra $D/data_mined_checked \
  --repeat $D/repeat_full_v4.txt --times 3 --init $D/runs/full_v3/best.pt
python python/model/export.py $D/runs/full_v4/best.pt
python python/model/accept.py full_v4
```

full_v4c, the control: full_v4's config and command (`full_v4c.json` differs only in its name) without the two
checked sets, with the plain `repeat_v9.txt`. It tells the fine-tune itself from the checked crops:

```bash
python python/model/train.py python/model/configs/full_v4c.json --data $D/data_v3 --extra $D/data_kills4 --extra $D/hand_data \
  --extra $D/hand_data2 --extra $D/data_moving_dark --repeat $D/repeat_v9.txt --times 3 --init $D/runs/full_v3/best.pt
python python/model/export.py $D/runs/full_v4c/best.pt
python python/model/accept.py full_v4c
```

full_v5: full_v4's recipe at half its learning rate (`full_v5.json`: 0.00025; full_v4c showed that 0.0005 alone fails
the gate), with `data_mined_checked2` (step 1) in place of `data_mined_checked`:

```bash
python python/model/train.py python/model/configs/full_v5.json --data $D/data_v3 --extra $D/data_kills4 --extra $D/hand_data \
  --extra $D/hand_data2 --extra $D/data_moving_dark --extra $D/data_themes_checked --extra $D/data_mined_checked2 \
  --repeat $D/repeat_full_v4.txt --times 3 --init $D/runs/full_v3/best.pt
python python/model/export.py $D/runs/full_v5/best.pt
python python/model/accept.py full_v5
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

The video-alone kill finder (a clicking run's kills from the video alone, with no stats file and no HUD:
`src/matching.rs`, `match_video`) on the 48 runs in `video_alone_runs.json` (one run per scenario: 29 for development,
19 of held-out scenarios), scored against each run's stats file (a kill within 3 frames, one to one). It takes the
model's name, or a model file with its _u8in export beside it (step 4). It tracks each run once in the app's native
review (about 15 minutes for full_v3 on the RTX 5070 Ti) and keeps the tracks in
`test_out/vod_model/eval/video_alone/<model>/`, so after a change to the finder it only scores again (under a minute).
`--retrack` tracks again after a change to the tracking. It writes `test_out/vod_model/eval/video_alone_<model>.json`:

```bash
python python/model/eval_video_alone.py full_v3
```

full_v3 (2026-10-04): recall 0.945 and precision 0.955 on 47 runs (one has no clock offset); held out, 0.973 and 0.967.

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
Last, it calibrates the model and writes its settings file (see "After a training run" below).

## 5. Quantize

The same command also writes `detector_<name>_int8.onnx`: static QDQ quantization (per-channel int8 weights, uint8
activations) calibrated on 64 validation crops. Evaluate it like any other model:

```bash
python python/model/eval.py python/model/exports/detector_small_v2_fp32.onnx python/model/exports/detector_small_v2_int8.onnx
```

## After a training run: settings file and contract

A new model needs no change to the code. It brings a settings file, and it must meet a contract. After training
(step 2) and the checks against the stats files (step 3), four commands:

```bash
cp test_out/vod_model/runs/<name>/best.pt python/model/exports/detector_<name>.pt
python python/model/export.py test_out/vod_model/runs/<name>/best.pt      # the exports, then the settings file
python python/model/contract.py <name>                                       # the contract
python python/model/accept.py <name> --list                                  # the acceptance gate (last)
```

### The settings file

`python/model/exports/detector_<name>.json` holds the model's threshold and score map. [MODEL_FILE.md](MODEL_FILE.md)
gives its format and who reads it. `export.py` writes it only when no file is there: it never replaces one. The
numbers behind it go to `python/model/reports/calibration_<name>.json`. To calibrate again without exporting:

```bash
python python/model/calibrate.py <name> --report python/model/reports/calibration_<name>.json   # prints the settings
python python/model/calibrate.py <name> --write                              # writes the file if there is none
```

The review reads scores in three places: the threshold (0.3), the one box past the target count (0.5 or more), and
the faint-target cut-off. Those numbers were set on full_v3, the reference model. The score map puts a model's raw
scores on full_v3's scale, so that the same score means the same precision: the same share of the boxes with that
score are targets. The calibration (`calibrate.py`):

- runs the model's `_u8in` export on the val crops of `data_v3`, `data_kills4` and `data_moving_dark` (8,559 crops of
  60 scenarios, every kind; the models were picked on them, and none trained on them), and matches every peak to the
  labels as `eval.py` does;
- fits each model's precision at each score, rising with the score (isotonic regression on bins 0.01 wide);
- maps a raw score to the full_v3 score with the same fitted precision, at the full_v3 scores 0.05, 0.10, ... 0.95;
- fits again on 400 draws of the 60 scenarios (both models on the same draw). A score moves only as far as the 95%
  range of the draws demands, and not at all when that range holds it. Moves under 0.01 are dropped. A model with no
  move left gets no map (null);
- picks the threshold as `infer.THRESHOLD` was picked: `eval.py`'s sweep (0.2 to 0.6, the best F1 on the val split of
  `test_out/vod_model/data`, where full_v3's best is 0.3), on the mapped scale. A threshold other than 0.3 is taken
  only when its F1 is better on 95% of the draws of that split's scenarios.

Why precision, and not the scores of true targets (the same recall at the same score)? Matching recall pushes a model
that finds fewer targets down to scores where most boxes are false. small_v13 would keep boxes from raw 0.21 up, where
about a fifth of its boxes are targets, to reach full_v3's recall at 0.3. small_v11, which never saw a moving target,
would map its 0.05 to 0.35. The review's numbers rely on what a score means; recall is the model's quality, which the
contract and the stats-file checks judge.

What the calibration gives today's models (the files keep today's values, threshold 0.3 and no map; the proposals
are in `python/model/reports/`):

| Model | Threshold | Score map |
| --- | --- | --- |
| full_v3 | 0.3 | none (the reference) |
| small_v13 | 0.3 | raw 0.5205 to 0.55, 0.5652 to 0.6, 0.6354 to 0.65; no move under 0.5 or over 0.7 |
| small_v11 | 0.3 (0.5 has the best F1, 0.9483 against 0.9469, but is better on only 56% of the draws) | none |

So at the threshold all three agree with full_v3. small_v13's scores from 0.5 to 0.65 are more precise than full_v3's
(its boxes scoring 0.5 to 0.6 are targets 85% of the time, full_v3's 71%): the map moves them up by at most 0.035. The
full results: `calibration_small_v13.json` and `calibration_small_v11.json`.

### The contract

```bash
python python/model/contract.py <name> [--settings FILE] [--report FILE]
```

It checks the model's `_u8in` export with its settings file (or with `--settings`, another settings file or a
calibration report). It writes `python/model/reports/contract_<name>.json` and exits with 1 when a check fails. It
keeps the recordings' camera readings and each export's peaks on them in `test_out/vod_model/contract/`, so a second
run takes a few minutes; the first takes about a quarter of an hour on the CPU.

Each check tests something the review relies on. Each limit compares the model with full_v3 on the same data:

| Check | What it measures | Limit |
| --- | --- | --- |
| export | Inputs `rgb` and `fixed` (uint8, a free batch axis), outputs `score` (peaks only, 0 to 1) and `reg` at 4 px a cell; a frame in a batch of 4 gives what it gives alone; a settings file the pipeline accepts; the fixed map made as for training (`DIFF` 30, `SHARE` 0.8) | all of them |
| crosshair | On the 8 static recordings of `eval_moving.py`: the turning pairs (the room moved 0.5 degrees or more since the frame before, and half that either side; `review.camera_motion`) with a box on the fixed map's crosshair that stayed put while the room moved | the pairs over full_v3's, recording by recording: at most 0.5% of all the pairs, and under 2% on any one recording |
| screen_fixed | The same on the fixed map's other parts (the HUD) | as for crosshair |
| boxes_per_frame | The most boxes in one frame of those recordings | 50 (`link` compares at most 2,500 pairs of boxes) |
| box_fit | Centre error (median, 90th percentile) and width and height (medians, over the label's) against the val labels; centre error against the 40 held-out hand-labelled targets (`hand_data`, `hand_data2` test splits) | within 2 standard deviations of full_v3's number over 400 draws of the val scenarios |
| one_box | Second boxes inside a found target's box, per target found | as box_fit, but never under 1 point |
| under_crosshair | Kill-moment labels touching the fixed map (a target under the crosshair) found | as one_box |
| calibrated | Precision in each band of mapped scores (0.3, 0.4, ... 0.8 to 1.0) against full_v3's | as one_box, either way, with 50 boxes or more in the band |
| targets_found | Recall at the threshold on static, kill-moment and moving val crops | as one_box |

Why these limits. A camera reading can jump for one frame (a shot's flash), so a turn must last; a frame where most
boxes stayed put is left out (the reading is wrong there). small_v10, which took KovaaK's crosshair for a target, boxes
it in 14.9% of 1w4ts's turning pairs, where full_v3 boxes it in none; the review's crosshair-spot rule
(`src/matching.rs` `crosshair_spots`) only engages when such boxes pile up in 2% of the turning frames, and the tracking
summary has no such rule. Two standard deviations of full_v3's own number is the range it keeps on 95% of the draws
of scenarios, so a model that fails is worse than full_v3 by more than full_v3's own results can tell apart. For shares
the gap is never under 1 point: full_v3 finds 99.8% of the kill-moment targets (spread 0.1 point), and a smaller gap is
a few labels in 2,500, which the stats-file checks do not resolve.

Results (2026-10-04):

| Check | full_v3 | small_v13 | small_v11 |
| --- | --- | --- | --- |
| export | pass | pass | pass |
| crosshair: share of the 5,179 turning pairs | 20.3% (valorant 99.2%, 849.91 0.9%, others 0) | 18.5%; **fails**: 1w4ts 2.8% where full_v3 has 0 | 20.3%; **fails**: 0.64% more than full_v3 (1w4ts 1.5%, 849.91 2.5%) |
| screen_fixed | 0 | 0.08% | 0 |
| boxes_per_frame | 11 | 15 | 21 |
| box_fit: centre error median, p90 (px) | 0.49, 1.33 | 0.47, 1.34 | 0.44, 1.22 |
| box_fit: width, height over the label's | 0.96, 0.99 | 0.98, 0.98 | 0.98, 1.00 |
| box_fit: hand labels, centre error median; found, false boxes | 0.72 px; 32 of 40, 9 | 0.67 px; 31, 17 | 0.64 px; 31, 18 |
| one_box | 2.5% | 2.1% | 0.2% |
| under_crosshair | 99.9% | 99.7% | 99.6% |
| calibrated: precision at 0.4 to 0.5, 0.5 to 0.6 | 0.46, 0.71 | 0.57, 0.85: **fails** at 0.5 to 0.6 (gap 0.14, allowed 0.11); with the calibrated map 0.57, 0.81: pass | 0.54, 0.81 |
| targets_found: static, kill moments, moving | 0.964, 0.998, 0.965 | 0.945, 0.995, 0.948 | 0.947, 0.996, **0.844 fails** |
| **Contract** | **meets it** | **fails** (crosshair; calibrated with today's file) | **fails** (crosshair; moving targets) |

What the results say:

- **full_v3 boxes the crosshair on the valorant run** (1wall 2targets xsmall, 558.46) in 99% of the turning pairs, at
  a score of about 0.6 and 6.6 px, and on 1wall 6targets 849.91 in 0.9% (scores 0.3 to 0.4). The crosshair-spot rule
  keeps those boxes from being taken for the killed target, which is why every kill there matches. So "the crosshair
  is never a target" does not hold for full_v3 itself; the check compares each model with it.
- **small_v13 boxes KovaaK's crosshair on 1w4ts** in 2.8% of the turning pairs (scores about 0.3 to 0.4), where full_v3
  boxes it in none. That is above 2%, the share at which the review's rule engages, but counted on turns of 0.5
  degrees a frame or more; whether the rule finds that spot over all the turning frames it reads was not checked.
- small_v13's precision at 0.5 to 0.6 is above full_v3's; with the map `calibrate.py` proposes it passes. Its file
  holds no map today.
- small_v11 misses moving targets (it never trained on them), as `MODEL_STATUS.md` says.

### The acceptance gate

```bash
python python/model/accept.py <name> [--list]
```

The last step: it decides whether the model may reach the app. It runs the contract, then the three checks against
the stats files (step 3), all through the app's native review, and compares every number with the best model's (the
model `models.json` marks as default, else `infer.BEST`: full_v3). It prints one table, pass or fail, with the numbers
that say why. It writes `python/model/reports/accept_<name>.json` and exits with 1 on a fail.

| Check | Data | Limit, against the best model |
| --- | --- | --- |
| contract | `contract.py` (its report goes to `test_out/vod_model/accept/<name>/contract.json`) | every check passes |
| moving: kills matched | `eval_moving.py`'s recordings, per kind: static, dynamic, switching | no drop, by a single kill |
| moving: flicks measured | the same | lower by at most 2 SD of the best model's share over 400 draws of its kills |
| moving: tracking | its 12 tracking runs: time on the bot minus the stats file's accuracy, the mean size and the mean's distance from 0 | worse by at most 2 SD of the best model's number over 400 draws of the runs |
| report: kills matched, flicks measured | `eval_vods.py`'s 4 static recordings, the app's own report | as for moving |
| video_alone: recall, precision | `eval_video_alone.py`'s 48 runs, overall and per kind | lower by at most 2 SD of the best model's share over 400 draws of its kills (recall: the stats kills; precision: the video's kills) |

Why these limits. Both models are measured on the same recordings, so what makes one recording harder than another
cancels out. What is left is chance at the level of the kill, the unit these checks count (as `contract.py` draws the
hand-labelled crops themselves). Today the margins are 5 flicks of 854 on static runs, 12.5 of 707 on dynamic runs, 8.4
of 405 on switching runs, and 0.5 to 2.6 points of recall and precision. Draws of the recordings (the contract's unit
for the val crops) would allow 6 to 7 points of flicks on dynamic and switching runs, where one run each (360 Tracking
OW2 at 5 flicks of 10, Smoothbot Switch Robots at 41 of 56) sets the spread: no small margin. A tracking run gives one
accuracy, so there the run is the unit: 0.031 on the mean size of the gap, 0.055 on the mean.

The gate reuses the tracks the scripts keep (`moving_<name>_native.pkl`, `video_alone/<name>/`) and tracks only what
is missing, as the scripts do. A cache made before the export, or before a settings file that changes the scores,
stops it. It reviews the four report recordings again (`eval_vods.py` keeps nothing), unless it reviewed them with the
same export, settings and review program before. It builds the review programs once and runs a copy of them
(`test_out/vod_model/accept/<name>/bin/`), so both models are reviewed by the same code. It writes no script's result
file.

`--list` adds a model that passes to `models.json`, before "hand", with what the gate measured (parameters, size,
checks, and where the report is), so `bun run assets` ships it. The speeds and the words about the model are yours to
add. It never edits `models.json` on a fail, and never changes the default model: that is the one line
`"default": "<name>"` in `models.json`, which `infer.py` (`BEST`), the service and the browser all read, and the
desktop app's installer bundles (`bun run assets` copies the listed models and `models.json` into `ui/generated/models/`).
So a new model needs no change to code: export it, pass the gate, list it, and set the default if you want it.

Results (2026-10-04):

| Check | full_v3 against itself | small_v13 against full_v3 |
| --- | --- | --- |
| contract | meets it | **fails**: crosshair (1w4ts 2.8%, allowed 2%), calibrated (0.5 to 0.6: 0.85 against 0.71, allowed gap 0.11) |
| moving: kills matched, flicks measured | static 854, 848 of 854; dynamic 694, 670 of 707; switching 403, 388 of 405 | static 854, 847; dynamic **693**, 669; switching 403, 380 (8 fewer, allowed 8.4) |
| moving: tracking, mean size and mean | 0.088, -0.033 | 0.097, -0.046 (pass) |
| report: kills matched, flicks measured | 496, 494 of 496 | 496, 493 |
| video_alone: recall, precision | 0.945, 0.955 | **0.909, 0.906** (static and dynamic fail too; switching passes) |
| **Verdict** | **pass**, no number differs | **fail** |

small_v13's video-alone tracks were made fresh by the gate (about 20 s a run); the finder's rules were tuned on
full_v3's tracks, so part of that gap may be the finder's tuning rather than the model.

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
