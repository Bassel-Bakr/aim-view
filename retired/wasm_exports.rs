//! Retired on 2026-10-09: the WebAssembly exports of src/wasm.rs that the page no longer called. Not compiled (no
//! crate names this file); kept as AGENTS.md asks, so an export can come back.
//!
//! The browser's review once drove the core piece by piece through them: a tracker, a camera watch and a HUD watch
//! per run (`tracker_*`, `camera_*`, `hud_add` to `hud_finish`), and the report, the scenario's facts and the mouse
//! log in the page (`review_report`, `scenario_facts`, `mouse_read`). The review session's passes (src/session.rs:
//! `review_*`, `tracking_*`, `watching_*`, `joining_*`) took their place, and the review service (service/) answers
//! the rest. The area finder's calls here (`areas_find`, `areas_learn`, `areas_predict`, `areas_same_layout`,
//! `areas_check`, `areas_maps`, `areas_frames`, `areas_zoomed`) went the same way. Only a harness out of git
//! (test_out/browser_check/runs-harness.js) still called them.
//!
//! They were written against src/wasm.rs's imports, as of commit 1588ca6:
//!
//! ```text
//! use crate::convert::{DST_H, DST_W};
//! use crate::track::{RawBox, TrackFrame};
//! use crate::tracker::{TrackPart, Tracker};
//! ```
//!
//! and its helpers `bytes_out` (a buffer that starts with its length, u32 little-endian; freed with
//! `dealloc(ptr, 4 + length)`) and `areas_call` (a JSON call's answer, or {"error": ...}).

/// A tracker for a recording. areas: `areas_len` excluded boxes as shares of the frame, [x0, y0, x1, y1] each
/// (f64); cap: the scenario's target count, 0 for none.
///
/// # Safety
/// `areas` must point to `4 * areas_len` f64s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tracker_new(areas: *const f64, areas_len: usize, cap: usize) -> *mut Tracker {
    let flat = unsafe { std::slice::from_raw_parts(areas, 4 * areas_len) };
    let boxes: Vec<[f64; 4]> = flat.chunks_exact(4).map(|b| [b[0], b[1], b[2], b[3]]).collect();
    Box::into_raw(Box::new(Tracker::new(boxes, cap)))
}

/// A tracker with the KovOBS overlay excluded (the default areas). cap: the scenario's target count, 0 for none.
#[unsafe(no_mangle)]
pub extern "C" fn tracker_new_kovobs(cap: usize) -> *mut Tracker {
    Box::into_raw(Box::new(Tracker::kovobs(cap)))
}

/// The detector model's settings file (detector_<name>.json, UTF-8: src/model.rs) for the tracker, before its first
/// frame. Returns a text as `tracker_finish` does: empty when the file was read, else why it was not (the tracker then
/// keeps the settings it had).
///
/// # Safety
/// `tracker` from `tracker_new`; `text` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tracker_set_model(tracker: *mut Tracker, text: *const u8, len: usize) -> *mut u8 {
    let tracker = unsafe { &mut *tracker };
    let text = String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(text, len) });
    match crate::model::ModelSettings::from_json(&text) {
        Ok(model) => {
            tracker.set_model(model);
            bytes_out(Vec::new())
        }
        Err(error) => bytes_out(error.into_bytes()),
    }
}

/// The frame the next boxes are from, as RGB24 at 1280 x 720: its excluded areas are watched for pop-ups.
///
/// # Safety
/// `tracker` from `tracker_new`; `rgb` must hold 1280 * 720 * 3 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tracker_watch(tracker: *mut Tracker, rgb: *const u8) {
    let tracker = unsafe { &mut *tracker };
    tracker.watch(unsafe { std::slice::from_raw_parts(rgb, DST_W * DST_H * 3) });
}

/// One frame's detector output: the score map (gh x gw) and reg maps (4 x gh x gw), f32. Returns the boxes kept.
///
/// # Safety
/// `tracker` from `tracker_new`; `score` and `reg` must hold `gw * gh` and `4 * gw * gh` f32s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tracker_push_maps(
    tracker: *mut Tracker,
    score: *const f32,
    reg: *const f32,
    gw: usize,
    gh: usize,
) -> usize {
    let tracker = unsafe { &mut *tracker };
    let score = unsafe { std::slice::from_raw_parts(score, gw * gh) };
    let reg = unsafe { std::slice::from_raw_parts(reg, 4 * gw * gh) };
    tracker.push_maps(score, reg, gw, gh)
}

/// One frame's boxes, already decoded: `count` boxes of [cx, cy, w, h, score] (f32, frame pixels). Returns the boxes
/// kept.
///
/// # Safety
/// `tracker` from `tracker_new`; `boxes` must hold `5 * count` f32s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tracker_push_boxes(tracker: *mut Tracker, boxes: *const f32, count: usize) -> usize {
    let tracker = unsafe { &mut *tracker };
    let flat = unsafe { std::slice::from_raw_parts(boxes, 5 * count) };
    let raw: Vec<RawBox> =
        flat.chunks_exact(5).map(|b| RawBox { cx: b[0], cy: b[1], w: b[2], h: b[3], score: b[4] }).collect();
    tracker.push_boxes(&raw)
}

/// The tracker's run starts at frame `first` of the recording: call before its first frame.
///
/// # Safety
/// `tracker` from `tracker_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tracker_start_at(tracker: *mut Tracker, first: usize) {
    unsafe { &mut *tracker }.start_at(first);
}

/// The run's part as JSON (src/tracker.rs: `TrackPart`), and frees the tracker. Free the result as
/// `tracker_finish`'s.
///
/// # Safety
/// `tracker` from `tracker_new`, not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tracker_part(tracker: *mut Tracker) -> *mut u8 {
    let tracker = unsafe { Box::from_raw(tracker) };
    bytes_out(serde_json::to_vec(&tracker.part()).unwrap_or_default())
}

/// The next run's part (`tracker_part`'s JSON), after the frames the tracker has (`Tracker::add_part`). Returns the
/// frames added; none when the part cannot be read.
///
/// # Safety
/// `tracker` from `tracker_new`; `part` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tracker_add_part(tracker: *mut Tracker, part: *const u8, len: usize) -> usize {
    let tracker = unsafe { &mut *tracker };
    match serde_json::from_slice::<TrackPart>(unsafe { std::slice::from_raw_parts(part, len) }) {
        Ok(part) => tracker.add_part(part),
        Err(_) => 0,
    }
}

/// Links the frames and frees the tracker. Returns the tracks as JSON (tracks.json's `frames`), in a buffer that
/// starts with its length (u32, little-endian); free it with `dealloc(ptr, 4 + length)`.
///
/// # Safety
/// `tracker` from `tracker_new`, not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tracker_finish(tracker: *mut Tracker) -> *mut u8 {
    let tracker = unsafe { Box::from_raw(tracker) };
    bytes_out(serde_json::to_vec(&tracker.finish()).unwrap_or_default())
}

/// The frame's luma at 1280 x 720 from its Y plane alone (`y`: the source's w x h bytes), into `out` (1280 x 720
/// bytes): the bytes `converter_yuv420p` gives for Y.
///
/// # Safety
/// `converter` from `converter_new`; `y` must hold `y_len` bytes, `out` 1280 x 720.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn converter_luma(converter: *mut Converter, y: *const u8, y_len: usize, out: *mut u8) {
    let (converter, y) = unsafe { (&mut *converter, std::slice::from_raw_parts(y, y_len)) };
    converter.luma(y, unsafe { std::slice::from_raw_parts_mut(out, DST_W * DST_H) });
}

/// The review's version (src/track.rs: `REVIEW_VERSION`), which the page keeps with a review's tracks.
#[unsafe(no_mangle)]
pub extern "C" fn review_version() -> u32 {
    crate::track::REVIEW_VERSION
}

/// A scenario file's facts (its bytes, UTF-8 or UTF-16 with its mark, at least up to "[Map Data]"), as JSON: {kind,
/// limit, targets, reload, hitbox} (`scenario::Facts`). Free the result as `tracker_finish`'s.
///
/// # Safety
/// `text` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scenario_facts(text: *const u8, len: usize) -> *mut u8 {
    let bytes = unsafe { std::slice::from_raw_parts(text, len) };
    let facts = crate::scenario::facts(&crate::scenario::text_of(bytes));
    bytes_out(serde_json::to_vec(&facts).unwrap_or_default())
}

/// A run reviewed: the request as JSON (src/review.rs: `ReviewRequest`), the outcome as JSON
/// ({report} or {error}). Free the result as `tracker_finish`'s.
///
/// # Safety
/// `request` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn review_report(request: *const u8, len: usize) -> *mut u8 {
    bytes_out(crate::review::review_json(unsafe { std::slice::from_raw_parts(request, len) }))
}

/// A camera watch for a recording (src/camera.rs), its tiles kept clear of the KovOBS overlay and of the fixed map
/// (`fixed`: 1280 * 720 bytes, 1 fixed).
///
/// # Safety
/// `fixed` must hold 1280 * 720 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn camera_new(fixed: *const u8) -> *mut crate::camera::CameraWatch {
    let fixed = unsafe { std::slice::from_raw_parts(fixed, DST_W * DST_H) };
    Box::into_raw(Box::new(crate::camera::CameraWatch::for_recording(fixed)))
}

/// One frame: its YUV 4:2:0 at 1280 x 720 (the luma is read) and its RGB24 at 1280 x 720 (the countdown bar).
///
/// # Safety
/// `camera` from `camera_new`; `yuv` and `rgb` must hold one frame each.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn camera_add(camera: *mut crate::camera::CameraWatch, yuv: *const u8, rgb: *const u8) {
    let camera = unsafe { &mut *camera };
    let gray = unsafe { std::slice::from_raw_parts(yuv, DST_W * DST_H) };
    camera.add(gray, unsafe { std::slice::from_raw_parts(rgb, DST_W * DST_H * 3) });
}

/// Frames not reviewed before the first (a review from part way in): `CameraWatch::skip`.
///
/// # Safety
/// `camera` from `camera_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn camera_skip(camera: *mut crate::camera::CameraWatch, frames: usize) {
    unsafe { &mut *camera }.skip(frames);
}

/// The run's part of the watch as JSON (src/camera.rs: `CameraPart`), and frees the watch. Free the result as
/// `tracker_finish`'s.
///
/// # Safety
/// `camera` from `camera_new`, not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn camera_part(camera: *mut crate::camera::CameraWatch) -> *mut u8 {
    let camera = unsafe { Box::from_raw(camera) };
    bytes_out(serde_json::to_vec(&camera.part()).unwrap_or_default())
}

/// The next run's part (`camera_part`'s JSON), after the frames the watch has (`CameraWatch::join`). Returns the
/// watch's frames after it; none when the part cannot be read.
///
/// # Safety
/// `camera` from `camera_new`; `part` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn camera_add_part(
    camera: *mut crate::camera::CameraWatch,
    part: *const u8,
    len: usize,
) -> usize {
    let camera = unsafe { &mut *camera };
    let slice = unsafe { std::slice::from_raw_parts(part, len) };
    let Ok(part) = serde_json::from_slice::<crate::camera::CameraPart>(slice) else {
        return 0;
    };
    camera.join(part);
    camera.countdown.len()
}

/// The readings, the tracks known (`frames`: the JSON `tracker_finish` gave), as JSON {camera, countdown}, and frees
/// the watch. Free the result as `tracker_finish`'s.
///
/// # Safety
/// `camera` from `camera_new`, not used again; `frames` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn camera_finish(
    camera: *mut crate::camera::CameraWatch,
    frames: *const u8,
    len: usize,
) -> *mut u8 {
    let camera = unsafe { Box::from_raw(camera) };
    let frames: Vec<TrackFrame> =
        serde_json::from_slice(unsafe { std::slice::from_raw_parts(frames, len) }).unwrap_or_default();
    bytes_out(serde_json::to_vec(&camera.finish(&frames)).unwrap_or_default())
}

/// One frame's Y plane (`w` x `h` bytes), in order.
///
/// # Safety
/// `hud` from `hud_new`; `y` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hud_add(hud: *mut crate::hud::HudWatch, y: *const u8, len: usize) {
    unsafe { &mut *hud }.add(unsafe { std::slice::from_raw_parts(y, len) });
}

/// Frames not reviewed before the first (a review from part way in): `HudWatch::skip`.
///
/// # Safety
/// `hud` from `hud_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hud_skip(hud: *mut crate::hud::HudWatch, frames: usize) {
    unsafe { &mut *hud }.skip(frames);
}

/// The run's part of the watch as JSON (`HudPart`), and frees the watch. Free the result as `tracker_finish`'s.
///
/// # Safety
/// `hud` from `hud_new`, not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hud_part(hud: *mut crate::hud::HudWatch) -> *mut u8 {
    let hud = unsafe { Box::from_raw(hud) };
    bytes_out(serde_json::to_vec(&hud.part()).unwrap_or_default())
}

/// The next run's part (`hud_part`'s JSON), after the frames the watch has (`HudWatch::join`). Returns the watch's
/// frames after it; none when the part cannot be read.
///
/// # Safety
/// `hud` from `hud_new`; `part` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hud_add_part(hud: *mut crate::hud::HudWatch, part: *const u8, len: usize) -> usize {
    let hud = unsafe { &mut *hud };
    let bytes = unsafe { std::slice::from_raw_parts(part, len) };
    let Ok(part) = serde_json::from_slice::<crate::hud::HudPart>(bytes) else {
        return 0;
    };
    hud.join(part);
    hud.frames()
}

/// What the HUD read as JSON (`HudReading`, or null without a readable HUD), and frees the watch. Free the result as
/// `tracker_finish`'s.
///
/// # Safety
/// `hud` from `hud_new`, not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hud_finish(hud: *mut crate::hud::HudWatch) -> *mut u8 {
    let hud = unsafe { Box::from_raw(hud) };
    bytes_out(serde_json::to_vec(&hud.finish()).unwrap_or_default())
}

/// A raw mouse log read (src/mouse.rs): the log's bytes, and the request as JSON (`mouse::ReadRequest`: the run's
/// stats file, the options, the UTC offset). The outcome as JSON ({run}, {summary} or {error}); free it as
/// `tracker_finish`'s.
///
/// # Safety
/// `log` must hold `log_len` bytes and `request` `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mouse_read(log: *const u8, log_len: usize, request: *const u8, len: usize) -> *mut u8 {
    let log = unsafe { std::slice::from_raw_parts(log, log_len) };
    bytes_out(crate::mouse::read_json(log, unsafe { std::slice::from_raw_parts(request, len) }))
}

/// A camera watch for a recording (src/camera.rs), its tiles kept clear of the recording's excluded areas and of the
/// fixed map: `areas_len` boxes as shares of the frame, [x0, y0, x1, y1] each (f64), KovOBS's layout when there are
/// none (python/retired/review.py's camera mask); `fixed`: 1280 * 720 bytes, 1 fixed.
///
/// # Safety
/// `areas` must point to `4 * areas_len` f64s; `fixed` must hold 1280 * 720 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn camera_new_areas(
    areas: *const f64,
    areas_len: usize,
    fixed: *const u8,
) -> *mut crate::camera::CameraWatch {
    let flat = unsafe { std::slice::from_raw_parts(areas, 4 * areas_len) };
    let mut rects: Vec<[f64; 4]> = flat.chunks_exact(4).map(|b| [b[0], b[1], b[2], b[3]]).collect();
    if rects.is_empty() {
        rects = crate::geometry::overlay_shares().to_vec();
    }
    let fixed = unsafe { std::slice::from_raw_parts(fixed, DST_W * DST_H) };
    let keep = crate::track::Mask::without(&rects);
    Box::into_raw(Box::new(crate::camera::CameraWatch::new(&crate::camera::excluded(keep.kept(), fixed))))
}

/// The frames added so far.
///
/// # Safety
/// `finder` from `areas_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn areas_frames(finder: *const crate::areas::AreaFinder) -> usize {
    unsafe { &*finder }.frames()
}

/// Whether a box ([x0, y0, x1, y1] as shares of the frame, JSON) is a crosshair zoom over the frames added: 1 or 0.
///
/// # Safety
/// `finder` from `areas_new`; `bounds` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn areas_zoomed(finder: *const crate::areas::AreaFinder, bounds: *const u8, len: usize) -> u32 {
    let b = serde_json::from_slice::<[f64; 4]>(unsafe { std::slice::from_raw_parts(bounds, len) });
    b.is_ok_and(|b| unsafe { &*finder }.zoomed(&b)) as u32
}

/// The areas to propose: {found, examples, labelled, kinds?} in, {boxes, copied, by, examples, recordings} out
/// (`areas::find_json`). Free the result as `tracker_finish`'s.
///
/// # Safety
/// `input` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn areas_find(input: *const u8, len: usize) -> *mut u8 {
    unsafe { areas_call(input, len, crate::areas::find_json) }
}

/// Learning from saved areas: {rec, found, saved, maps?, examples?, kinds?} in, {examples: the new
/// area_examples.jsonl text, added} out (`areas::learn_json`). Free the result as `tracker_finish`'s.
///
/// # Safety
/// `input` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn areas_learn(input: *const u8, len: usize) -> *mut u8 {
    unsafe { areas_call(input, len, crate::areas::learn_json) }
}

/// Found areas named: {found, examples, k?} in, [{box, feat, rule, kind, by}, ...] out (`areas::predict_json`). Free
/// the result as `tracker_finish`'s.
///
/// # Safety
/// `input` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn areas_predict(input: *const u8, len: usize) -> *mut u8 {
    unsafe { areas_call(input, len, crate::areas::predict_json) }
}

/// How alike two recordings' overlays are: {found, other} in, a number out (`areas::same_layout_json`). Free the result
/// as `tracker_finish`'s.
///
/// # Safety
/// `input` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn areas_same_layout(input: *const u8, len: usize) -> *mut u8 {
    unsafe { areas_call(input, len, crate::areas::same_layout_json) }
}

/// Leave one recording out: {examples} in, {sure, right, count, wrong: [[truth, guess, n], ...]} out
/// (`areas::check_json`). Free the result as `tracker_finish`'s.
///
/// # Safety
/// `input` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn areas_check(input: *const u8, len: usize) -> *mut u8 {
    unsafe { areas_call(input, len, crate::areas::check_json) }
}

/// The maps of `areas_finish`'s JSON (its `maps` object) as bytes: the stand-out map, then the change map, 1280 * 720
/// bytes each, row by row; no bytes when they cannot be read. Free the result as `tracker_finish`'s.
///
/// # Safety
/// `maps` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn areas_maps(maps: *const u8, len: usize) -> *mut u8 {
    let maps = serde_json::from_slice::<crate::areas::Maps>(unsafe { std::slice::from_raw_parts(maps, len) });
    bytes_out(maps.map(|maps| [maps.stand(), maps.change()].concat()).unwrap_or_default())
}

/// A tracker for a recording, as `tracker_new` makes it, that also knows which excluded areas are the challenge's end
/// screen (src/popup.rs: `END_SCREEN`): `ends` holds one byte per area, 1 for an end screen, which is excluded only
/// while it shows. cap: the scenario's target count, 0 for none.
///
/// # Safety
/// `areas` must point to `4 * areas_len` f64s and `ends` to `areas_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tracker_new_ends(
    areas: *const f64,
    ends: *const u8,
    areas_len: usize,
    cap: usize,
) -> *mut Tracker {
    let flat = unsafe { std::slice::from_raw_parts(areas, 4 * areas_len) };
    let boxes: Vec<[f64; 4]> = flat.chunks_exact(4).map(|b| [b[0], b[1], b[2], b[3]]).collect();
    let which: Vec<bool> = unsafe { std::slice::from_raw_parts(ends, areas_len) }.iter().map(|&end| end != 0).collect();
    Box::into_raw(Box::new(Tracker::new(boxes, cap).end_screens(&which)))
}
