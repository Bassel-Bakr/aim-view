# Code map

Where each thing lives in the Rust code: each file's header comment, then its public types (with the first sentence of
their doc and their methods), functions and constants. tests/code_map.rs builds it from the sources and fails when this
file is out of date; `CODE_MAP_WRITE=1 cargo test --profile quick --test code_map` writes it again. Who calls what is
rust-analyzer's call hierarchy.

## Files

- `src/areas.rs`: The area finder: a recording's overlay areas (HUD boxes, clocks, a webcam) for the run page's Excluded
  areas editor, and what each one is (python/areas.py, all of it).
- `src/camera.rs`: How the camera turned, from the video alone (python/retired/review.py: `camera_motion`), and whether
  KovaaK's countdown bar shows (`countdown_showing`).
- `src/capped.rs`: A list of at most N items, kept in an array with its length: a list with a small fixed limit needs no
  heap.
- `src/convert.rs`: A decoded frame (YUV 4:2:0, any size) converted as ffmpeg 8.1 converts it for the review:
  `scale=1280:720:flags=area` to `rgb24` (the detector's input) or `yuv420p` (the fixed map's), byte for byte as the
  ffmpeg CLI does on x86-64 (the old review, python/retired/review.py: `rgb_frames`, `_frames`).
- `src/dates.rs`: Civil dates (year, month, day of the Gregorian calendar) as days since 1970-01-01 and back, by Howard
  Hinnant's algorithms, which count years from March so that a leap day ends the year.
- `src/detect.rs`: The detector model's output maps as boxes (python/model/infer.py: `decode_np`, for the `_u8in` and
  `_fp32` exports, whose score map already holds only the peaks).
- `src/faint.rs`: The faint-target cut-off (python/retired/review.py: `faint_scores`, `without_faint`;
  python/model/hand_crops.py: `cutoff_crops`): each track's score, the recording's level, the tracks the user's cut-off
  leaves out, and the detector labels a submitted cut-off gives.
- `src/fixed.rs`: The fixed map: the pixels that stay put on screen while the view moves (crosshair, HUD text, a gun
  model), found in the recording's key frames.
- `src/geometry.rs`: The frame's geometry at the size the review works in (1280 x 720): where the crosshair is, and how
  a pixel maps to an angle from it (python/retired/review.py: `W`, `H`, `CX`, `CY`, `K`, `to_deg`, `to_px`).
- `src/hud/glyphs.rs`: What each frame keeps of the HUD (src/hud/mod.rs): its value rows' glyphs, each row's kept once
  while it stays the same from frame to frame, so a long recording stays small (`GlyphStore`).
- `src/hud/layout.rs`: Where the HUD's boxes and their text rows are (src/hud/mod.rs): the grey levels and ink of a
  scaled region, KovaaK's session box found in the key frames' median (a flood fill from a patch of the box's level) and
  its value rows, and each row's glyphs cut apart; Aim Lab's POINTS and TIME boxes' glyphs too.
- `src/hud/mod.rs`: A recording's on-screen HUD, read frame by frame for a run without a stats file (python/hud.py's
  algorithm): KovaaK's session box (Kill Count, and Accuracy as hits/shots) or, when there is none, Aim Lab's POINTS and
  TIME boxes.
- `src/hud/reading.rs`: Reading the HUD's counts (src/hud/mod.rs): the digits learned from the recording itself (the
  glyphs' shapes, and KovaaK's Kill Count counting up one kill at a time, Aim Lab's timer down one second at a time),
  then the Kill Count's kill frames, the Accuracy line's shots and hits, or Aim Lab's POINTS steps.
- `src/hud/scale.rs`: The HUD's scaling (src/hud/mod.rs): a frame's region resampled as ffmpeg's `scale` filter does
  (area, bilinear and bicubic taps along each axis), the glyphs cut from it made GLYPH_WIDTH_PX x GLYPH_HEIGHT_PX, and a
  limited-range Y stretched to 0..255.
- `src/hud/watch.rs`: The HUD watch (src/hud/mod.rs): the key frames' layout, then each frame's glyphs, a run part at a
  time, its part saved as JSON and the parts joined, then the reading.
- `src/kill_check.rs`: The kills the video alone gives (matching.rs `match_video`), checked in the frames round them: a
  target that dies leaves wall where it was, while a target the tracking only lost, or a crosshair the detector boxed,
  still shows there.
- `src/lib.rs`: Aim View's review core: it finds the targets in a recording's frames, follows them, and measures the
  aim.
- `src/local_config.rs`: Aim View's settings for a checkout of the repo and this computer: aimview.defaults.json (in
  git, built in here: the project's own layout and where KovaaK keeps its folders under Steam's) under aimview.json
  (beside it at the repo's root, out of git, optional: this computer's own, such as the recordings' folder).
- `src/matching.rs`: Each kill matched to the target it killed, and the flick to it (python/retired/review.py:
  `match_times`, `_attach_kills`, `appearances`, `crosshair_spots`, `ghosts`).
- `src/measure.rs`: Each flick of a clicking run measured (the old review, python/retired/review.py: `target_radius`,
  `measure`, `choices`): how long each kill step took, how fast the crosshair moved, where it ended and where it
  clicked.
- `src/model.rs`: The detector model's settings file (python/model/MODEL_FILE.md): `detector_<name>.json` beside the
  model's exports, so a retrained model needs no change to the code.
- `src/mouse.rs`: The raw mouse log and what is measured from it (a port of python/mouse_read.py): each flick of a
  KovaaK's run, matched with the run's stats file.
- `src/optional_fields.rs`: A part of a struct whose fields are written into it all or none (`#[serde(flatten)]` over an
  optional part).
- `src/popup.rs`: Excluded areas that only sometimes show (a "Last kill" pop-up) are excluded only while they show
  (python/retired/review.py: `AreaWatch`).
- `src/py_random.rs`: Python's `random.Random` seeded with a string (CPython's Mersenne Twister, seeded from the string
  and its SHA-512), and MD5 as hashlib gives it.
- `src/python.rs`: Arithmetic done the way Python and NumPy do it, where the last bit of a result can reach a report:
  rounding to a number of decimals, CPython's `math.hypot`, and NumPy's sums, means, medians and percentiles.
- `src/reload.rs`: The reloads a clicking run's magazine forced (src/scenario.rs: `AmmoRules`), worked out from each
  kill's shots.
- `src/review.rs`: The review of a run (python/retired/review.py: `review`): a clicking run's flicks, or a tracking
  run's time on the target.
- `src/scenario.rs`: What a scenario file (.sce) says about a run (python/retired/review.py: `scenario_facts`,
  `target_counts`): its kind, its time limit, how many targets are alive at once, the player's weapon's ammo rules, and
  the bots' hitbox (its shape and its width over its height).
- `src/scipy.rs`: What the review uses from SciPy's `ndimage`, done the way SciPy does it, so results match bit for bit.
- `src/session.rs`: A review session: everything a review does between the decoder and the detector, the same for the
  browser and the desktop.
- `src/shapes.rs`: The shapes a target is drawn with on a crop: KovaaK's two, as their outline on screen (the targets
  are 3D).
- `src/statistics.rs`: The statistics a report gives: means, spreads and medians, in plain floating point (python.rs has
  NumPy's, where its order of operations reaches a result).
- `src/stats_file.rs`: KovaaK's stats file (review.py: `load_stats`): its "Key:,value" lines and its kill table.
- `src/summary.rs`: A clicking run's summary and its checks (the old review, python/retired/review.py: `summarize`,
  `_fitts`, `judge`).
- `src/track.rs`: Tracking: the detector's boxes kept or dropped per frame (python/retired/review.py: `track_model`'s
  `keep`), and the targets of each frame given ids that follow them from frame to frame (`link`).
- `src/track_checks.rs`: A tracking run's checks: each a Work on / Fine verdict, like a clicking run's (src/summary.rs
  `judge`).
- `src/tracker.rs`: The track step for one recording, or one run of it (a recording split into runs, reviewed at once):
  each frame's boxes as the detector gave them, its excluded areas watched for pop-ups, then, when the frames are in,
  each frame's boxes kept or dropped (those under a pop-up that is off kept again) and all linked.
- `src/tracking.rs`: A tracking run's summary (the old review, python/retired/review.py: `track_summary`,
  `track_motion`, `what_if`, `stats_length`, `countdown_end`, `tracking_crosshair`, `without_crosshair`): how the
  crosshair stayed on the target, from the tracks and the camera's turn.
- `src/typescript.rs`: Names for the tuples the reports write, for the TypeScript types only (ts-rs, feature `ts`): the
  UI names every tuple type, and ts-rs writes a field's tuple in place.
- `src/wasm.rs`: The core's interface to the browser (WebAssembly builds only): plain exports over the module's memory,
  so no binding generator is needed.
- `src/what_if.rs`: What would raise a clicking run's score.
- `service/src/api.rs`: The review server's API (python/retired/server.py's), free of any web framework: a request's
  method, path and query, Range header and body in (`ApiRequest`), the status, headers and body out (`ApiResponse`).
- `service/src/areas.rs`: The areas a review leaves out (python/retired/server.py: exclude, set_exclude, kinds,
  save_kind, find_areas, labelled): a webcam, another player's overlay.
- `service/src/batch.rs`: Files sent in one body, both ways between the page and the service: each [u32 path
  length][path, UTF-8][f64 time of change, seconds since 1970][u32 length][bytes], little-endian.
- `service/src/bin/aimview-tool.rs`: aimview-tool: the review service's library and its native review from the command
  line, for the Python scripts (python/aimview_tools.py runs it).
- `service/src/config.rs`: What a library needs to know (`Config`): where it keeps its files, the user's folders (the
  recordings, KovaaK's stats files and scenarios), the models, the device the detector runs on and where ffmpeg comes
  from.
- `service/src/crops.rs`: The Crops page's files: the check folders of detector crops
  (python/model/crop_check/make_page.py writes each: its crops.json, sets.json and crops/<id>.png), the user's answer to
  each crop (answers/checks/<id>.json, the file the claude.ai check pages' answers were saved to), and the answers moved
  between modes as one document (browser mode exports it, the review server imports it).
- `service/src/database.rs`: The store as one SQLite database in the data folder (docs/storage-design.md): what
  store.rs's `Files` keeps as files, kept as rows instead, the same bytes in and out, so every answer is the same.
- `service/src/detector.rs`: The detector model on this computer, on the device the configuration says (config.rs:
  `Device`): ONNX Runtime with DirectML (any Windows GPU), with CUDA (an NVIDIA GPU, with the `cuda` feature) or on the
  CPU; `Auto` tries the GPU first and falls back to the CPU.
- `service/src/disk.rs`: The file system and the clock, for the whole service: every file the library reads or writes
  goes through here.
- `service/src/faint.rs`: The faint-target cut-off (python/retired/server.py: faint, set_faint, submit_faint,
  skip_faint, faint_queue).
- `service/src/ffmpeg.rs`: ffmpeg for the review, from where the configuration says (config.rs: `Ffmpeg`): the PATH, a
  folder, or found the way KovOBS finds it: the PATH's ffmpeg and ffprobe when both run, else (ffmpeg-sidecar)
  downloaded into a folder the first time a review needs it, and unpacked there (the desktop app does not ship it).
- `service/src/finder.rs`: The area finder (src/areas.rs, python/areas.py) on a recording: its frames read, and what it
  found kept with the recording as python/areas.py keeps it (store.rs; areas.json: the found areas; areas_maps.npz: the
  stand-out and change maps).
- `service/src/gpu_frames.rs`: A run's frames decoded on the GPU (Windows): Media Foundation decodes the recording into
  D3D11 textures, and a compute shader (gpu_frames.hlsl) makes the detector's 1280 x 720 RGB with src/convert.rs's 2:1
  integer arithmetic, the 720p luma the camera reads (the same means), and the Y plane's top rows the HUD reads (only
  those: the whole plane, 3.7 MB a frame, was most of what the CPU copied back).
- `service/src/labels.rs`: Labelling (python/retired/server.py): the recordings the user marked as another game, the
  queue of recordings to label areas in and the ones skipped there, kept as the review server keeps them (store.rs:
  sorted lists of recording ids, not_aim_trainer.json and label_skipped.json in its data folder).
- `service/src/lib.rs`: Aim View's review service: the review server's API (python/retired/server.py's) over a library
  of recordings, with the review run natively (ffmpeg's frames, the core, and the detector on the GPU).
- `service/src/library/browser.rs`: The browser build's own routes (api.rs): the page runs the review and the area
  finder itself and sends what they give, which is kept as the native review keeps it; it adds raw mouse logs, chooses
  the VODs folder (a folder it mounted), sends KovaaK's files the user chose, read once, and the detector labels a
  cut-off's submit made.
- `service/src/library/export.rs`: Recordings shared as one zip (docs/storage-design.md, "Export"): what the service
  keeps of each, for the page to write into the zip beside the videos, and what the page gives back when it opens one.
- `service/src/library/links.rs`: Recordings added from a link: a video's page on a site yt-dlp reads (YouTube, Twitch,
  Medal, Streamable...) or a video file's address.
- `service/src/library/mod.rs`: The library: the user's recordings (the VODs folder and the uploads), KovaaK's stats
  files and scenarios, the models, and each recording's reviews, kept in the data folder (config.rs: `Layout`).
- `service/src/library/names.rs`: File names and time stamps: a recording's name as KovOBS writes it, a stats file's as
  KovaaK writes it, their time stamps, and a recording's folder name (python/retired/server.py: NAME, STATS_NAME,
  stamp_seconds, cache_dir).
- `service/src/library/recordings.rs`: The recordings: the list (python/retired/server.py: Library.list), a recording's
  video from its id and its folder, videos and stats files added from the user's computer, and each scenario's facts
  from its scenario file.
- `service/src/library/reviews.rs`: A recording's reviews (each model's kept apart, store.rs: tracks, readings, what the
  HUD read, the kills' check): the review on show, the review jobs (each runs in a thread of its own; in the browser
  build the page runs it, browser.rs), the user's run window and the report, worked out when it is shown
  (python/retired/server.py: shown, analyse, run, set_run, /api/report).
- `service/src/library/settings.rs`: What the user set, kept in settings.json: the VODs folder they chose in the app
  (`vods`), the model new reviews use (`model`), the device the detector runs on (`device`) and the frames it takes at
  once on each device (`batch`, by device name).
- `service/src/library/stats.rs`: KovaaK's stats files and each recording's pairing with one (python/retired/server.py:
  stats_index, stats_for, stats_of, stats_info, set_stats): the user's choice (stats.json in the recording's folder),
  else one uploaded beside it, else the stats file of the same scenario whose time is nearest the recording's.
- `service/src/library/usage.rs`: What the library keeps and how much space each part takes, for the data panel
  (docs/storage-design.md, "Space and cleanup"), and removing a part the user can do without.
- `service/src/mouse.rs`: A recording's measures from the raw mouse logs (python/mouse_log.py's, or the desktop app's
  logger's, in the layout's mouse folder): the newest log that covers the recording's run, read by the core
  (src/mouse.rs, as python/mouse_read.py reads it).
- `service/src/npz.rs`: NumPy's .npz files as python/ writes them with `np.savez_compressed` (a zip of .npy arrays,
  deflated): the cut-off's detector labels (faint.rs) and the area finder's maps (finder.rs), so Python's tools read
  what the app writes and the app reads what Python wrote.
- `service/src/pyjson.rs`: JSON as python/retired/server.py read and wrote it, so the files the app keeps are the review
  server's, byte for byte.
- `service/src/report.rs`: A review's report, worked out by the core as the browser does (src/review.rs: `review_json`),
  from what the review keeps (store.rs: `Part`): its tracks, readings and what the HUD read (a review made before the
  HUD was read has none).
- `service/src/review.rs`: A recording's review on this computer: ffmpeg decodes the frames and the core converts them
  to ffmpeg's 720p RGB byte for byte (or, for the videos gpu_frames.rs takes, the GPU does both), the detector runs on
  the GPU or the CPU (detector.rs), and the core's review session (aimview::session, which the browser's workers feed
  the same way) does the rest: it plans the runs, reads the key frames, tracks each run's frames, watches the camera's
  turn and the HUD, and joins the runs.
- `service/src/run_window.rs`: The user's run window for a recording: where the run starts and ends, kept with the
  recording (store.rs; as python/retired/server.py kept it, run.json in its folder).
- `service/src/sql.rs`: The SQL the data folder's database runs (database.rs), behind one interface (`Sql`) so its
  statements are written once (docs/storage-design.md): natively SQLite built into the exe through rusqlite (`Sqlite`),
  in the browser build SQLite's own WebAssembly in the page (`HostSql`, the `host_sql` import, in a binary form both
  sides read: tagged values, rows as a column and a row count before them).
- `service/src/store.rs`: What the library keeps, through one interface (`Store`), so where it is kept can change
  (docs/storage-design.md): the files in the data folder (`Files`, laid out as config.rs's layout says) or one SQLite
  database (database.rs).
- `service/src/video.rs`: A recording's frames from ffmpeg (ffmpeg.rs: the PATH's, a folder's or a downloaded one), as
  Python's review decoded them (python/retired/review.py: `_frames`): the video's own YUV 4:2:0 at its size, through a
  pipe, so the core converts them to the same bytes.
- `service/src/ytdlp.rs`: yt-dlp, for recordings added from a link (library/links.rs): found as ffmpeg is (ffmpeg.rs),
  the PATH's when it runs, else the official release from GitHub, downloaded once into the tools folder beside ffmpeg's.
- `server/src/access.rs`: Who may use the server.
- `server/src/config.rs`: The server's settings: the command line, over a settings file (TOML), over the defaults.
- `server/src/files.rs`: The UI's files (its server-mode build).
- `server/src/glue.rs`: The review service (aimview-service) behind the HTTP side: the settings as its `Config`, each
  call as its `ApiRequest`.
- `server/src/http.rs`: The HTTP side: each request is checked (access.rs), then goes to the review API or to the UI's
  files.
- `server/src/main.rs`: Aim View's review server: the UI's server-mode build and the review server's API
  (python/retired/server.py's, served by the aimview-service crate as the desktop app serves it) over plain HTTP.
- `desktop/src/lib.rs`: Aim View's desktop app: the Angular app (ui/, its desktop build) in a Tauri 2 window.
- `desktop/src/main.rs`: Aim View's desktop app: the executable, which starts the app (lib.rs `run`).
- `desktop/src/mouse.rs`: The raw mouse logger (python/mouse_log.py, ported) and the app's side of it: an on/off switch
  that logs in the background while the user plays.
- `desktop/src/protocol.rs`: The review server's API inside the app, over a custom protocol (`api`, at
  http://api.localhost in the window): no network port, so nothing outside the app reaches it.
- `browser-service/src/lib.rs`: The review service (aimview-service, without its `native` feature) as WebAssembly:
  browser mode runs it in a worker, so the browser answers the same API as the review server and the desktop app.
- `tests/camera_parity.rs`: The camera's readings (src/camera.rs) against Python's camera_motion on the same gray
  frames: python/retired/tests/fixtures.py --review writes sample frame pairs (gray.raw, gray.json) and every frame's
  reading (camera.json), and the countdown-teal counts (teal.json, checked in the browser).
- `tests/camera_same.rs`: The camera watch's tile shifts (src/camera.rs) against the ones it gave before, to the bit, on
  the parity cases' frame pairs: a change made for speed must not change a reading.
- `tests/code_map.rs`: The code map (docs/CODEMAP.md), built from the Rust sources so a reader finds where a thing lives
  with one read or one search instead of opening files: each file's header comment, then its public types with the first
  sentence of their doc, their methods, and the file's functions and constants by name.
- `tests/common/mod.rs`: What the parity tests share: the frozen fixtures in test_out/parity
  (python/retired/tests/fixtures.py made them), reading their JSON, excluded areas, gray frames and tracking inputs, and
  comparing the core's JSON with Python's: everything that is not a number equal, numbers within a relative tolerance.
- `tests/compare.rs`: The parity tests' comparison (tests/common): it compares only the fields Python's output has, so a
  field the core adds passes, while a field Python has that the core lacks, or a different value, is a difference.
- `tests/convert_parity.rs`: `convert::Converter` against ffmpeg 8.1 itself: frames of real recordings (AV1, H.264 and
  HEVC; 2560, 1920 and 1280 wide; full and limited range) decoded as they are (`<key>_<n>_src.yuv`), and ffmpeg's
  `scale=1280:720:flags=area` to rgb24 and yuv420p, with its x86 kernels (`.raw`) and its plain C code (`_c.raw`,
  `-cpuflags 0`).
- `tests/faint_parity.rs`: The faint-target cut-off (src/faint.rs, review.rs) against Python's:
  python/retired/tests/fixtures.py --faint writes test_out/parity/<case>/faint/<offset>/report.json (a tracking review
  with the cut-off on) and test_out/parity/faint/<recording>.json (the scores, the cut and the labels of every recording
  the user set a cut-off for).
- `tests/fixed_parity.rs`: `fixed::FixedMap` against Python's `fixed_map` on a real recording's key frames (keys.yuv:
  YUV 4:2:0 at 1280 x 720, as ffmpeg gave them to Python), to the bit (fixed.npy).
- `tests/keep_parity.rs`: `track::keep` against Python's `keep` (python/retired/review.py, `track_model`) on real
  recordings: the detector's raw boxes per frame (raw.json) must give the same targets (dets.json), to the bit, with the
  recording's excluded areas and target count (meta.json), and the frames where a pop-up area is off kept again
  (`reopen`, with the pop-ups Python found, meta.json's `showing`).
- `tests/link_parity.rs`: `track::link` against Python's `link` on real recordings: the fixtures
  python/retired/tests/fixtures.py writes to test_out/parity/<name>/ (dets.json: link's input, frames.json: its output).
- `tests/mouse_parity.rs`: The mouse log reader (src/mouse.rs) against python/mouse_read.py on the same logs:
  tests/mouse_fixtures.py writes test_out/parity/mouse/<case>/ (the log, its stats file, and want.json with what Python
  prints and writes).
- `tests/popup_parity.rs`: `popup::AreaWatch` against Python's `AreaWatch` on real recordings with a pop-up area: every
  frame decoded by ffmpeg as python/retired/review.py's `rgb_frames` does (scale=1280:720:flags=area, rgb24), and the
  per-frame decisions compared with Python's (meta.json's `showing`).
- `tests/python_parity.rs`: `python::hypot` against CPython's `math.hypot` on 20,000 random pairs and a few edge cases,
  to the bit (test_out/parity/hypot.json: [x, y, math.hypot(x, y)] rows, made by python/retired/tests/fixtures.py's
  hypot cases), and the KovOBS overlay's boxes against Python's (test_out/parity/overlay.json).
- `tests/reload_runs.rs`: The forced reloads (src/reload.rs) on real runs of scenarios whose magazine runs out, reviewed
  with their stats files into test_out/reload_runs/<run>/ (`aimview-tool review <video> --out <that folder> --stats-file
  <csv>`), with the ammo rules read from the scenario's file.
- `tests/replay.rs`: The review after the detector, replayed from the parts the native review kept, without the video:
  each run's track part (the detector's boxes, the pop-up areas' looks) and watch part (the camera's tile shifts, the
  countdown, the HUD's glyphs) joined as the review joins them (keep, the pop-ups, link, the camera's readings, the
  HUD's reading), then the report worked out as the service does (matching, measures, summary, checks).
- `tests/review_parity.rs`: The clicking review (src/review.rs) against Python's on the same tracks:
  python/retired/tests/fixtures.py --review writes test_out/parity/<case>/review/ (tracks.json, flicks.json,
  measures.json, report.json).
- `tests/scenario_parity.rs`: `scenario::facts` against Python's `scenario_facts` and `target_counts` over every
  scenario file on this computer (test_out/parity/scenarios.json: the files in Python's order, and Python's facts by
  lower-case name).
- `tests/tracking_parity.rs`: The tracking review (src/tracking.rs, src/review.rs) against Python's on the same tracks
  and camera readings: python/retired/tests/fixtures.py --review writes test_out/parity/<case>/review/ (tracks.json,
  camera.json, teal.json, report.json).
- `tests/what_if_runs.rs`: The what-if lines (src/what_if.rs) on real runs with their stats files: the parity runs
  (test_out/parity/<case>/ review/) and some of the video-alone benchmark's
  (test_out/vod_model/eval/video_alone/full_v3/<run>/).
- `examples/areas.rs`: Checks the area finder (src/areas.rs) against python/areas.py's analyse() on recordings: `cargo
  run --profile quick --example areas -- <reference folder> <name> [more names]`.
- `examples/hud.rs`: Reads recordings' HUDs with the core (src/hud.rs) and prints one line of JSON for each: `cargo run
  --profile quick --example hud -- <video> [more videos]`.
- `examples/review.rs`: Reviews one request (src/review.rs `ReviewRequest`, as JSON in a file) and prints the outcome,
  for checks outside the app: `cargo run --profile quick --example review -- <request.json>`.
- `examples/review_runs.rs`: Reviews every kept run again and writes its reports: the check that a change after the
  tracking (matching, measures, the report) moved nothing it should not.
- `service/examples/api.rs`: The API without a window or a server: requests answered by `api::handle` on a library in a
  data folder, each answer printed as a line of JSON ({"status": ..., "body": ...}), to check the answers against
  python/retired/server.py's on copies of its data.
- `service/examples/detector_speed.rs`: The detector alone, as the native review runs it (service/src/detector.rs): a
  model's _u8in export on frames of noise, `batch` a call, in one session or several at once (a review runs one a part),
  with the time a frame.
- `service/examples/frames_check.rs`: A video's frames from the GPU (gpu_frames.rs) against ffmpeg's, converted by the
  core (video.rs, convert.rs), byte for byte: each frame's RGB, 720p luma and Y plane (all its rows), from the start or
  from a time on.
- `service/examples/mouse_read.rs`: A raw mouse log read (python/mouse_read.py's command line, on the core's reader:
  src/mouse.rs).
- `service/examples/track.rs`: A recording reviewed natively, without the app: its tracks, readings and HUD reading
  written as JSON, the time it took, and the report the core works out from them (report.json), with the stats file when
  one is given, else from the HUD's reading or the video alone.
- `desktop/examples/mouse_log.rs`: The raw mouse logger on its own (python/mouse_log.py's command line): logs the mouse
  to a file while KovaaK's runs.
- `benches/hot_paths/frames.rs`: The stages a frame goes through: the key frames' fixed map and area finder, the
  conversion to 720p, the camera watch, the HUD watch and the pop-up areas' watch.
- `benches/hot_paths/inputs.rs`: Where the benches' inputs are: the parity fixtures (test_out/parity/, from
  python/retired/tests/fixtures.py) and the native review's kept outputs (test_out/baselines/4b7ddc4/native/).
- `benches/hot_paths/main.rs`: The review's hot paths (docs/HOT_PATHS.md) timed with criterion, on real recordings'
  inputs kept in test_out/ (ignored by git; docs/BENCH.md, "Function benchmarks").
- `benches/hot_paths/review.rs`: The review's last steps, from the joined tracks: the kills matched in the tracks, each
  flick measured, and the report worked out (the service's report request).
- `benches/hot_paths/tracks.rs`: The track step after the detector: each frame's boxes kept or dropped (`keep`), then
  the frames linked into tracks (`link`, with each frame's view shift).

## src/areas.rs

The area finder: a recording's overlay areas (HUD boxes, clocks, a webcam) for the run page's Excluded areas editor, and
what each one is (python/areas.py, all of it).

- `Area` (struct): A found area, as areas.json holds it: its box (shares of the frame, x0, y0, x1, y1), its features
  (`features`) and the kind the rules give it.
- `SavedBox` (struct): An area the user saved (exclude.json): [x0, y0, x1, y1] as shares, and its kind (a type id, or a
  name in older files), None when the entry has only four numbers.
- `Example` (struct): One example for the learner (a line of area_examples.jsonl): the recording, an area's features,
  and its kind.
- `Named` (struct): A found area with the kind given to it, and by what ("learned" or "rule").
- `Found` (struct): What the area finder found in a recording: the areas and the maps they came from.
- `Maps` (struct): The stand-out map (the share of the frames each pixel stood out in, times 255, rounded) and the
  change map (each pixel's mean brightness change from one frame to the next, cut to 0..255), 1280 x 720 each, as
  python/areas.py keeps them (areas_maps.npz). Methods: `new`, `stand`, `change`, `features`.
- `AreaFinder` (struct): Finds a recording's areas from its frames (python/areas.py: analyse). Methods: `new`, `add`,
  `add_contrast`, `frames`, `zoomed`, `finish`.
- `AimBoxes` (type): Aim Lab's POINTS, TIME and ACCURACY boxes and their kinds.
- `Labelled` (struct): A recording the user saved areas for: its found areas and its saved areas.
- `Proposal` (struct): The areas to propose for a recording.
- `Check` (struct): Leave one recording out: each example's kind predicted from the other recordings' examples
  (python/areas.py: check).
- `Kind` (struct): An area type (area_kinds.json): its id and name.
- `Examples` (enum): Examples as the text of area_examples.jsonl, or as a list. Methods: `list`, `lines`.
- Functions: `sample_frames`, `session_share`, `review_rows`, `zoomed`, `inside`, `iou`, `rule_kind`, `learn`, `merge`,
  `example_line`, `predict`, `same_layout`, `find`, `check`, `kind_id`, `with_ids`, `sample_json`, `find_json`,
  `learn_json`, `predict_json`, `same_layout_json`, `check_json`.
- Constants: `N_SAMPLES`, `MIN_KEYS`, `FIXED`, `GAP`, `NONE`, `NEAREST_EXAMPLES`, `FRAME`, `FEATURES`.

## src/camera.rs

How the camera turned, from the video alone (python/retired/review.py: `camera_motion`), and whether KovaaK's countdown
bar shows (`countdown_showing`). The review session (src/session.rs) feeds the watch each frame's luma and the countdown
bar's rows of its RGB: in the camera worker in the browser, on a thread of its own natively. A recording split into run
parts has a watch for each, joined in order (`CameraPart`). Once the tracks are known, `finish` gives the readings
(`VideoReadings`): the camera's turn, which src/tracking.rs measures a tracking run with, and the countdown, which
src/review.rs places the run's start with when no kill does.

- `TileShifts` (type): Each tile's shift since the frame before (degrees, the room's move on screen), or None where its
  peak is too low.
- `CameraWatch` (struct): The camera watch over a recording: fed each frame's luma (1280 x 720), it keeps each frame's
  tile shifts and its countdown bar's showing; the readings come once the tracks are known (a tile with a target in it
  is left out). Methods: `new`, `for_recording`, `finish`, `skip`, `part`, `join`, `add`, `reading`, `readings`.
- `VideoReadings` (struct): What a tracking run reads from the video besides the tracks (CameraWatch::finish).
- `CameraPart` (struct): A run's part of the camera watch (CameraWatch::part): each frame's tile shifts and whether the
  countdown bar shows.
- Functions: `excluded`, `countdown_showing`.
- Constants: `COUNTDOWN_ROWS`.

## src/capped.rs

A list of at most N items, kept in an array with its length: a list with a small fixed limit needs no heap. The report's
small per-kill and per-run lists (matching.rs, measure.rs, summary.rs, tracking.rs, what_if.rs, the camera and HUD
watches) use it; it serializes as a JSON array.

- `Capped` (struct): At most N items, in the order pushed; the slots past the length hold `T::default()`. Methods:
  `new`, `push`, `remove`.

## src/convert.rs

A decoded frame (YUV 4:2:0, any size) converted as ffmpeg 8.1 converts it for the review: `scale=1280:720:flags=area` to
`rgb24` (the detector's input) or `yuv420p` (the fixed map's), byte for byte as the ffmpeg CLI does on x86-64 (the old
review, python/retired/review.py: `rgb_frames`, `_frames`). The model was trained on these exact bytes: GPU color
conversion once changed detections at the crosshair. Every step mirrors libswscale's integer code (utils.c `initFilter`,
hscale.c, vscale.c, output.c, yuv2rgb.c, and the x86 kernels where ffmpeg uses them), and the names follow swscale's
where it has them.

- `Matrix` (enum): The color matrices ffmpeg knows (AVColorSpace). Methods: `from_code`, `code`.
- `Converter` (struct): Converts a recording's frames: built once per recording (its size and color tags), then one call
  per frame. Methods: `new`, `with_kernels`, `without_shortcut`, `rgb24`, `yuv420p`, `luma`.
- Constants: `DST_W`, `DST_H`.

## src/dates.rs

Civil dates (year, month, day of the Gregorian calendar) as days since 1970-01-01 and back, by Howard Hinnant's
algorithms, which count years from March so that a leap day ends the year.

- Functions: `days_from_civil`, `civil_from_days`, `days_in_month`.

## src/detect.rs

The detector model's output maps as boxes (python/model/infer.py: `decode_np`, for the `_u8in` and `_fp32` exports,
whose score map already holds only the peaks).

- Functions: `decode`.
- Constants: `STRIDE`.

## src/faint.rs

The faint-target cut-off (python/retired/review.py: `faint_scores`, `without_faint`; python/model/hand_crops.py:
`cutoff_crops`): each track's score, the recording's level, the tracks the user's cut-off leaves out, and the detector
labels a submitted cut-off gives.

- `FaintSetting` (struct): The user's faint-target cut-off for a recording (faint.json): whether it is on, and how far
  below the recording's level a track may score before the cut leaves it out.
- `TrackScore` (struct): A track's score: the 90th percentile of the detector's scores for it away from the crosshair,
  and how many frames gave one.
- `FaintScores` (struct): Every track's score, in the order the tracks first scored, and the recording's level: the 90th
  percentile of the scores, weighted by frames (None without scores).
- `FaintCutFrames` (struct): What a cut-off left: the frames without the tracks it cuts, the score it cut at (rounded to
  3 decimals; None without scores, when nothing is cut) and how many tracks went.
- `CutoffRequest` (struct): What the labels of a submitted cut-off are made from: the recording's tracks and name, the
  run's first and last frames (a clicking run: its first flick's start and its last kill; a tracking run: the run's
  own), the excluded areas (shares of the frame; None: KovOBS's layout), the cut-off's offset, and how near the
  crosshair a score is not counted (a tracking run 0, a clicking run 2 degrees).
- `CutoffRow` (struct): A label's row, as label_check.py writes its checks (checked.jsonl): the crop's file, the boxes
  it keeps (crop pixels: center, size, 2 decimals), every box the model gave there, and where the label came from.
- `CutoffCrop` (struct): A crop to write: the frame, the crop's corner (pixels at 1280 x 720), the boxes it keeps as the
  file holds them (float32 there), and its row.
- Functions: `faint_scores`, `without_faint`, `crop_stem`, `cutoff_crops`, `cutoff_json`.
- Constants: `DEFAULT_OFFSET`, `CROP`.

## src/fixed.rs

The fixed map: the pixels that stay put on screen while the view moves (crosshair, HUD text, a gun model), found in the
recording's key frames. It is the detector model's 4th input (python/retired/review.py: `contrast`, `_blur_up`,
`fixed_map`). The arithmetic is NumPy's and SciPy's, float32 where they keep float32, so the map is the same bit for
bit.

- `FixedMap` (struct): Counts, per pixel, the key frames it stands out in. Methods: `add`, `add_contrast`, `map`.
- Functions: `contrast`.
- Constants: `DIFF`, `SHARE`.

## src/geometry.rs

The frame's geometry at the size the review works in (1280 x 720): where the crosshair is, and how a pixel maps to an
angle from it (python/retired/review.py: `W`, `H`, `CX`, `CY`, `K`, `to_deg`, `to_px`).

- Functions: `overlay_shares`, `degrees`, `blob_radius_deg`, `radians`, `to_deg`, `to_px`.
- Constants: `W`, `H`, `CX`, `CY`, `K`, `OVERLAY`.

## src/hud/glyphs.rs

What each frame keeps of the HUD (src/hud/mod.rs): its value rows' glyphs, each row's kept once while it stays the same
from frame to frame, so a long recording stays small (`GlyphStore`).

## src/hud/layout.rs

Where the HUD's boxes and their text rows are (src/hud/mod.rs): the grey levels and ink of a scaled region, KovaaK's
session box found in the key frames' median (a flood fill from a patch of the box's level) and its value rows, and each
row's glyphs cut apart; Aim Lab's POINTS and TIME boxes' glyphs too.

- `SessionRows` (struct): KovaaK's session box as python/hud.py's layout finds it, for the area finder (src/areas.rs):
  the columns of its text rows and the top of the first row and the bottom of the last, in the scaled region (BW x BH:
  the pixels of a 2560 x 1440 frame, from its top left corner).

## src/hud/mod.rs

A recording's on-screen HUD, read frame by frame for a run without a stats file (python/hud.py's algorithm): KovaaK's
session box (Kill Count, and Accuracy as hits/shots) or, when there is none, Aim Lab's POINTS and TIME boxes. It gives
the kill frames, the shots and hits, and the run's totals. The digits are learned from the recording itself (no font):
KovaaK's Kill Count counts up one kill at a time, Aim Lab's timer down one second at a time.

- `HudGame` (enum): Which game's HUD was read.
- `HudFinal` (struct): The run's totals as the HUD shows them at the end: the kills counted, and the hits and shots
  (None where the Accuracy line was not read).
- `HudReading` (struct): What the HUD read (python/hud.py: read() and read_aimlab()).

## src/hud/reading.rs

Reading the HUD's counts (src/hud/mod.rs): the digits learned from the recording itself (the glyphs' shapes, and
KovaaK's Kill Count counting up one kill at a time, Aim Lab's timer down one second at a time), then the Kill Count's
kill frames, the Accuracy line's shots and hits, or Aim Lab's POINTS steps.

## src/hud/scale.rs

The HUD's scaling (src/hud/mod.rs): a frame's region resampled as ffmpeg's `scale` filter does (area, bilinear and
bicubic taps along each axis), the glyphs cut from it made GLYPH_WIDTH_PX x GLYPH_HEIGHT_PX, and a limited-range Y
stretched to 0..255.

## src/hud/watch.rs

The HUD watch (src/hud/mod.rs): the key frames' layout, then each frame's glyphs, a run part at a time, its part saved
as JSON and the parts joined, then the reading.

- `HudPart` (struct): A run part's share of the watch (a review split into run parts: each part's watch reads its own
  frames, the page joins them).
- `HudKeys` (struct): What a watch reads in the key frames (`HudWatch::keys`): KovaaK's box, None without one, and its
  text rows. Methods: `session`.
- `HudWatch` (struct): Reads a recording's HUD: first every key frame (`add_key`, for where KovaaK's box and its rows
  are), then every frame in order (`add`), then `finish`. Methods: `new`, `rows_read`, `add_key`, `session_box`, `keys`,
  `from_keys`, `skip`, `add`, `frames`, `part`, `join`, `finish`.

## src/kill_check.rs

The kills the video alone gives (matching.rs `match_video`), checked in the frames round them: a target that dies leaves
wall where it was, while a target the tracking only lost, or a crosshair the detector boxed, still shows there. Before a
kill (frames kill-4 to kill-2) its target's patch is measured at its tracked place against the wall round it; after it
(kill+3 to kill+6) the same at the place it died, carried along by the camera's turn (a frame's shift moves a still spot
on screen by as much). A kill whose target still shows after, by half as much as before or more, is no kill
(`ruled_out`): on the gate's static runs that left out 34% of the video's false kills and 0.65% of its true ones, on its
dynamic runs 37% and 0.31% (measured against the stats files, 2026-10-06). The place it died is measured in each of the
TRAIL frames after it (`KillEvidence::trail`): a target hidden under the crosshair before the click stays a while at
part of its level, and goes when it dies. Such a kill is moved to when it died (`with_hidden_kills`): the frame the
place reached the wall, less the death's fade, the target held at the crosshair until then. On the video-alone runs' dev
set that took 1wall 6targets extra small from 45 to 57 of its 98 kills, the held-out runs unchanged (2026-10-06).

- `KillEvidence` (struct): A kill's evidence: how much its target stood out from the wall before it and after it (the
  median over the frames measured; None where none could be: the place was off screen, or the frames were missing), and
  the place it died in each frame after it, 1 to TRAIL (None where it could not be measured).
- `KillCheck` (struct): The kills of a review being checked: the measurements each frame needs, and those made. Methods:
  `new`, `frames`, `add`, `evidence`.
- Functions: `ruled_out`, `with_hidden_kills`.

## src/lib.rs

Aim View's review core: it finds the targets in a recording's frames, follows them, and measures the aim. It is built
natively for the service (service/: the desktop app and the review server) and as WebAssembly for the browser
(src/wasm.rs). It began as a port of the old Python review (python/retired/review.py, retired on 2026-10-04); the parity
tests (tests/) still compare it with that review's stored outputs (test_out/parity/), and KovaaK's stats files are the
ground truth for new work.

## src/local_config.rs

Aim View's settings for a checkout of the repo and this computer: aimview.defaults.json (in git, built in here: the
project's own layout and where KovaaK keeps its folders under Steam's) under aimview.json (beside it at the repo's root,
out of git, optional: this computer's own, such as the recordings' folder). No folder is written in the code: the review
server's defaults, the examples and the tests read them here (python/local_config.py reads the same files for the
scripts). Steam's folder, unless named, is where Steam records it: the registry on Windows, else under the home folder.
In: the two files. Out: folders, and the server's address.

- `LocalConfig` (struct): The settings: the defaults with this computer's file over them, and the folder their relative
  paths start at. Methods: `load`, `at`, `folder`, `server`, `steam`, `kovaak`, `kovaak_scenarios`.

## src/matching.rs

Each kill matched to the target it killed, and the flick to it (python/retired/review.py: `match_times`,
`_attach_kills`, `appearances`, `crosshair_spots`, `ghosts`).

- `PathPoint` (type): A point of a target's path: frame, x and y (degrees from the crosshair).
- `Flick` (struct): A kill and the flick to it: its number, the frame its target was last seen on and the frame the kill
  times give, where the flick starts, the shots it took, the target's path, whether the target appeared after the kill
  before it, and its median blob area in pixels.
- `KillSource` (enum): Where the kill times came from.
- `MatchInfo` (struct): How the kills matched: kills seen in the video and in the kill times, kills matched, kills
  confirmed (the target last seen at the crosshair within 2 frames of its kill time), the kill times' offset on the
  video's clock (s).
- `Appearances` (struct): Tracks that are one target picked up again: appeared (each track with the frame its target
  first appeared on, in the order the tracks start) and follows (a track to the track that continues it).
- Functions: `appearances`, `crosshair_center`, `crosshair_spots`, `without_crosshair_boxes`, `without_crosshair_ends`,
  `match_times`, `ghosts`, `without_ghosts`, `match_video`.
- Constants: `JOIN_GAP_S`, `JOIN_RADIUS_DEG`, `SPOTS`, `ON_SPOT_DEG`.

## src/measure.rs

Each flick of a clicking run measured (the old review, python/retired/review.py: `target_radius`, `measure`, `choices`):
how long each kill step took, how fast the crosshair moved, where it ended and where it clicked.

- `Measure` (struct): One flick's measures (seconds, degrees and degrees a second): the keys the old review's `measure`
  wrote, plus settle, still and the time parts.
- `SpeedCurve` (struct): The camera's speed through a main flick, in degrees a second, one value a frame from the
  flick's start (the moves into the frame before, that frame and the next, averaged), and on past its end for a quarter
  of its length (at least 2 frames) to show the braking.
- `Choice` (struct): For a kill after the first: whether the next target was the nearest on screen (rank 0), and how
  much farther.
- `FlickProfile` (struct): The flick speed profile: each main flick's camera speed, as a share of its own peak, against
  the time as a share of the flick (the points are `step` apart from 0), averaged over `flicks` flicks, with the 25th
  and 75th percentiles.
- Functions: `target_radius`, `measure`, `flick_profile`, `choices`.
- Constants: `PROFILE_STEP`.

## src/model.rs

The detector model's settings file (python/model/MODEL_FILE.md): `detector_<name>.json` beside the model's exports, so a
retrained model needs no change to the code. Format 1: the score a cell must pass to be a target, the map that puts the
model's scores on the reference model's scale, and (optional) the weaker score a cell at the crosshair may pass instead
(`AtCrosshair`). A model with no file gets today's values (`ModelSettings::default`).

- `ModelSettings` (struct): A model's settings. Methods: `from_json`, `mapped`, `passes`, `lowest_threshold`,
  `for_kind`, `floor`.
- `AtCrosshair` (struct): Weaker cells kept at the crosshair: a cell whose score passes `threshold` (on the reference
  model's scale, under the model's own) is a target too when its box's center is within `reach_px` of the crosshair
  (1280 x 720 pixels), in a run of one of `kinds` (None: every kind; a run of no known kind gets the rule only then).
- Functions: `settings_file`.
- Constants: `FORMAT`, `DEFAULT_THRESHOLD`, `REFERENCE`.

## src/mouse.rs

The raw mouse log and what is measured from it (a port of python/mouse_read.py): each flick of a KovaaK's run, matched
with the run's stats file.

- `MouseLog` (struct): A log read into columns, one entry per event (mouse_log.py: `read_log`). Methods:
  `device_events`.
- `Options` (struct): The reader's settings: mouse_read.py's options, with its defaults.
- `Sensitivity` (struct): A sensitivity in cm/360: the mouse's dots per inch, and the centimeters it moves for a full
  turn.
- `KillMeasure` (struct): One kill's measures, from the previous kill's press to its own (mouse_read.py: the rows of
  `<log>.kills.json`).
- `DeviceFacts` (struct): A device in the log: its handle and its events.
- `LogFacts` (struct): What any log says (mouse_read.py: `head` and `rate_line`): its span, events, clock drift, devices
  and rate.
- `LogSummary` (struct): A log without its run (mouse_read.py's summary mode): its facts, the total travel and the
  left-button presses.
- `StatsKill` (struct): A kill in the stats file: its number, its local time as written, that time in seconds since
  1970, and its shots.
- `StatsRun` (struct): What the reader takes from a stats file (mouse_read.py: `read_stats`): the kills, the run's start
  and end (seconds since 1970), the shots, the sensitivity when it is in cm/360, and the scenario.
- `Spread` (struct): One measure over the kills: how many have it, and its p10, median and p90.
- `MouseRun` (struct): A run measured from its mouse log (mouse_read.py's run mode): everything it prints, and the kills
  it writes.
- `ReadRequest` (struct): What the page asks of a log: its run's stats file (name and text; none for the log on its
  own), the options, and the UTC offset (local minus UTC, seconds) for local times.
- `ReadOutcome` (enum): The answer: the run's measures, the log's summary (no stats file), or why there are none.
- Functions: `header`, `event`, `device`, `stop`, `read_header`, `read_log`, `busiest_rate`, `local_clock`,
  `deg_per_count`, `presses_of`, `log_facts`, `summary`, `read_stats`, `run`, `read`, `read_json`, `log_span`, `fmt_g`,
  `summary_text`, `run_text`, `kills_json`.
- Constants: `MAGIC`, `VERSION`, `HEADER_SIZE`, `RECORD_SIZE`, `KIND_DEVICE`, `KIND_STOP`, `MOUSE_MOVE_ABSOLUTE`,
  `LEFT_BUTTON_DOWN`, `LEFT_BUTTON_UP`.

## src/optional_fields.rs

A part of a struct whose fields are written into it all or none (`#[serde(flatten)]` over an optional part).

- `OptionalFields` (struct): An optional part of a struct, its fields written into the struct (`#[serde(flatten)]`): all
  of them when the part is there, none when it is not.

## src/popup.rs

Excluded areas that only sometimes show (a "Last kill" pop-up) are excluded only while they show
(python/retired/review.py: `AreaWatch`). Every other frame, each area's stand-out pattern is kept, small: the pixels
that differ from their neighbors, as text and boxes do and a plain wall does not. After the run, an area is a pop-up
when it is off for 30% of the run or more, comes and goes 3 times or more (the results screen covering it once at the
end is not), and looks the same whenever it is on; it is then excluded in its on frames and 4 frames either side. Any
other area (the session box, a webcam) is excluded all the time. An area the user named the challenge's end screen
(`END_SCREEN`) shows once or twice, at the end or between runs, and covers most of the frame: it is excluded only while
it shows, however few its episodes (excluded all the time, it hid the whole run: VT FlyTS, 0 of 5 kills).

- `AreaWatch` (struct): Watches a recording's excluded areas frame by frame. Methods: `new`, `end_screens`, `start_at`,
  `from`, `join`, `add`, `showing`.
- Constants: `STEP`, `END_SCREEN`.

## src/py_random.rs

Python's `random.Random` seeded with a string (CPython's Mersenne Twister, seeded from the string and its SHA-512), and
MD5 as hashlib gives it.

- `PyRandom` (struct): `random.Random(seed)` for a string seed: the same numbers as Python gives, call for call.
  Methods: `seeded`, `below`, `randint`, `choice`.
- Functions: `sha512`, `md5`, `hex`.

## src/python.rs

Arithmetic done the way Python and NumPy do it, where the last bit of a result can reach a report: rounding to a number
of decimals, CPython's `math.hypot`, and NumPy's sums, means, medians and percentiles.

- Functions: `round`, `hypot`, `numpy_sum`, `numpy_mean`, `numpy_percentile`, `numpy_median`.

## src/reload.rs

The reloads a clicking run's magazine forced (src/scenario.rs: `AmmoRules`), worked out from each kill's shots.

- `KillReloads` (struct): One kill's forced reloads: how many, and their time in seconds.
- `Reloads` (struct): Forced reloads over a run: how many, their time in seconds, and the points they took off (None
  when the scenario takes none for a reload).
- `ReloadCost` (struct): What reloading cost a run: each kill's forced reloads, the run's, and the run's with every miss
  taken out (each kill's shots cut to its hits; None when the hits are not known).
- Functions: `forced_reloads`, `reload_cost`.

## src/review.rs

The review of a run (python/retired/review.py: `review`): a clicking run's flicks, or a tracking run's time on the
target. The kills come from the run's stats file; without one, from the HUD read in the video (src/hud/); without a
readable HUD, from the video alone. In: a request (`review_json`: the run's tracks, its stats file, what the video read
and the user's run marks), which the service builds in every mode (service/src/report.rs). Out: the report as JSON
(report.json), which the run page shows.

- `Geometry` (struct): The frame's size and the crosshair's place (pixels), and the focal length (pixels) the degrees
  come from.
- `Report` (struct): A clicking run's report, as report.json keeps it.
- `Reviewed` (struct): A review's results: the flicks matched to the kills (flicks.json) and the report.
- `KillTimes` (enum): Where a run's kills come from: its stats file (its name and text), or, for a run without one, the
  HUD read in the video (None where it did not read) and, for the kills the video alone gives, their check in the frames
  round them (kill_check.rs; None: not checked).
- `TrackReport` (struct): A tracking run's report, as report.json keeps it: the summary, with the clicking run's parts
  empty.
- `VideoReadings` (struct): What a tracking run reads from its video besides the tracks: per frame the camera's reading,
  and whether KovaaK's countdown bar shows.
- `TrackScenario` (struct): What a tracking review takes from the scenario: its time limit (seconds), which the stats
  file's own length overrides, and its bots' hitbox (None: the crosshair is on a target within a margin of its box).
- `ReviewRequest` (struct): What the service asks the core to review (service/src/report.rs): the tracks, the video's
  name, the stats file's name and text (empty without one), what the HUD read, the user's run marks; for a clicking run
  the ammo rules of the scenario's weapon (null or missing: its magazine never runs out, or the scenario is not known);
  for a tracking run also the scenario's time limit, the video's readings and the user's faint-target cut-off ({on,
  offset}; null or missing: none).
- `AnyReport` (enum): A clicking run's report or a tracking run's.
- `Outcome` (enum): The report, or why there is none: {"report": ...} or {"error": "..."}.
- Functions: `review_clicks`, `review_tracking`, `run_window`, `review_json`.

## src/scenario.rs

What a scenario file (.sce) says about a run (python/retired/review.py: `scenario_facts`, `target_counts`): its kind,
its time limit, how many targets are alive at once, the player's weapon's ammo rules, and the bots' hitbox (its shape
and its width over its height). Read from the file's part before "[Map Data]".

- `Kind` (enum): The kinds of run the review tells apart.
- `Facts` (struct): A scenario's facts: its kind, its time limit in seconds, its targets alive at once (one per bot
  added), the player's weapon's ammo rules (none when its magazine never runs out) and the bots' hitbox (none when the
  bots differ or look like something else).
- `HitboxKind` (enum): A bot's hitbox shape (KovaaK's MainBBType): an ellipsoid (a sphere when its height is its width),
  an upright capsule (a cylinder with round ends), or a box.
- `Hitbox` (struct): The bots' hitbox: its shape and its width over its height (2 x MainBBRadius over MainBBHeight).
- `AmmoRules` (struct): The ammo rules of a weapon whose magazine can run out (KovaaK's weapon profile, and the
  scenario's points for a reload): the magazine's size (MagazineMax), the ammo a shot uses (AmmoPerShot), the ammo a
  kill puts back, up to a full magazine (AmmoReloadedOnKill), the reload's time in seconds from an empty magazine and
  from a part-used one (ReloadTimeFromEmpty, ReloadTimeFromPartial), and the points a reload takes off
  (ScoreLossPerReload).
- Functions: `header`, `text_of`, `facts`.

## src/scipy.rs

What the review uses from SciPy's `ndimage`, done the way SciPy does it, so results match bit for bit.

- `Edge` (enum): How a filter sees past the ends of a line (SciPy's `mode`).
- Functions: `uniform_line`, `uniform_filter`, `dilate_line`, `erode_line`, `close_line`, `count_runs`, `runs`.

## src/session.rs

A review session: everything a review does between the decoder and the detector, the same for the browser and the
desktop. The hosts (the browser's review and camera workers in ui/src/app/modes/wasm/, the native review in
service/src/review.rs) decode the frames, run the detector and feed this. A recording is split into run parts at key
frames (`Run`; "runs" in this file, not runs of a scenario), reviewed at once (one decoder is the limit, in the browser
and natively alike), then joined:

- `TimeWindow` (struct): A part of a video, in seconds.
- `FrameRange` (struct): The frames to review: from `first` up to `end` (not included), as indexes in the recording.
- `Run` (struct): A run of the recording's frames: from a key frame's time (`from`; the recording's first run at 0) to
  the next run's (`to`, None for the last), its first frame's index in the recording and how many frames it has.
  Methods: `reads`.
- `AreaBox` (type): An area the review leaves out: [x0, y0, x1, y1] as shares of the frame, and its kind's id.
- `FrameFormat` (struct): The recording's frames as the decoder gives them: their size, their color matrix
  (`Matrix::from_code`) and whether they are full range.
- `Setup` (struct): What a review is set up from: the video's frame rate, every frame's time and the key frames' (from 0
  on, in order) and the frames' format; the scenario's target count (0: not known), the areas the review leaves out, the
  part of the video to track (None: all of it), the runs to split it into, and the detector model's settings (its
  settings file; today's values without one).
- `Review` (struct): A review: its setup and the runs it is split into. Methods: `new`, `setup`, `set_model`, `runs`,
  `frames`, `keys`, `tracking`, `hud_rows`, `watching`, `joining`.
- `Keys` (struct): The key frames' pass (`Review::keys`). Methods: `add`, `add_contrast`, `finish`.
- `KeysRead` (struct): What the key frames give every run: the fixed map (1280 x 720, 1 fixed) and where the HUD's boxes
  are.
- `NextFrame` (enum): What a decoded frame is for (`RunTracking::next_frame`).
- `RunTracking` (struct): A run's tracking (`Review::tracking`): which frames it reads, each tracked frame's excluded
  areas watched for pop-ups, and the detector's maps in order. Methods: `next_frame`, `watch`, `maps`, `part`.
- `RunWatching` (struct): A run's watches (`Review::watching`): the camera watch and the HUD watch, fed each frame the
  run reads. Methods: `frame`, `frame_with_luma`, `y_bytes`, `part`.
- `WatchPart` (struct): A run's part of the watches (`RunWatching::part`).
- `Tracks` (struct): The tracks as tracks.json keeps them: the frame rate, each frame's targets, the share of the frame
  the fixed map covers, and the detector that found them.
- `Joined` (struct): A review's runs joined: the tracks, the video's readings (each frame's camera turn and countdown
  bar), and what the HUD read (None: no HUD was read).
- `Joining` (struct): The runs' parts joined in order (`Review::joining`). Methods: `add`, `finish`.
- Functions: `window_frames`, `split_runs`, `countdown_bytes`.
- Constants: `LEAST_RUN`.

## src/shapes.rs

The shapes a target is drawn with on a crop: KovaaK's two, as their outline on screen (the targets are 3D). A pill (a
sphere is a pill with equal sides) and a box (a square or a cube), each turned to any angle. Either can have a third
face, the offset of its far end, for a target seen at an angle: a cube's outline is then a hexagon, a deep pill's the
pill swept back to its far end. Or either can be solid: a box or a capsule with a thickness, tipped and swung out of the
screen's plane, its outline what that solid shows the camera. A box's vertices can also be placed one by one (`points`:
a flat box's 4 corners, a 3D box's 8), for a target seen in perspective: its outline is then what they span. Shapes are
joined into targets (a bot's head and body), ordered front to back by depth (a shape hides the parts of shapes behind
it), and some only hide what is behind them (occluders: the crosshair, a pillar, an overlay).

- `ShapeKind` (enum): KovaaK's target shapes.
- `ShapeRole` (enum): The part of a bot a shape stands for.
- `Shape` (struct): One shape: its kind, its frame before turning ([center x, center y, width, height], pixels), its
  angle (degrees, clockwise), a third face (the offset of the far end, pixels), its solid (a 3D shape's thickness and
  tumble), a box's vertices placed by hand (crop pixels; they decide its outline when given), its depth (greater is
  nearer), its role, and the model box it started from (an index into the crop's boxes), if any.
- `Solid` (struct): A 3D shape's thickness and its turn out of the screen's plane, for a target seen at an angle.
- `Scene` (struct): A crop's shapes: which are joined into one target (a shape in no group, and no occluder, is a target
  of its own), and which only hide what is behind them.
- `TargetView` (struct): One target as the scene shows it: its shapes, the box round its visible pixels ([center x,
  center y, width, height]; None when none shows), the box round all its shapes (hidden parts too), whether something
  hides all of it, and its visible pixels as run lengths (`runs`).
- `SceneView` (struct): A scene seen on a crop: its targets, and the pixels of all of them (the training `tmask`) as run
  lengths.
- Functions: `outline`, `contains`, `check`, `runs`, `from_runs`, `visible`, `visible_json`.

## src/statistics.rs

The statistics a report gives: means, spreads and medians, in plain floating point (python.rs has NumPy's, where its
order of operations reaches a result).

- Functions: `mean`, `pstdev`, `median`, `med`.

## src/stats_file.rs

KovaaK's stats file (review.py: `load_stats`): its "Key:,value" lines and its kill table.

- `StatsFile` (struct): A stats file: its "Key:,value" lines (a later line wins), and the kill table's rows as cells of
  text. Methods: `parse`, `start_micros`, `shots`, `hits`, `kills`.
- `StatsKills` (struct): The kills in a stats file: each one's time in seconds since the challenge started, and the
  shots it took.

## src/summary.rs

A clicking run's summary and its checks (the old review, python/retired/review.py: `summarize`, `_fitts`, `judge`).

- `Mode` (enum): Click: one shot a kill.
- `Holding` (struct): A hold-fire run's holding: the median time from reaching a target to its kill, the share of kills
  where the crosshair slipped off, and the share of that time spent off the target.
- `Pace` (struct): The median kill time in the first and the last third of the run.
- `DistanceGroup` (struct): Flicks of one distance range (degrees).
- `DirectionGroup` (struct): Flicks of one direction (a 45-degree sector), with the median time each took beyond what
  its distance predicts.
- `Summary` (struct): A clicking run's summary: the stats file's facts, then the medians and shares of the measures.
- `Direction` (enum): The names in DIRECTIONS, for the TypeScript types only (the core keeps them as strings).
- `Flag` (enum): A check's verdict.
- `Issue` (struct): One check: the issue's number, the number it reads, a plain verdict and why.
- Functions: `in_distance_group`, `direction_sector`, `whole_ms`, `summarize`, `judge`.
- Constants: `DIRECTIONS`, `DISTANCES`.

## src/track.rs

Tracking: the detector's boxes kept or dropped per frame (python/retired/review.py: `track_model`'s `keep`), and the
targets of each frame given ids that follow them from frame to frame (`link`).

- `RawBox` (struct): A box from the detector model, in frame pixels, as it gives them (float32): center, size, and score
  (on the reference model's scale: src/model.rs).
- `Mask` (struct): Where targets count in a frame: every pixel but the excluded areas (W x H, row by row). Methods:
  `kept`, `without`.
- `ModelBox` (struct): The detector model's box around a target, in degrees, and how sure it is.
- `Spot` (struct): A target in one frame: its place in degrees from the crosshair (right and up positive), its area in
  pixels, and from the detector model its box and score.
- `TrackPoint` (type): A target in a linked frame: [id, x, y], degrees rounded to 4 decimals.
- `TrackFrame` (struct): One frame of tracks, as tracks.json keeps it: the view's shift since the frame before
  (degrees), each target with its id, its area, and from the model its box (w, h in degrees, 3 decimals) and score (3
  decimals).
- `Tracks` (struct): A recording's tracks, as tracks.json keeps them: the frame rate, each frame's targets, and the
  review's version (0 where it is not given: Python's, and the browser's and the desktop app's before version 2).
- Functions: `keep`, `reopen`, `view_shift_between`, `spikes`, `link`.
- Constants: `REVIEW_VERSION`.

## src/track_checks.rs

A tracking run's checks: each a Work on / Fine verdict, like a clicking run's (src/summary.rs `judge`).

- Functions: `judge`.

## src/tracker.rs

The track step for one recording, or one run of it (a recording split into runs, reviewed at once): each frame's boxes
as the detector gave them, its excluded areas watched for pop-ups, then, when the frames are in, each frame's boxes kept
or dropped (those under a pop-up that is off kept again) and all linked. The review session (src/session.rs) drives it
for the browser and the desktop app.

- `Tracker` (struct): The track step's state: the settings it decodes and keeps boxes with, and what it has of the
  frames so far. Methods: `new`, `end_screens`, `set_model`, `kovobs`, `start_at`, `watch`, `push_maps`, `push_boxes`,
  `part`, `add_part`, `finish`.
- `TrackPart` (struct): A run's part of the track step (`Tracker::part`): its frames' raw boxes and its area watch,
  joined in order with the other runs' (`Tracker::add_part`).

## src/tracking.rs

A tracking run's summary (the old review, python/retired/review.py: `track_summary`, `track_motion`, `what_if`,
`stats_length`, `countdown_end`, `tracking_crosshair`, `without_crosshair`): how the crosshair stayed on the target,
from the tracks and the camera's turn.

- `CameraReading` (type): The camera's reading for a frame: the room's move on screen since the frame before (degrees;
  the camera turned by minus that) and how many tiles agreed on it, or None.
- `MotionDirection` (struct): The tracking per direction of the target's motion: the share of the moving time, the share
  on the target, the median distance from its center line and the median offset along the motion.
- `OffFrames` (struct): What the off-target time went on, in frames (for the what-if estimates).
- `MotionCounts` (struct): The counts behind the swings: swings, corrections, and swings as a share of corrections.
- `TurnBack` (struct): One of the bot's direction changes: its frame, and the seconds from it until the crosshair was on
  the bot again (0 when it stayed on; None when it was not back before the next change or the run's end).
- `Motion` (struct): Tracking diagnostics from the target's own motion and the camera's (see the old review's
  `track_motion`).
- `WhatIf` (struct): One what-if estimate: how much of the run's time on target one change would add (a share of the
  run).
- `RunShares` (struct): The shares of the run that need its frames: on a target over the whole run (switching included),
  and what the lost stretches and the shorter slips cost.
- `FaintCut` (struct): The user's faint-target cut-off, when on: its offset, the score it cuts at, and how many tracks
  it left out.
- `TrackInfo` (struct): Where the tracking run's kill times came from.
- `TrackSummary` (struct): A tracking run's summary (see the old review's `track_summary`).
- `RunFacts` (struct): What a tracking run's summary is measured with besides its tracks: the stats file's facts
  (`meta`), the run's length (seconds) and its first frame when known, the camera's readings, the frames where bots die,
  where the kills come from, and the bots' hitbox (None: the crosshair is on a target within INSIDE_MARGIN_DEG of its
  box).
- Functions: `track_motion`, `stats_length`, `countdown_end`, `box_ratio`, `track_summary`.
- Constants: `GET_BACK`, `NO_SLIPS`, `NO_LEADING`, `NO_TRAILING`, `NOT_THROWN`, `EVERY_DIRECTION`, `BEST_TEN_SECONDS`,
  `FASTER_SWITCH`.

## src/typescript.rs

Names for the tuples the reports write, for the TypeScript types only (ts-rs, feature `ts`): the UI names every tuple
type, and ts-rs writes a field's tuple in place. The fields that hold them say so with `#[ts(as = ...)]`.

- `PathPoint` (struct): A target's place in one frame: frame, x, y (degrees from the crosshair).
- `TrackPoint` (struct): A target in a frame: its track id, x, y (degrees from the crosshair).
- `TargetSize` (struct): A target's size: width, height (degrees).
- `ViewShift` (struct): How far the view moved since the frame before: x, y (degrees).
- `CrosshairSpot` (struct): A fixed screen spot where the detector marks the crosshair: x, y (degrees).
- `TargetOffset` (struct): A target's place from the crosshair at the click: x, y (degrees).
- `KillParts` (struct): A kill's time in its five steps, in seconds: react, main flick, onto the target, settle, still
  on the target.
- `AroundPoint` (struct): A frame's offset from the bot's center line along its motion (positive: ahead) and across it,
  and its radius.
- `SecondShares` (struct): A second of a tracking run: the share of it on the bot, and the share switching between bots.
- `AreaBox` (struct): An area a review leaves out: x0, y0, x1, y1 (shares of the frame), and its kind's id.
- `Switch` (struct): A bot's death: its frame, the frame the crosshair is on a bot again, and the first frame a bot
  shows.
- `CropBox` (struct): A box on a crop: center x, center y, width, height (crop pixels; a shape's before it is turned).
- `CropVertex` (struct): A box's vertex placed by hand: x, y (crop pixels).
- `FaceOffset` (struct): Where a box's far face sits from its near one: x, y (crop pixels).

## src/wasm.rs

The core's interface to the browser (WebAssembly builds only): plain exports over the module's memory, so no binding
generator is needed. The page reserves memory with `alloc`, fills it, calls a function with pointers, and frees it with
`dealloc`. ui/src/app/modes/wasm/core.ts wraps these.

## src/what_if.rs

What would raise a clicking run's score. Each line is one change, all else the same: the time it would save over the
run, turned into kills at the run's own pace (its measured kills over the time they took), since the scenario's time
limit stays the same. Each line is a ceiling and they overlap, so they do not add up.

- `Group` (enum): The part of the run a line is about.
- `ClickWhatIf` (struct): One line: the extra kills over the run, the extra score (null when the run gives no score per
  kill), and what was assumed.
- Functions: `click_what_if`.

## service/src/api.rs

The review server's API (python/retired/server.py's), free of any web framework: a request's method, path and query,
Range header and body in (`ApiRequest`), the status, headers and body out (`ApiResponse`). The desktop app answers its
window with it (over a custom protocol), and the HTTP server answers the browser. Routes that need the desktop (the
folder dialog, /api/folder; the mouse logger's switch, /api/mouse/logger) are answered by the desktop app before it asks
here: here they are not found (404). Each route asks the library (library/) for its answer.

- `ApiRequest` (struct): A request: its method ("GET", "POST"), its path with its query ("/api/report?id=..."), its
  Range header if any, and its body (in memory, or for an upload a file).
- `ApiResponse` (struct): A response: its status, its headers (Content-Type always; for a video Accept-Ranges and
  Content-Range) and its body.
- Functions: `handle`.

## service/src/areas.rs

The areas a review leaves out (python/retired/server.py: exclude, set_exclude, kinds, save_kind, find_areas, labelled):
a webcam, another player's overlay.

- `Kind` (struct): A kind of area: its id never changes; its name and what it is can.
- `Library` methods: `kinds`, `save_kind`, `exclude`, `exclude_boxes`, `exclude_areas`, `tracked_with_areas`,
  `exclude_answer`, `set_exclude`, `labelled`, `find_areas`, `examples_text`, `kinds_file`, `set_kinds_file`,
  `set_examples`, `fix_examples`.
- Functions: `kovobs_areas`, `tracked_areas`.

## service/src/batch.rs

Files sent in one body, both ways between the page and the service: each [u32 path length][path, UTF-8][f64 time of
change, seconds since 1970][u32 length][bytes], little-endian. The page sends KovaaK's files and the cut-off's labels
this way (library/browser.rs) and an export's recording to open (library/export.rs); the service answers an export with
one. The page's side is ui/src/app/modes/service/kovaak-batch.ts. In: a body. Out: its files, or a body.

- `BatchFile` (struct): One file of a batch.
- Functions: `read`, `push`.

## service/src/bin/aimview-tool.rs

aimview-tool: the review service's library and its native review from the command line, for the Python scripts
(python/aimview_tools.py runs it). Each command prints JSON on stdout; a failure prints {"error", "status"} there and
exits with 1 (status: 404 for something missing, 400 for a bad value, else 500). A review's progress goes to stderr.
`aimview-tool help` lists the commands and their options.

## service/src/config.rs

What a library needs to know (`Config`): where it keeps its files, the user's folders (the recordings, KovaaK's stats
files and scenarios), the models, the device the detector runs on and where ffmpeg comes from. The desktop app fills it
from its own folders; the review server from its settings (server/src/config.rs); aimview-tool from its options.

- `Config` (struct): A library's settings. Methods: `new`, `folders`.
- `Layout` (enum): Where a library keeps its files in the data folder. Methods: `folders`.
- `Device` (enum): The detector's device. Methods: `from_name`, `built`, `name`.
- `Ffmpeg` (enum): Where ffmpeg and ffprobe come from.
- `Folders` (struct): The folders a layout puts the library's files in.

## service/src/crops.rs

The Crops page's files: the check folders of detector crops (python/model/crop_check/make_page.py writes each: its
crops.json, sets.json and crops/<id>.png), the user's answer to each crop (answers/checks/<id>.json, the file the
claude.ai check pages' answers were saved to), and the answers moved between modes as one document (browser mode exports
it, the review server imports it). The check folders are the data folder's crops/ in the app's layout (browser mode, the
desktop app) and test_out/vod_model/check_* in Python's (the review server).

- `CropVerdict` (enum): What the user said of a crop: its boxes are right, wrong (and fixed), or it cannot tell.
- `CropAnswer` (struct): A crop's answer: the verdict, its set and file, when (ms since 1970), the model boxes crossed
  out, moved or resized, the boxes drawn and points tapped (the claude.ai pages' fields), whether a suggestion was taken
  as offered, and the scene the Crops page drew (src/shapes.rs), when it drew one.
- `CropEntry` (struct): A crop as make_page.py lists it: its id, set and file, the recording's folder and kind, why it
  was picked, the rule that mined it, the model's boxes ([cx, cy, w, h], crop pixels) and scores, and the boxes it
  starts crossed out.
- `CropPreset` (struct): The model boxes a crop starts with crossed out (a mined false box: Right agrees it is no
  target).
- `CropSet` (struct): A set of a check folder: its tab's title, the note over a crossed-out box, whether its answers
  teach the page's suggestions, and how many crops it has and how many are answered.
- `CropPage` (struct): A check folder (the page names it by its folder's name) and its sets.
- `CropAnswers` (struct): A check folder's answers as one document, to move them between modes: browser mode exports it,
  the review server imports it.
- `CropChecks` (struct): The Crops page's check folders in the layout's crops folder (config.rs `Folders`): what the
  page lists, shows and answers. Methods: `new`, `crop_pages`, `crops`, `crop_answers`, `crop_image`,
  `save_crop_answer`, `export_crop_answers`, `import_crop_answers`, `crop_labels`.
- `Library` methods: `crop_pages`, `crops`, `crop_answers`, `crop_image`, `save_crop_answer`, `export_crop_answers`,
  `import_crop_answers`, `crop_labels`.
- Constants: `CROP_PX`.

## service/src/database.rs

The store as one SQLite database in the data folder (docs/storage-design.md): what store.rs's `Files` keeps as files,
kept as rows instead, the same bytes in and out, so every answer is the same. The reviews' parts are kept
gzip-compressed. On its first opening it imports what the data folder's files hold, in one transaction, and leaves the
files where they are. The SQL runs through sql.rs, natively SQLite through rusqlite. In: the library's items and their
bytes, and on first opening the data folder's files. Out: the same bytes, and what is kept for each recording.

- `Database` (struct): The store as one SQLite database (see the module's comment). Methods: `open_file`, `open`.
- Constants: `DATABASE_FILE`.

## service/src/detector.rs

The detector model on this computer, on the device the configuration says (config.rs: `Device`): ONNX Runtime with
DirectML (any Windows GPU), with CUDA (an NVIDIA GPU, with the `cuda` feature) or on the CPU; `Auto` tries the GPU first
and falls back to the CPU. It takes the _u8in export (python/model/export.py): a batch of 720p RGB frames and the fixed
map, as bytes. The model's settings come from its settings file beside it (`model_settings`). In: the review's frames
(review.rs). Out: each frame's score and reg maps, which the core's tracking takes.

- `Detector` (struct): The detector model loaded in ONNX Runtime, with the fixed map ready for every call. Methods:
  `new`, `run`.
- `Maps` (struct): One call's maps: score (batch x 1 x MAP_HEIGHT x MAP_WIDTH) and reg (batch x REG_MAPS x MAP_HEIGHT x
  MAP_WIDTH), where ONNX Runtime left them. Methods: `of_frame`.
- Functions: `model_settings`.
- Constants: `MAP_WIDTH`, `MAP_HEIGHT`.

## service/src/disk.rs

The file system and the clock, for the whole service: every file the library reads or writes goes through here. In: the
library's paths and bytes. Out: the files' bytes, listings and metadata, the time and the time zone.

- `Metadata` (struct): A file's or a folder's size, time of change (seconds since 1970) and kind. Methods: `len`,
  `modified`, `is_dir`, `is_file`.

## service/src/faint.rs

The faint-target cut-off (python/retired/server.py: faint, set_faint, submit_faint, skip_faint, faint_queue).

- `Library` methods: `faint`, `set_faint`, `cutoff_labels_count`, `cutoff_labels_zip`, `submit_faint`, `skip_faint`,
  `faint_queue`.

## service/src/ffmpeg.rs

ffmpeg for the review, from where the configuration says (config.rs: `Ffmpeg`): the PATH, a folder, or found the way
KovOBS finds it: the PATH's ffmpeg and ffprobe when both run, else (ffmpeg-sidecar) downloaded into a folder the first
time a review needs it, and unpacked there (the desktop app does not ship it). On Windows the download is BtbN's GPL
build, which has the dav1d AV1 decoder: gyan.dev's essentials build (KovOBS's) decodes AV1 with libaom, 2.5 times
slower. Until a library sets it (the examples), ffmpeg comes from the PATH. In: the library's configuration. Out: the
programs' paths, which video.rs and links.rs run.

- Functions: `set_source`, `program`, `ensure`.

## service/src/finder.rs

The area finder (src/areas.rs, python/areas.py) on a recording: its frames read, and what it found kept with the
recording as python/areas.py keeps it (store.rs; areas.json: the found areas; areas_maps.npz: the stand-out and change
maps). A review keeps what the finder found in the key frames it reads anyway (review.rs); a recording not reviewed yet
is read here when its areas are first asked for (in the browser build the page reads it, with the core's finder in a
worker, and sends what it found: library/browser.rs). In: a video, or a review's finds. Out: those two items, which
areas.rs reads for /api/find_areas.

- Functions: `analyse`, `found`, `maps`, `keep`, `keep_with_review`.

## service/src/gpu_frames.rs

A run's frames decoded on the GPU (Windows): Media Foundation decodes the recording into D3D11 textures, and a compute
shader (gpu_frames.hlsl) makes the detector's 1280 x 720 RGB with src/convert.rs's 2:1 integer arithmetic, the 720p luma
the camera reads (the same means), and the Y plane's top rows the HUD reads (only those: the whole plane, 3.7 MB a
frame, was most of what the CPU copied back). They are read back a few frames behind, through a ring of staging buffers,
so the GPU never waits for the CPU. The CPU does no decoding and no conversion: ffmpeg's software decode took about 7 ms
of CPU a 1440p frame. prototypes/gpu_decode checked the decoded frames against ffmpeg's and the RGB against
convert.rs's, byte for byte. Media Foundation counts its times from the file's earliest frame, the pre-roll an MP4 edit
list hides included (OBS's AV1 files have about 100 such frames; its H.264 files none), where ffmpeg's and the browser's
start at the first frame shown: a frame's time here is its time there less `VideoInfo::earliest`. Only 2560 x 1440 AV1
or H.264 MP4s (`usable`); other videos keep ffmpeg (video.rs). In: the video, its `VideoInfo`, where a run starts, the Y
plane's rows wanted. Out: each frame's RGB, 720p luma and those rows.

- `GpuFrames` (struct): A run's frames from the GPU, in order: `next_into` gives each one's RGB, 720p luma and Y plane's
  top rows. Methods: `open`, `next_into`.
- Functions: `usable`.

## service/src/labels.rs

Labelling (python/retired/server.py): the recordings the user marked as another game, the queue of recordings to label
areas in and the ones skipped there, kept as the review server keeps them (store.rs: sorted lists of recording ids,
not_aim_trainer.json and label_skipped.json in its data folder). In: the page's marks and skips (/api/not_aim,
/api/label_skip) and the recordings list. Out: those lists, and the queues the page labels from (/api/label_queue;
faint.rs's).

- `Library` methods: `not_aim`, `set_not_aim`, `skip_label`, `label_queue`.

## service/src/lib.rs

Aim View's review service: the review server's API (python/retired/server.py's) over a library of recordings, with the
review run natively (ffmpeg's frames, the core, and the detector on the GPU). The desktop app (desktop/) and the HTTP
server (server/) serve it: each opens a `Library` from a `Config` and answers requests with `api::handle`. Python's
scripts use the library and the native review through aimview-tool (src/bin/aimview-tool.rs).

## service/src/library/browser.rs

The browser build's own routes (api.rs): the page runs the review and the area finder itself and sends what they give,
which is kept as the native review keeps it; it adds raw mouse logs, chooses the VODs folder (a folder it mounted),
sends KovaaK's files the user chose, read once, and the detector labels a cut-off's submit made. In: /api/job (POST),
/api/reviewed, /api/found, /api/mouse_log, /api/folder, /api/kovaak, /api/kovaak_files and /api/cutoff_labels. Out: the
reviews, found areas, mouse logs, settings, KovaaK's runs and scenario facts and the cut-off labels kept, and the jobs'
state.

- `Library` methods: `choose_vods`, `page_progress`, `review_done`, `keep_found`, `keep_mouse_log`, `kovaak_files`,
  `add_kovaak_files`, `add_cutoff_labels`, `kovaak_changed`.

## service/src/library/export.rs

Recordings shared as one zip (docs/storage-design.md, "Export"): what the service keeps of each, for the page to write
into the zip beside the videos, and what the page gives back when it opens one. The same in every mode.

- `Library` methods: `export`, `import_recording`.

## service/src/library/links.rs

Recordings added from a link: a video's page on a site yt-dlp reads (YouTube, Twitch, Medal, Streamable...) or a video
file's address. yt-dlp (ytdlp.rs) reads the link's title and qualities; the chosen quality is downloaded in a job
(/api/job: stage "downloading", megabytes done of how many) into a folder of its own in the uploads, and moved into
place as one MP4 file when it is complete, so the list never shows half a file. Once it is there the job is gone (stage
"none"); a failure ends it with yt-dlp's reason. In: /api/link/formats and /api/link. Out: the video in the uploads, and
the new recording's row.

- `Library` methods: `link_formats`, `add_link`.

## service/src/library/mod.rs

The library: the user's recordings (the VODs folder and the uploads), KovaaK's stats files and scenarios, the models,
and each recording's reviews, kept in the data folder (config.rs: `Layout`). It answers what the review server
(python/retired/server.py) answered (api.rs); the review itself runs natively (review.rs), or in the browser build in
the page (browser.rs).

- `Failure` (struct): An error for the page: its message, and the HTTP status the API answers with. Methods: `missing`,
  `bad`.
- `Answer` (type): A library answer: the value, or the failure the API answers with.
- `Library` (struct): The user's recordings and everything kept for them; one per process, shared by every request's
  thread. Methods: `open`, `config`, `folders`, `crop_checks`.
- Constants: `FOUND_NEEDED`.

## service/src/library/names.rs

File names and time stamps: a recording's name as KovOBS writes it, a stats file's as KovaaK writes it, their time
stamps, and a recording's folder name (python/retired/server.py: NAME, STATS_NAME, stamp_seconds, cache_dir). In: file
names and times. Out: their parts and stamps, for the recordings list, the stats files' pairing, uploads and links.

- Functions: `parse_name`, `parse_stats_name`, `stamp_seconds`, `slug`, `local_stamp`.

## service/src/library/recordings.rs

The recordings: the list (python/retired/server.py: Library.list), a recording's video from its id and its folder,
videos and stats files added from the user's computer, and each scenario's facts from its scenario file.

- `Library` methods: `resolve`, `review_dir`, `scenarios`, `set_hitbox`, `set_kind`, `recordings`, `upload`,
  `upload_file`, `spool`.

## service/src/library/reviews.rs

A recording's reviews (each model's kept apart, store.rs: tracks, readings, what the HUD read, the kills' check): the
review on show, the review jobs (each runs in a thread of its own; in the browser build the page runs it, browser.rs),
the user's run window and the report, worked out when it is shown (python/retired/server.py: shown, analyse, run,
set_run, /api/report). In: /api/analyse, /api/job, /api/cancel, /api/run, /api/tracks and /api/report. Out: the kept
reviews (review.rs's results), the run window (run_window.rs) and the answers.

- `Job` (struct): A review job: its stage, how far it is (frames), the device its detector runs on once it has loaded
  ("DirectML", "CUDA" or "CPU"; "DirectML and CPU" when its runs' differ), and at the end its time or its error.
- `Library` methods: `shown`, `job`, `analyse`, `cancel`, `marks`, `set_marks`, `tracks`, `report`.

## service/src/library/settings.rs

What the user set, kept in settings.json: the VODs folder they chose in the app (`vods`), the model new reviews use
(`model`), the device the detector runs on (`device`) and the frames it takes at once on each device (`batch`, by device
name). Other keys in the file are kept as they are (the desktop app also kept KovaaK's folder as `kovaak`;
python/retired/server.py kept only `model`). And the models to pick from (models.json and the exports in the models
folder). In: /api/model, /api/device, /api/batch and the app's folder dialog. Out: settings.json, /api/models' answer,
and the model, device and frames at once new reviews use (reviews.rs).

- `Library` methods: `vods`, `set_vods`, `model`, `device`, `batch`, `use_device`, `use_batch`, `default_model`,
  `model_file`, `models`, `pick`.
- Constants: `BEST`, `BATCHES`.

## service/src/library/stats.rs

KovaaK's stats files and each recording's pairing with one (python/retired/server.py: stats_index, stats_for, stats_of,
stats_info, set_stats): the user's choice (stats.json in the recording's folder), else one uploaded beside it, else the
stats file of the same scenario whose time is nearest the recording's.

- `Library` methods: `stats_folder`, `stats_for`, `history`, `stats_path`, `stats_info`, `set_stats`.

## service/src/library/usage.rs

What the library keeps and how much space each part takes, for the data panel (docs/storage-design.md, "Space and
cleanup"), and removing a part the user can do without. In: the store's sizes (store.rs), the data folder's folders and
the settings. Out: GET /api/storage, {total, parts: [{id, kind, bytes, ...}]}, and POST /api/storage?remove=, after
which the database gives back the space freed.

- `Library` methods: `storage`, `remove_storage`.

## service/src/mouse.rs

A recording's measures from the raw mouse logs (python/mouse_log.py's, or the desktop app's logger's, in the layout's
mouse folder): the newest log that covers the recording's run, read by the core (src/mouse.rs, as python/mouse_read.py
reads it). In: the recording's stats file and the mouse folder's logs. Out: the run's measures, which /api/mouse
answers.

- `Library` methods: `mouse_measures`.
- Functions: `measures_in`.

## service/src/npz.rs

NumPy's .npz files as python/ writes them with `np.savez_compressed` (a zip of .npy arrays, deflated): the cut-off's
detector labels (faint.rs) and the area finder's maps (finder.rs), so Python's tools read what the app writes and the
app reads what Python wrote. In: arrays to save, or a file's bytes to read one from. Out: the file's bytes, or the
array.

- `Dtype` (enum): An array's element type: bytes, or 32-bit floats (little-endian).
- `Array` (struct): An array: its type, its shape (empty for a single value) and its bytes in C order. Methods: `u8`,
  `f32`, `floats`.
- Functions: `to_bytes`, `array`.

## service/src/pyjson.rs

JSON as python/retired/server.py read and wrote it, so the files the app keeps are the review server's, byte for byte.

- Functions: `parse`, `to_vec`, `write_text`, `append_text`, `dump`, `load`, `float_repr`.

## service/src/report.rs

A review's report, worked out by the core as the browser does (src/review.rs: `review_json`), from what the review keeps
(store.rs: `Part`): its tracks, readings and what the HUD read (a review made before the HUD was read has none). With a
stats file the core reviews from it; without one, from the HUD's reading, else from the video alone
(python/retired/server.py did the same). In: the review's parts and the recording's stats file, run marks, facts and
cut-off (library/reviews.rs; aimview-tool's from a folder, store.rs: `folder_parts`). Out: the report's JSON, which
/api/report answers.

- Functions: `work_out`.

## service/src/review.rs

A recording's review on this computer: ffmpeg decodes the frames and the core converts them to ffmpeg's 720p RGB byte
for byte (or, for the videos gpu_frames.rs takes, the GPU does both), the detector runs on the GPU or the CPU
(detector.rs), and the core's review session (aimview::session, which the browser's workers feed the same way) does the
rest: it plans the runs, reads the key frames, tracks each run's frames, watches the camera's turn and the HUD, and
joins the runs. The runs are reviewed at once: one ffmpeg decoder is the limit, as one browser decoder was. Without a
stats file the kills the video alone gives are then checked in the frames round them (`check_kills`). The browser build
has only what a review is (`Request`): the page runs it (library/browser.rs). In: a `Request` (library/reviews.rs,
aimview-tool, the track example). Out: the review's tracks, readings, HUD reading, found areas and kill check
(`Reviewed`), which library/reviews.rs keeps.

- `Request` (struct): What to review: the video, the model and its device, how the work is split, and what the review
  leaves out, keeps and checks.
- `Reviewed` (struct): A review's tracks, the video's readings, what the HUD read (None: no HUD was read), the areas the
  area finder found in the key frames it read (None when the recording has too few for it: areas.rs reads its frames
  then), and the check of the kills the video alone gives (None: not asked for).
- `Progress` (type): Where a review stands: its stage ("looking" at the key frames, "tracking", "linking"), frames done,
  of how many.
- `DeviceNote` (type): Told the device each run's detector runs on ("DirectML", "CUDA" or "CPU") once it has loaded:
  with `Device::Auto` the CPU when the GPU could not start it.
- Functions: `add_device`, `frame_bytes`, `parts_at_once`, `review`.
- Constants: `CANCELLED`, `MIN_GPU_SHARE`.

## service/src/run_window.rs

The user's run window for a recording: where the run starts and ends, kept with the recording (store.rs; as
python/retired/server.py kept it, run.json in its folder). In: the marks the page sends (/api/run). Out: the kept marks,
the part of the video a review tracks (with a margin: `RunMarks::tracked`, for library/reviews.rs) and the marks the
report measures within (report.rs).

- `RunMarks` (struct): The marks in seconds, any of them None. Methods: `read`, `is_set`, `parse`, `save`, `tracked`.
- Functions: `covers`.

## service/src/sql.rs

The SQL the data folder's database runs (database.rs), behind one interface (`Sql`) so its statements are written once
(docs/storage-design.md): natively SQLite built into the exe through rusqlite (`Sqlite`), in the browser build SQLite's
own WebAssembly in the page (`HostSql`, the `host_sql` import, in a binary form both sides read: tagged values, rows as
a column and a row count before them). In: a statement and its values. Out: the rows it gives.

- `SqlValue` (enum): A value a statement takes or a row gives (SQLite's five kinds).
- `Sql` (trait): One connection to a database: one caller at a time (database.rs holds it behind a mutex).
- `Sqlite` (struct): A database file through rusqlite, in WAL mode: readers don't wait on a writer, and a crash
  mid-write loses nothing committed. Methods: `open`.
- `HostSql` (struct): The page's database, through the `host_sql` import: SQLite's own WebAssembly in the service's
  worker, on the browser's private file system through its pool of sync access handles, so each call returns at once (no
  Asyncify wait; ui/src/app/modes/service/service-database.ts answers it).
- Functions: `encode_values`, `decode_rows`.

## service/src/store.rs

What the library keeps, through one interface (`Store`), so where it is kept can change (docs/storage-design.md): the
files in the data folder (`Files`, laid out as config.rs's layout says) or one SQLite database (database.rs). The
library formats each thing (JSON as python/retired/server.py wrote it, .npz as NumPy does); a store keeps the bytes it
is given and gives the same bytes back. The videos (uploads), the mouse logs (the desktop app's logger writes them) and
the crop-check folders stay files outside it. In: the library's items and their bytes. Out: the same bytes, and what is
kept for each recording; the space it takes, behind a narrower interface (`StoreUsage`).

- `IdList` (enum): A list of recording ids the user marked.
- `Mark` (enum): What is kept for a recording beside its reviews. Methods: `file_name`.
- `Part` (enum): A part of a review: its tracks, the video's readings (the camera's turn, the countdown), what the HUD
  read, and the check of the kills the video alone gives. Methods: `file_name`.
- `ReviewBy` (enum): Which of a recording's reviews: a model's, or the one python/retired/server.py kept before reviews
  were kept per model.
- `Item` (enum): One thing the library keeps. Methods: `file_name`.
- `StoreUsage` (trait): The space a store's parts take and their removal, for the data panel (library/usage.rs): all a
  caller needs that only counts or frees space.
- `Store` (trait): Where the library keeps what it keeps (see the module's comment).
- `ReviewSize` (struct): One model's reviews: the model ("" the old reviews, from before reviews were kept per model),
  how many recordings it reviewed, and the bytes its reviews keep (compressed in the database).
- `StatsRun` (struct): A run as its stats file's footer gives it: KovaaK's score, the kills when the file gives them,
  and hits over shots (0 to 1) when it gives both.
- `StatsRow` (struct): A stats file of KovaaK's as the browser keeps it: its name, size and time of change (to tell a
  changed file), and its run (None: the file has no score).
- `ScenarioRow` (struct): A scenario file of KovaaK's as the browser keeps it: its path in /kovaak (scenarios/<name>.sce
  or workshop/<item>/<name>.sce), its size and time of change, and its facts.
- `Kovaak` (trait): KovaaK's files as the browser keeps them (see `Store::kovaak`): every stats file's run, the whole
  text only of those a recording used, and every scenario's facts.
- `Files` (struct): The store as the files in the data folder (disk.rs), laid out as the layout's folders say: today's
  files, byte for byte. Methods: `new`, `path`.
- Functions: `folder_size`, `recording_folder`, `folder_parts`.

## service/src/video.rs

A recording's frames from ffmpeg (ffmpeg.rs: the PATH's, a folder's or a downloaded one), as Python's review decoded
them (python/retired/review.py: `_frames`): the video's own YUV 4:2:0 at its size, through a pipe, so the core converts
them to the same bytes. ffprobe gives the frames' times, the key frames and the colors. In: a video's path. Out: what
the video is (`VideoInfo`) and its frames, for the review (review.rs) and the area finder (finder.rs).

- `VideoInfo` (struct): What a recording is: its frames' size, rate and colors, every frame's time (from 0 on, in order:
  the edit list's pre-roll before 0 is not shown), the key frames' times, its duration as ffprobe gives it, and its
  earliest frame's time, the pre-roll's included (Media Foundation counts its times from that frame: gpu_frames.rs), in
  seconds.
- `Frames` (struct): A recording's frames from an ffmpeg process, one at a time; the process ends when this is dropped.
  Methods: `open`, `next_into`.
- Functions: `probe`.

## service/src/ytdlp.rs

yt-dlp, for recordings added from a link (library/links.rs): found as ffmpeg is (ffmpeg.rs), the PATH's when it runs,
else the official release from GitHub, downloaded once into the tools folder beside ffmpeg's.

- `LinkInfo` (struct): What a link offers: its title, its length in seconds, when it was uploaded, and the qualities to
  choose from, best first.
- `Choice` (struct): A quality to download: yt-dlp's format id, its frame size, frame rate, video codec, and size in
  bytes (with the best audio) where yt-dlp knows it.
- Functions: `tools_folder`, `ensure`, `reason`, `info`, `format_spec`, `download`.

## server/src/access.rs

Who may use the server. With no token (only on a loopback address), anyone on this machine: requests must name a
loopback host and come from a loopback page, so a web site cannot reach the server through the browser (no DNS
rebinding, no cross-site requests but a loopback page's: the UI in browser mode, on http://localhost:4200, asks the
server to download a link, and may read the answers: `loopback_caller`). With a token, every request carries it:
`Authorization: Bearer <token>`, or the cookie that visiting `/?token=<token>` once sets (SameSite=Strict, so other
sites' requests do not carry it). In dev mode no token is used: on a network address, any device on the local network
gets in, by an address or a machine's name (still no rebinding, and no other site's page).

- `Verdict` (enum): What to do with a request.
- `Access` (struct): Who may use this server: its token, if any, and whether dev mode opens it to the local network.
  Methods: `new`, `has_token`, `is_open_network`, `check`.
- Functions: `loopback_caller`, `all_loopback`.
- Constants: `COOKIE_NAME`.

## server/src/config.rs

The server's settings: the command line, over a settings file (TOML), over the defaults. The defaults are the repo's
settings (aimview.defaults.json under this computer's aimview.json: aimview::local_config): the repo's test_out/ as the
data folder (Python's layout, python/retired/server.py's), the recordings' folder this computer names, KovaaK's folders
under Steam's, and the models in python/model/.

- `Switch` (enum): A setting turned on or off on the command line.
- `Device` (enum): Where the detector runs. Methods: `flag`.
- `FfmpegChoice` (enum): Where ffmpeg comes from.
- `Flags` (struct): The command line.
- `FileSettings` (struct): The settings file: the same settings as the flags, each one optional.
- `Settings` (struct): The settings the server runs with. Methods: `defaults`, `url`.
- Functions: `parse_file`, `resolve`, `load`.
- Constants: `DEFAULT_FILE`.

## server/src/files.rs

The UI's files (its server-mode build). A path that is not a file there is one of the single-page app's own pages: it
gets index.html, and the app shows that page.

- `Found` (enum): What a UI path is.
- Functions: `is_api`, `find`, `content_type`.

## server/src/glue.rs

The review service (aimview-service) behind the HTTP side: the settings as its `Config`, each call as its `ApiRequest`.

- Functions: `open`, `describe`.

## server/src/http.rs

The HTTP side: each request is checked (access.rs), then goes to the review API or to the UI's files. The API answers on
a blocking thread of its own: a listing or a report reads files, and a review's start can take a while (the review
itself runs in the background, polled with /api/job).

- `Call` (struct): One request to the review API. Methods: `get`.
- `Reply` (struct): The review API's answer.
- `Api` (trait): The review API (aimview_service::api, or a stand-in in the tests).
- `App` (struct): What every request is answered with: the API, who gets in, and the UI's build.
- Functions: `router`.

## server/src/main.rs

Aim View's review server: the UI's server-mode build and the review server's API (python/retired/server.py's, served by
the aimview-service crate as the desktop app serves it) over plain HTTP.

## desktop/src/lib.rs

Aim View's desktop app: the Angular app (ui/, its desktop build) in a Tauri 2 window. The window's server-mode services
talk to the app itself (protocol.rs: the review server's API over the `api` protocol), which the review service answers
(service/: the library in the app's data folder, and the review run natively). The app adds the folder dialog and the
raw mouse logger (mouse.rs). In: Tauri's folders for the app (its data, local data and resource folders) and the
window's requests. Out: the window, the library's files in the data folder, ffmpeg in the local data folder, and the
mouse logs.

- Functions: `run`.

## desktop/src/main.rs

Aim View's desktop app: the executable, which starts the app (lib.rs `run`). In: nothing. Out: the app's window, or the
mouse logger's process (lib.rs, mouse.rs).

## desktop/src/mouse.rs

The raw mouse logger (python/mouse_log.py, ported) and the app's side of it: an on/off switch that logs in the
background while the user plays. Each recording's measures from the log that covers its run are the service's
(service/src/mouse.rs).

- `Logger` (struct): A message-only window that turns WM_INPUT messages into records in `records`. Methods: `new`,
  `register`, `drain`, `clock_pair`, `wait`, `close`.
- `Logged` (struct): What a finished log holds, as the logger reports it.
- Functions: `qpc_now`, `time_ns`, `local_stamp`, `throttle_setting`, `log_to`, `logged_text`, `bench`, `set_folder`,
  `child_main`, `logger_state`, `set_logger`.

## desktop/src/protocol.rs

The review server's API inside the app, over a custom protocol (`api`, at http://api.localhost in the window): no
network port, so nothing outside the app reaches it. The window's server-mode services send /api/... and /video there
(ui/src/app/modes/tauri/). The service answers (aimview_service::api); the routes that need the desktop are answered
here: the folder dialog (/api/folder) and the mouse logger's switch (/api/mouse/logger).

- Functions: `handle`.

## browser-service/src/lib.rs

The review service (aimview-service, without its `native` feature) as WebAssembly: browser mode runs it in a worker, so
the browser answers the same API as the review server and the desktop app. Its files are the page's mounted folders,
read and written through the host's imports (service/src/disk.rs: module "host", `host_fs` the one asynchronous call,
which Asyncify suspends: scripts/ui-assets.ts runs wasm-opt on this module).

## tests/camera_parity.rs

The camera's readings (src/camera.rs) against Python's camera_motion on the same gray frames:
python/retired/tests/fixtures.py --review writes sample frame pairs (gray.raw, gray.json) and every frame's reading
(camera.json), and the countdown-teal counts (teal.json, checked in the browser). The FFTs differ in rounding (rustfft
against SciPy's pocketfft, both in single precision), so readings must agree within 0.001 degrees.

## tests/camera_same.rs

The camera watch's tile shifts (src/camera.rs) against the ones it gave before, to the bit, on the parity cases' frame
pairs: a change made for speed must not change a reading. AIMVIEW_KEEP_SHIFTS=1 stores the shifts as they are now
(test_out/parity/<case>/review/camera_shifts.json); without it they are compared with the stored ones.

## tests/code_map.rs

The code map (docs/CODEMAP.md), built from the Rust sources so a reader finds where a thing lives with one read or one
search instead of opening files: each file's header comment, then its public types with the first sentence of their doc,
their methods, and the file's functions and constants by name. The test fails when docs/CODEMAP.md is out of date;
`CODE_MAP_WRITE=1 cargo test --profile quick --test code_map` writes it again.

## tests/common/mod.rs

What the parity tests share: the frozen fixtures in test_out/parity (python/retired/tests/fixtures.py made them),
reading their JSON, excluded areas, gray frames and tracking inputs, and comparing the core's JSON with Python's:
everything that is not a number equal, numbers within a relative tolerance.

- `Diff` (struct): Where two JSON values differ (paths), and how many numbers were equal only within the tolerance.
  Methods: `print_wrong`, `assert_none`.
- `GrayFrames` (struct): A camera case's gray frames as python/retired/tests/fixtures.py --review kept them
  (review/gray.raw: one 1280 x 720 luma frame after another, the frame numbers in review/gray.json), and the pixels the
  camera watch leaves out (the KovOBS overlay and the fixed map's pixels, from fixed.npy). Methods: `read`, `frame`.
- `TrackingInputs` (struct): A tracking run's inputs as python/retired/tests/fixtures.py --review kept them in
  test_out/parity/<case>/review/: its tracks, the camera's readings and the countdown (from teal.json's counts).
  Methods: `read`, `review`.
- Functions: `compare`, `rename_key`, `without_tracking_checks`, `read`, `read_lossy`, `parity_root`, `fixture_dirs`,
  `excluded_areas`, `showing_frames`, `fixed_map`, `scenario_key`.
- Constants: `FRAME_PIXELS`.

## tests/compare.rs

The parity tests' comparison (tests/common): it compares only the fields Python's output has, so a field the core adds
passes, while a field Python has that the core lacks, or a different value, is a difference.

## tests/convert_parity.rs

`convert::Converter` against ffmpeg 8.1 itself: frames of real recordings (AV1, H.264 and HEVC; 2560, 1920 and 1280
wide; full and limited range) decoded as they are (`<key>_<n>_src.yuv`), and ffmpeg's `scale=1280:720:flags=area` to
rgb24 and yuv420p, with its x86 kernels (`.raw`) and its plain C code (`_c.raw`, `-cpuflags 0`). Every byte must be
equal, with the 2:1 shortcut and through the full pipeline. Data in test_out/parity/convert/ (meta.json: each source's
size and range).

## tests/faint_parity.rs

The faint-target cut-off (src/faint.rs, review.rs) against Python's: python/retired/tests/fixtures.py --faint writes
test_out/parity/<case>/faint/<offset>/report.json (a tracking review with the cut-off on) and
test_out/parity/faint/<recording>.json (the scores, the cut and the labels of every recording the user set a cut-off
for). Everything that is not a number must be equal; numbers within 1e-9 of Python's (relative). Each recording's
tracks, which Python read from its review in the data folder, are kept in test_out/parity/faint_tracks/ too: the Storage
panel can remove those reviews (2026-10-08 it did), and the test needs the very tracks Python read.

## tests/fixed_parity.rs

`fixed::FixedMap` against Python's `fixed_map` on a real recording's key frames (keys.yuv: YUV 4:2:0 at 1280 x 720, as
ffmpeg gave them to Python), to the bit (fixed.npy). Fixtures from python/retired/tests/fixtures.py.

## tests/keep_parity.rs

`track::keep` against Python's `keep` (python/retired/review.py, `track_model`) on real recordings: the detector's raw
boxes per frame (raw.json) must give the same targets (dets.json), to the bit, with the recording's excluded areas and
target count (meta.json), and the frames where a pop-up area is off kept again (`reopen`, with the pop-ups Python found,
meta.json's `showing`). Fixtures from python/retired/tests/fixtures.py, in test_out/parity/<name>/.

## tests/link_parity.rs

`track::link` against Python's `link` on real recordings: the fixtures python/retired/tests/fixtures.py writes to
test_out/parity/<name>/ (dets.json: link's input, frames.json: its output). Every value must be equal, to the bit.

## tests/mouse_parity.rs

The mouse log reader (src/mouse.rs) against python/mouse_read.py on the same logs: tests/mouse_fixtures.py writes
test_out/parity/mouse/<case>/ (the log, its stats file, and want.json with what Python prints and writes). The printed
text must be the same, and every number equal to the bit.

## tests/popup_parity.rs

`popup::AreaWatch` against Python's `AreaWatch` on real recordings with a pop-up area: every frame decoded by ffmpeg as
python/retired/review.py's `rgb_frames` does (scale=1280:720:flags=area, rgb24), and the per-frame decisions compared
with Python's (meta.json's `showing`). Fixtures from python/retired/tests/fixtures.py (with --areas). Decodes whole
recordings, so it runs on request: cargo test --release --test popup_parity -- --ignored

## tests/python_parity.rs

`python::hypot` against CPython's `math.hypot` on 20,000 random pairs and a few edge cases, to the bit
(test_out/parity/hypot.json: [x, y, math.hypot(x, y)] rows, made by python/retired/tests/fixtures.py's hypot cases), and
the KovOBS overlay's boxes against Python's (test_out/parity/overlay.json).

## tests/reload_runs.rs

The forced reloads (src/reload.rs) on real runs of scenarios whose magazine runs out, reviewed with their stats files
into test_out/reload_runs/<run>/ (`aimview-tool review <video> --out <that folder> --stats-file <csv>`), with the ammo
rules read from the scenario's file. `--nocapture` prints each run's reloads and its what-if line.

## tests/replay.rs

The review after the detector, replayed from the parts the native review kept, without the video: each run's track part
(the detector's boxes, the pop-up areas' looks) and watch part (the camera's tile shifts, the countdown, the HUD's
glyphs) joined as the review joins them (keep, the pop-ups, link, the camera's readings, the HUD's reading), then the
report worked out as the service does (matching, measures, summary, checks). Every output must equal the native
review's, byte for byte: the byte-compare's baseline (docs/BENCH.md, Correctness; `NATIVE`/<video>/: tracks.json,
readings.json, hud.json, report.json; no_stats/ without the stats file). A change after the detector is checked here in
about a second a video, not with a whole review. The parts are in test_out/baselines/parts/<video>/, kept once by the
track example's `--parts` (service/examples/track.rs; saved.txt there): setup.json, fixed.bin, run<k>_track.json,
run<k>_watch.json and detector.txt. They change only with what comes before the join (decoding, the conversion, the
detector, the watches' readings of each frame): keep them again then.

## tests/review_parity.rs

The clicking review (src/review.rs) against Python's on the same tracks: python/retired/tests/fixtures.py --review
writes test_out/parity/<case>/review/ (tracks.json, flicks.json, measures.json, report.json). Everything that is not a
number must be equal; numbers within 1e-9 of Python's (relative), since the core uses plain floating point. The checks
are compared by their issue number, flag and numbers, not their words: the core words them in the app's terms (TTK,
micros, confirmation), python/retired/review.py in its own.

## tests/scenario_parity.rs

`scenario::facts` against Python's `scenario_facts` and `target_counts` over every scenario file on this computer
(test_out/parity/scenarios.json: the files in Python's order, and Python's facts by lower-case name). Later files win,
as in Python's dict.

## tests/tracking_parity.rs

The tracking review (src/tracking.rs, src/review.rs) against Python's on the same tracks and camera readings:
python/retired/tests/fixtures.py --review writes test_out/parity/<case>/review/ (tracks.json, camera.json, teal.json,
report.json). Everything that is not a number must be equal; numbers within 1e-9 of Python's (relative).

## tests/what_if_runs.rs

The what-if lines (src/what_if.rs) on real runs with their stats files: the parity runs (test_out/parity/<case>/
review/) and some of the video-alone benchmark's (test_out/vod_model/eval/video_alone/full_v3/<run>/). Each line must be
plausible: at least half a kill, no more than the run's kills, biggest first. `--nocapture` prints them.

## examples/areas.rs

Checks the area finder (src/areas.rs) against python/areas.py's analyse() on recordings: `cargo run --profile quick
--example areas -- <reference folder> <name> [more names]`. The folder holds, for each name, Python's results:
<name>.json (the video, its key frames, the frames read, the found areas, hud.layout's rows) and its maps as raw bytes
(<name>.stand.bin and <name>.change.bin, u8 1280 x 720; <name>.sums.bin, the change map's sums, u32). ffmpeg decodes (it
must be in PATH): the key frames (picked with select, as examples/hud.rs does), and, for a run with fewer than 24 of
them, every frame, from which the frames areas::sample_frames picks are read. The core's Converter scales each to 1280 x
720 YUV 4:2:0, as the review does. Prints one line of JSON for each recording: the maps' differences, each Python area
matched to the core's by IoU with both kinds, the time a frame takes and the size of what finish() gives.

## examples/hud.rs

Reads recordings' HUDs with the core (src/hud.rs) and prints one line of JSON for each: `cargo run --profile quick
--example hud -- <video> [more videos]`. ffmpeg decodes (it must be in PATH): first the key frames, picked with ffmpeg's
select (some builds ignore `-skip_frame nokey` on AV1), then every frame, which the watch reads as the Y plane of
yuv420p. Each recording is read three ways, which must agree: one watch; its part joined into a new watch (as the
desktop app and the page join runs); and two runs, each with the next run's first frame, joined. It also prints the time
a frame takes and how much the watch keeps.

## examples/review.rs

Reviews one request (src/review.rs `ReviewRequest`, as JSON in a file) and prints the outcome, for checks outside the
app: `cargo run --profile quick --example review -- <request.json>`.

## examples/review_runs.rs

Reviews every kept run again and writes its reports: the check that a change after the tracking (matching, measures, the
report) moved nothing it should not. Run it before and after the change into two folders, then compare them with `bun
scripts/same-json.ts <before> <after> [path=name ...]` (docs/BENCH.md, Correctness).

## service/examples/api.rs

The API without a window or a server: requests answered by `api::handle` on a library in a data folder, each answer
printed as a line of JSON ({"status": ..., "body": ...}), to check the answers against python/retired/server.py's on
copies of its data. cargo run -p aimview-service --example api -- <data folder> <models folder> <requests file>
[--layout app|python] [--vods <folder>] [--stats <KovaaK's stats folder>] [--files] The app's layout (the default) reads
the VODs folder from the data folder's settings.json; Python's layout takes test_out/ as the data folder and the VODs
folder from --vods. The app's layout keeps what it keeps in the data folder's database; --files keeps it in files
instead. Each line of the requests file: METHOD PATH (with its query), then a tab and the body when there is one; or
POLL PATH KEYS: the GET asked again (for up to 10 minutes) until one of its answer's KEYS (a|b) is not null; or SLEEP
SECONDS: a wait, for work the library does in the background (the area finder learning).

## service/examples/detector_speed.rs

The detector alone, as the native review runs it (service/src/detector.rs): a model's _u8in export on frames of noise,
`batch` a call, in one session or several at once (a review runs one a part), with the time a frame. For comparing
models, batch sizes and devices without decoding a video. cargo run -p aimview-service --release --example
detector_speed -- <model _u8in.onnx> [batch] [frames] [sessions] [device: auto, directml, cuda or cpu]

## service/examples/frames_check.rs

A video's frames from the GPU (gpu_frames.rs) against ffmpeg's, converted by the core (video.rs, convert.rs), byte for
byte: each frame's RGB, 720p luma and Y plane (all its rows), from the start or from a time on. Reports the frames that
differ and, for the first, which of ffmpeg's frames near it the GPU's equals (a frame lost or doubled shows as a shift).
Windows only, for the videos gpu_frames.rs takes. cargo run -p aimview-service --release --example frames_check --
<video> [frames] [from (seconds)]

## service/examples/mouse_read.rs

A raw mouse log read (python/mouse_read.py's command line, on the core's reader: src/mouse.rs). With a stats file it
measures each flick of the run and writes <log>.kills.json beside the log; without one it sums the log up. cargo run -p
aimview-service --release --example mouse_read -- <log.bin> [--stats "<stats csv>"] [--dpi N] [--cm360 N] [--window MS]
[--start DEG_S] [--stop DEG_S] [--hold MS]

## service/examples/track.rs

A recording reviewed natively, without the app: its tracks, readings and HUD reading written as JSON, the time it took,
and the report the core works out from them (report.json), with the stats file when one is given, else from the HUD's
reading or the video alone. cargo run -p aimview-service --release --example track -- <video> <model _u8in.onnx> <out
folder> [cap] [runs] [batch] [window start] [window end] (seconds: only that part is tracked; "-" for none) [stats file]
[exclude.json] [--parts <folder>] (the review's parts kept there before they are joined, for tests/replay.rs)

## desktop/examples/mouse_log.rs

The raw mouse logger on its own (python/mouse_log.py's command line): logs the mouse to a file while KovaaK's runs.
Ctrl+C stops it; it then prints the events, the duration, the rates and the devices. cargo run -p aimview-desktop
--release --example mouse_log -- [--out FILE] [--seconds N] (default file: test_out/mouse/mouse_<date>_<time>.bin; an
existing file is never overwritten) cargo run -p aimview-desktop --release --example mouse_log -- --bench times the
per-event code path without real input, as mouse_log.py --bench does cargo run -p aimview-desktop --release --example
mouse_log -- --stream SECONDS [--hz N] [--out FILE] checks what the logger records of a stream it is sent: SendInput
mouse moves of zero counts (the cursor stays where it is, no button is pressed), N a second (default 8000), with the
logger in the background as it is while KovaaK's has focus

## benches/hot_paths/frames.rs

The stages a frame goes through: the key frames' fixed map and area finder, the conversion to 720p, the camera watch,
the HUD watch and the pop-up areas' watch.

- Functions: `fixed`, `convert`, `camera`, `hud`, `popup`, `areas`.

## benches/hot_paths/inputs.rs

Where the benches' inputs are: the parity fixtures (test_out/parity/, from python/retired/tests/fixtures.py) and the
native review's kept outputs (test_out/baselines/4b7ddc4/native/).

- Functions: `bytes`, `json`, `text`, `npy`, `areas`.
- Constants: `AV1`, `FLOWER`, `NATIVE`.

## benches/hot_paths/main.rs

The review's hot paths (docs/HOT_PATHS.md) timed with criterion, on real recordings' inputs kept in test_out/ (ignored
by git; docs/BENCH.md, "Function benchmarks"). A bench whose input is missing is skipped with a message. `cargo bench --
<name>` runs the benches whose name holds <name> (`cargo bench -- track/link`).

- Constants: `FEW_SAMPLES`, `FEWEST_SAMPLES`.

## benches/hot_paths/review.rs

The review's last steps, from the joined tracks: the kills matched in the tracks, each flick measured, and the report
worked out (the service's report request).

- Functions: `matching`, `measure`, `report`.

## benches/hot_paths/tracks.rs

The track step after the detector: each frame's boxes kept or dropped (`keep`), then the frames linked into tracks
(`link`, with each frame's view shift).

- Functions: `track`.
