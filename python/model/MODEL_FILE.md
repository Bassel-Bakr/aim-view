# The model's settings file

Each detector model carries a settings file beside its exports: `python/model/exports/detector_<name>.json`. The
review reads every setting that depends on how a model behaves from this file. So a retrained model needs no change to
the code.

## Format 1

```json
{"format": 1, "name": "full_v3", "threshold": 0.3, "score_map": null, "reference": "full_v3"}
```

- `format`: 1. A reader refuses any other format.
- `name`: the model's name.
- `threshold`: a cell of the score map is a target when its score, after `score_map`, is over this value. It is read
  as a float32, as the scores are.
- `score_map`: null, or a list of `[raw, mapped]` points that put the model's raw scores on the reference model's scale.
  It needs 2 points or more, every value from 0 to 1, and both values must rise from point to point. Between two points
  the map is linear; below the first point and above the last, it gives that point's mapped value. The core works it
  out as NumPy's `interp` does, in float64, and keeps the result as a float32.
- `reference`: the model whose scale the mapped scores are on (full_v3).
- `at_crosshair` (optional): `{"threshold": 0.2, "reach_px": 30}`. A cell under `threshold` but over this one (on the
  same scale) is a target too when its box's center is within `reach_px` of the crosshair (1280 x 720 pixels): a
  target being shot is under the crosshair. Without it, nothing changes. Measured on large_v11 (2026-10-06, the gate
  against itself without it): tracking's gap to the stats files 0.065 to 0.049, but the video alone worse (dynamic
  recall 0.979 to 0.951: what a dying target leaves at the crosshair keeps its track alive), so no model's file has
  it yet.

Every score the review keeps is the mapped one: the boxes' scores, the tracks' `s` (which the faint cut-off reads) and
the score that `keep` compares with 0.5. Other fields are allowed and ignored.

The files of today's models (full_v3, small_v13, small_v11) hold today's values: threshold 0.3, no map, reference
full_v3. The calibration (`python/model/calibrate.py`) writes the file for a new model.

## Who reads it

- **The core** (`src/model.rs`: `ModelSettings`). The tracker decodes the detector's maps with it (`Tracker::set_model`,
  `detect::decode`).
- **The browser.** The review worker reads `models/detector_<name>.json` beside the `_u8in` export and gives it to the
  core (`tracker_set_model`). `bun run assets` copies the files into `ui/generated/models/`.
- **The service and the desktop app.** The review reads the file beside the export in the models folder
  (`service/src/detector.rs`: `model_settings`). The desktop installer ships the files (`desktop/tauri.conf.json`).

A model with no file takes today's values (threshold 0.3, no map). The service says so once on stderr, and the browser
in the console. A file that cannot be read, or that breaks the rules above, stops the review and says why.

## Audit: the constants after the model's output

Each constant is one of four kinds:

- **File**: it depends on how a model behaves. It comes from the settings file.
- **Reference scale**: a score. It stays in the code, because every score is on the reference model's scale once mapped.
- **Contract**: the export's form (`python/model/export.py`). A model must meet it; the contract check tests it.
- **Geometry**: degrees, pixels, frames, or the run's own measurement. It stays in the code.

| Constant | Where | Kind | Why |
| --- | --- | --- | --- |
| Threshold 0.3 | `src/detect.rs` `decode` (was `THRESHOLD`) | File: `threshold` | Each model's threshold is chosen on the val split. |
| Score map | `src/detect.rs` `decode` | File: `score_map` | It puts the model's raw scores on the reference scale. |
| Cells of 4 pixels; `reg` as offsets in cells and log sizes | `src/detect.rs` `STRIDE`, `decode`; `W / 4` in `review.worker.ts` and `service/src/review.rs` | Contract | The output of `net.py`. A model with another stride is a new export format. |
| Inputs `rgb` (uint8, n x 720 x 1280 x 3) and `fixed`; outputs `score` (peaks only) and `reg` | `service/src/detector.rs`, `review.worker.ts` | Contract | The `_u8in` export's form. |
| Fixed map: `DIFF` 30, `SHARE` 0.8 | `src/fixed.rs` | Contract | The model's second input, made as in training (`review.fixed_map`). A model trained on another fixed map takes a new input. |
| Boxes within 2 degrees of the crosshair always stay | `src/track.rs` `NEAR_DEG` | Geometry | The crosshair's reach. A target under the crosshair stays whatever it scores; a model that scores such targets high loses nothing. |
| One box past the target count, at a score of 0.5 or more | `src/track.rs` `EXTRA_SCORE` | Reference scale | A score: the mapped scores keep its meaning. |
| Boxes ordered by score | `src/track.rs` `keep` | Reference scale | The map rises, so the order does not change. |
| View shift: pairs within 6 degrees, agreeing within 0.35, at most 2,500; links within 0.5 degrees | `src/track.rs` `view_shift`, `link` | Geometry | How far the view and the targets move between frames. |
| Pop-ups: a look every 2 frames; off 30% of the run, 3 times; 4 frames either side | `src/popup.rs` | Geometry | It reads the frame's pixels, not the model's output. |
| Faint cut-off offset 0.3 (0.2 to 0.6 allowed) | `src/faint.rs` `DEFAULT_OFFSET`; `service/src/api.rs`; `ui/src/app/modes/web-files/saved-faint.ts`; `browser-faint-cutoffs.ts` `LOWEST`, `HIGHEST` | Reference scale | A distance below the recording's own level, in score. The user's saved cut-offs (`faint.json`) are on full_v3's scale, so a new model's scores must be mapped onto it. |
| Faint level: 90th percentile; 3 frames or more | `src/faint.rs` `faint_scores` | Geometry | The recording's own measurement: the cut-off is relative to its level. |
| Faint scores counted from 2 degrees (clicking) or 0 (tracking) | `service/src/faint.rs`; `browser-faint-cutoffs.ts` | Geometry | The crosshair's reach, as for `NEAR_DEG`. |
| Box sizes (`wh`, the area pi/4 x w x h) | `src/track.rs` `keep` | Contract | No constant: the model's measurement of a target. The contract check should test box sizes against the labels. |
| On the bot: inside the box plus 0.05 degrees; engaged within 5 radii or 2 degrees | `src/tracking.rs` | Geometry | Degrees around the model's box (see box sizes). |
| Crosshair spots and ghosts (0.015, 0.02, 0.06, 0.1 degrees; 2% of the turning frames and 25 or more; 60%; at most 3; `ghosts(0.5, 0.5, 0.3)`) | `src/matching.rs` `crosshair_spots`, `ghosts` | Geometry | They catch a model that boxes the crosshair, but they measure it in degrees and shares of frames, not in score. |
| Kills: within 0.6 degrees and 2 frames; the video alone's radius from the median box area (0.43 degrees without), near = radius + 0.25 and 0.6 or more, spikes of 1 degree and 3 times, the same size within half and twice | `src/matching.rs` `match_times`, `match_video` | Geometry | Degrees, frames and ratios. The radius reads the model's box sizes (see box sizes). |

`src/matching.rs` was not changed here (another change was in progress). Nothing in it reads a score, and nothing in it
needs to move into the file.

These copies still use the constant 0.3 and no map, so they review a model with other settings differently:

- `python/model/infer.py` `THRESHOLD` (`decode_np` and the runners), which `python/review.py` and the eval scripts use.
- `python/model/rust/src/decode.rs` `THRESHOLD`, the KovOBS prototype.
