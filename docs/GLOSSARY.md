# Glossary

The words the code and the docs use, each with the type that holds it. Paths are in `src/` unless they say otherwise.
Names in code use these words in full; the short forms at the end are the only ones allowed (AGENTS.md, "Readable
code").

## Recordings and runs

- **Recording.** A video file. It can hold several runs.
- **Run.** One play of a scenario, from its start to its end screen. Its kills come from its stats file, its HUD or the
  video alone.
- **Run window.** The part of a recording the user marks as the run, kept in run.json. `TimeWindow` (session.rs).
- **Run part.** A stretch of a recording that one review worker decodes and tracks, split at a key frame so two
  decoders can work at once. `Run` (session.rs). The name clashes with a run of a scenario; the refactor renames it.
- **Scenario.** KovaaK's challenge definition (a .sce file). Its facts are its kind, time limit, targets alive at once
  and ammo rules. `Facts`, `AmmoRules` (scenario.rs).
- **Scenario kind.** Static, dynamic, tracking or switching. Tracking runs get the tracking review; the others are
  reviewed as clicking runs. `Kind` (scenario.rs).
- **Stats file.** KovaaK's CSV for a run: its "Key:,value" lines and its kill table (each kill's time and shots).
  `StatsFile` (stats_file.rs).
- **Kill times.** Where a run's kills come from: the stats file, the HUD, or the video alone. `KillTimes` (review.rs),
  `KillSource` (matching.rs).
- **HUD.** The game's on-screen counters: KovaaK's session box (kill count, hits and shots), or Aim Lab's POINTS and
  TIME boxes. `HudWatch` (hud/watch.rs), `HudReading` (hud/mod.rs).
- **Countdown bar.** KovaaK's bar before a run starts. A recording may not show it. Read by `CameraWatch` (camera.rs).
- **Review version.** The version of the review that made a report. An older one is outdated. `REVIEW_VERSION`
  (track.rs).

## Frames and the screen

- **Key frame.** A frame the video can decode on its own. The fixed map, the HUD's boxes and the run parts start from
  them. `Keys` (session.rs).
- **Crosshair.** Always at the screen's center, and it never changes during a run. All places are in degrees from it:
  right and up are positive.
- **Fixed map.** Per pixel, how many key frames it stands out in. Pixels that stay put while the view turns (crosshair,
  HUD, gun model) score high. The detector's fourth input. `FixedMap` (fixed.rs).
- **Area, excluded area.** A box on screen where targets do not count: a HUD box, a clock, an overlay. `Area`
  (areas.rs), `AreaBox` (session.rs). The area finder proposes them: `AreaFinder` (areas.rs).
- **Pop-up.** An excluded area that only sometimes shows, such as a "Last kill" message. It is excluded only while it
  shows. `AreaWatch` (popup.rs).
- **Mask.** The pixels of a frame where targets count: all but the excluded areas. `Mask` (track.rs).
- **View shift.** How far the view moved since the frame before, in degrees. `ViewShift` (typescript.rs), the `shift`
  of `TrackFrame` (track.rs).
- **Camera reading.** The camera's turn since the frame before, measured from the room's move on screen by phase
  correlation over 18 tiles. `CameraWatch`, `TileShifts` (camera.rs), `CameraReading` (tracking.rs).

## Targets and tracks

- **Detector.** The neural network that finds targets in a frame (python/model/ trains it). It gives boxes. `RawBox`
  (frame pixels), `ModelBox` (degrees) (track.rs).
- **Spot.** A target in one frame: its place, its area in pixels, and its box and score. `Spot` (track.rs).
- **Keep.** The step that picks a frame's boxes that count: none in an excluded area; those near the crosshair, then
  the most confident up to the scenario's target count. `keep` (track.rs).
- **Link.** The step that gives targets ids that follow them from frame to frame. `link` (track.rs).
- **Track.** One target followed under one id. `TrackFrame`, `TrackPoint`, `Tracks` (track.rs; kept as tracks.json).
- **Gap.** Frames between two sightings of a target where it was not found.
- **Appearance.** Tracks that are one target picked up again after a gap, joined. `Appearances` (matching.rs).
- **Crosshair spot.** A fixed screen spot where the detector marks the crosshair as a target. `CrosshairSpot`
  (typescript.rs).
- **Faint cut-off.** The user's setting that leaves out tracks that score far below the recording's level.
  `FaintSetting`, `FaintCutFrames` (faint.rs; kept as faint.json).

## Kills and flicks (clicking runs)

- **Kill.** A target killed. One click kills at most one target. Some bots die on a timer, not at a hit.
- **Clock offset.** How far the kill times' clock is from the video's, in seconds. In `MatchInfo` (matching.rs).
- **Flick.** A kill and the move to it: where it starts, the shots it took and the target's path. `Flick`, `PathPoint`
  (matching.rs).
- **Kill steps.** A kill's time in the field's words: Reaction, Flick, Micro (onto the target, then settling),
  Confirmation, then the Click. `KillParts` (typescript.rs), `Measure` (measure.rs).
- **Underflick, overflick.** A flick that ends short of the target or past it.
- **TTK.** Time to kill.
- **Choice.** Whether the next target was the nearest on screen. `Choice` (measure.rs).
- **Pathing.** The order of kills against the fastest order. ui/src/app/run/fastest-path/.
- **Reloads.** The reloads a magazine forced, and their cost. `Reloads` (reload.rs).
- **Summary, check.** A clicking run's medians and shares, and each check's verdict. `Summary`, `Issue` (summary.rs).
- **What-if.** What one change would add to the score, all else the same. `ClickWhatIf` (what_if.rs).

## Tracking runs

- **Time on target.** The share of a tracking run the crosshair is on the bot. `TrackSummary` (tracking.rs).
- **Turn back.** A bot's change of direction and how long the crosshair took to follow. `TurnBack` (tracking.rs).
- **Switch.** A bot's death, when the next bot shows, and when the crosshair is on it. `Switch` (typescript.rs).
- **Around.** A frame's offset from the bot's center line, along its motion and across it. `AroundPoint`
  (typescript.rs).
- **What-if.** The time on target one change would add. `WhatIf` (tracking.rs).

## Short forms

Only these, in names and in docs:

- `deg` degrees, `px` pixels, `ms` milliseconds, `fps` frames a second.
- `hud`, `ttk`, `fov` (field of view).
- `rgb`, `yuv`, `luma` (color formats).
- `id`, `json`, `csv`.
- `i`, `j` loop counters; `x`, `y` coordinates; `a`, `b` the two sides of a comparison.
