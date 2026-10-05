//! The core's interface to the browser (WebAssembly builds only): plain exports over the module's memory, so no
//! binding generator is needed. The page reserves memory with `alloc`, fills it, calls a function with pointers, and
//! frees it with `dealloc`. ui/src/app/modes/wasm/core.ts wraps these.

use std::alloc::{Layout, alloc as raw_alloc, dealloc as raw_dealloc};

use crate::convert::{Converter, DST_H, DST_W, Matrix};
use crate::fixed::FixedMap;
use crate::session::{Joining, Keys, KeysRead, NextFrame, Review, RunTracking, RunWatching, Setup, WatchPart};
use crate::track::{RawBox, TrackFrame};
use crate::tracker::{TrackPart, Tracker};

fn layout(len: usize) -> Layout {
    Layout::from_size_align(len.max(1), 8).unwrap()
}

/// Reserves `len` bytes (8-byte aligned) in the module's memory.
#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    unsafe { raw_alloc(layout(len)) }
}

/// Frees what `alloc` reserved.
///
/// # Safety
/// `ptr` and `len` must come from one `alloc` call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: usize) {
    unsafe { raw_dealloc(ptr, layout(len)) }
}

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
    let raw: Vec<RawBox> = flat
        .chunks_exact(5)
        .map(|b| RawBox { cx: b[0], cy: b[1], w: b[2], h: b[3], score: b[4] })
        .collect();
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

/// A byte buffer handed to the page: its length (u32), then the bytes.
fn bytes_out(data: Vec<u8>) -> *mut u8 {
    let ptr = alloc(4 + data.len());
    unsafe {
        std::ptr::copy_nonoverlapping((data.len() as u32).to_le_bytes().as_ptr(), ptr, 4);
        std::ptr::copy_nonoverlapping(data.as_ptr(), ptr.add(4), data.len());
    }
    ptr
}

/// A converter for a recording's frames (width x height, YUV 4:2:0). matrix: 0 BT.709, 1 BT.601 (also unspecified),
/// 2 FCC, 3 SMPTE 240M, 4 BT.2020; full: 1 for full ("pc") range.
#[unsafe(no_mangle)]
pub extern "C" fn converter_new(width: usize, height: usize, matrix: u32, full: u32) -> *mut Converter {
    Box::into_raw(Box::new(Converter::new(width, height, Matrix::from_code(matrix), full == 1)))
}

/// One frame (YUV 4:2:0 at the converter's size) as RGB24 at 1280 x 720, into `out` (1280 * 720 * 3 bytes).
///
/// # Safety
/// `converter` from `converter_new`; `yuv` must hold the frame, `out` 1280 * 720 * 3 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn converter_rgb24(converter: *mut Converter, yuv: *const u8, yuv_len: usize, out: *mut u8) {
    let (converter, yuv) = unsafe { (&mut *converter, std::slice::from_raw_parts(yuv, yuv_len)) };
    converter.rgb24(yuv, unsafe { std::slice::from_raw_parts_mut(out, DST_W * DST_H * 3) });
}

/// One frame as YUV 4:2:0 at 1280 x 720, into `out` (1280 * 720 * 3 / 2 bytes).
///
/// # Safety
/// As `converter_rgb24`, with `out` 1280 * 720 * 3 / 2 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn converter_yuv420p(converter: *mut Converter, yuv: *const u8, yuv_len: usize, out: *mut u8) {
    let (converter, yuv) = unsafe { (&mut *converter, std::slice::from_raw_parts(yuv, yuv_len)) };
    converter.yuv420p(yuv, unsafe { std::slice::from_raw_parts_mut(out, DST_W * DST_H * 3 / 2) });
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

/// The rows of a frame's RGB the camera watch reads (the countdown bar's), as from + (to << 16).
#[unsafe(no_mangle)]
pub extern "C" fn camera_rgb_rows() -> u32 {
    let (from, to) = crate::camera::COUNTDOWN_ROWS;
    (from | (to << 16)) as u32
}

/// # Safety
/// `converter` from `converter_new`, not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn converter_free(converter: *mut Converter) {
    drop(unsafe { Box::from_raw(converter) });
}

/// A fixed map, built from a recording's key frames.
#[unsafe(no_mangle)]
pub extern "C" fn fixed_new() -> *mut FixedMap {
    Box::into_raw(Box::default())
}

/// One key frame, YUV 4:2:0 at 1280 x 720.
///
/// # Safety
/// `fixed` from `fixed_new`; `yuv` must hold 1280 * 720 * 3 / 2 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fixed_add(fixed: *mut FixedMap, yuv: *const u8) {
    let fixed = unsafe { &mut *fixed };
    fixed.add(unsafe { std::slice::from_raw_parts(yuv, DST_W * DST_H * 3 / 2) });
}

/// The map (1 fixed, 0 not), into `out` (1280 * 720 bytes), and frees the builder.
///
/// # Safety
/// `fixed` from `fixed_new`, not used again; `out` must hold 1280 * 720 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fixed_finish(fixed: *mut FixedMap, out: *mut u8) {
    let fixed = unsafe { Box::from_raw(fixed) };
    unsafe { std::slice::from_raw_parts_mut(out, DST_W * DST_H) }.copy_from_slice(&fixed.map());
}

/// A scenario file's facts (its bytes, UTF-8 or UTF-16 with its mark, at least up to "[Map Data]"), as JSON: {kind,
/// limit, targets}. Free the result as `tracker_finish`'s.
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

/// A HUD watch (src/hud.rs) for a recording of `width` x `height` pixels; `full`: its Y spans 0..255.
#[unsafe(no_mangle)]
pub extern "C" fn hud_new(width: usize, height: usize, full: u32) -> *mut crate::hud::HudWatch {
    Box::into_raw(Box::new(crate::hud::HudWatch::new(width, height, full != 0)))
}

/// One key frame's Y plane (`w` x `h` bytes), before any frame.
///
/// # Safety
/// `hud` from `hud_new`; `y` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hud_add_key(hud: *mut crate::hud::HudWatch, y: *const u8, len: usize) {
    unsafe { &mut *hud }.add_key(unsafe { std::slice::from_raw_parts(y, len) });
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

/// The crops a submitted faint-target cut-off gives as detector labels: the request as JSON (src/faint.rs:
/// `CutoffRequest`), the crops as JSON (an array of `CutoffCrop`, or {error}). Free the result as `tracker_finish`'s.
///
/// # Safety
/// `request` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cutoff_crops(request: *const u8, len: usize) -> *mut u8 {
    bytes_out(crate::faint::cutoff_json(unsafe { std::slice::from_raw_parts(request, len) }))
}

/// What a crop's shapes show (src/shapes.rs): the request as JSON ({scene, width, height}), the answer as JSON (a
/// `SceneView`, or {error}). Free the result as `tracker_finish`'s.
///
/// # Safety
/// `request` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn shapes_visible(request: *const u8, len: usize) -> *mut u8 {
    bytes_out(crate::shapes::visible_json(unsafe { std::slice::from_raw_parts(request, len) }))
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

// ---- the area finder (src/areas.rs) ---------------------------------------------------------------------------------

/// An area finder (`areas::AreaFinder`). Give it the frames `areas_sample` picks, then `areas_finish`.
#[unsafe(no_mangle)]
pub extern "C" fn areas_new() -> *mut crate::areas::AreaFinder {
    Box::into_raw(Box::new(crate::areas::AreaFinder::new()))
}

/// One frame as YUV 4:2:0 at 1280 x 720 (`convert_yuv420p`'s output).
///
/// # Safety
/// `finder` from `areas_new`; `yuv` must hold 1280 * 720 * 3 / 2 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn areas_add(finder: *mut crate::areas::AreaFinder, yuv: *const u8) {
    unsafe { &mut *finder }.add(unsafe { std::slice::from_raw_parts(yuv, crate::areas::FRAME) });
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

/// The found areas and their maps as JSON (`areas::Found`: {frames, areas: [{box, feat, rule}, ...], maps}), and
/// frees the finder. `session`: `hud_session_box`'s JSON, or no bytes (or null) without a session box. Free the result
/// as `tracker_finish`'s.
///
/// # Safety
/// `finder` from `areas_new`, not used again; `session` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn areas_finish(
    finder: *mut crate::areas::AreaFinder,
    session: *const u8,
    len: usize,
) -> *mut u8 {
    let finder = unsafe { Box::from_raw(finder) };
    let session = if len == 0 {
        None
    } else {
        serde_json::from_slice(unsafe { std::slice::from_raw_parts(session, len) }).ok().flatten()
    };
    bytes_out(serde_json::to_vec(&finder.finish(session)).unwrap_or_default())
}

/// KovaaK's session box as the HUD watch finds it in its key frames (`hud::SessionRows` as JSON, or null), for
/// `areas_finish`. Call it after the watch's last key frame. Free the result as `tracker_finish`'s.
///
/// # Safety
/// `hud` from `hud_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hud_session_box(hud: *mut crate::hud::HudWatch) -> *mut u8 {
    bytes_out(serde_json::to_vec(&unsafe { &mut *hud }.session_box()).unwrap_or_default())
}

/// A JSON call: the request's text to the function, its answer (or {"error": ...}) handed to the page.
///
/// # Safety
/// `input` must hold `len` bytes.
unsafe fn areas_call(input: *const u8, len: usize, answer: fn(&str) -> Result<String, String>) -> *mut u8 {
    let bytes = unsafe { std::slice::from_raw_parts(input, len) };
    let text = std::str::from_utf8(bytes).map_err(|error| error.to_string());
    let out = text.and_then(answer).unwrap_or_else(|error| serde_json::json!({ "error": error }).to_string());
    bytes_out(out.into_bytes())
}

/// Which frames the area finder reads: {keys, times, duration} -> null (the key frames) or [frame index, ...]
/// (`areas::sample_json`). Free the result as `tracker_finish`'s.
///
/// # Safety
/// `input` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn areas_sample(input: *const u8, len: usize) -> *mut u8 {
    unsafe { areas_call(input, len, crate::areas::sample_json) }
}

/// The areas to propose: {found, examples, labelled, kinds?} -> {boxes, copied, by, examples, recordings}
/// (`areas::find_json`). Free the result as `tracker_finish`'s.
///
/// # Safety
/// `input` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn areas_find(input: *const u8, len: usize) -> *mut u8 {
    unsafe { areas_call(input, len, crate::areas::find_json) }
}

/// Learning from saved areas: {rec, found, saved, maps?, examples?, kinds?} -> {examples: the new
/// area_examples.jsonl text, added} (`areas::learn_json`). Free the result as `tracker_finish`'s.
///
/// # Safety
/// `input` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn areas_learn(input: *const u8, len: usize) -> *mut u8 {
    unsafe { areas_call(input, len, crate::areas::learn_json) }
}

/// Found areas named: {found, examples, k?} -> [{box, feat, rule, kind, by}, ...] (`areas::predict_json`). Free the
/// result as `tracker_finish`'s.
///
/// # Safety
/// `input` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn areas_predict(input: *const u8, len: usize) -> *mut u8 {
    unsafe { areas_call(input, len, crate::areas::predict_json) }
}

/// How alike two recordings' overlays are: {found, other} -> a number (`areas::same_layout_json`). Free the result as
/// `tracker_finish`'s.
///
/// # Safety
/// `input` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn areas_same_layout(input: *const u8, len: usize) -> *mut u8 {
    unsafe { areas_call(input, len, crate::areas::same_layout_json) }
}

/// Leave one recording out: {examples} -> {sure, right, count, wrong: [[truth, guess, n], ...]}
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

// ---- the review session (src/session.rs) ----------------------------------------------------------------------------

/// A part's or a review's JSON, or {"error": ...}, handed to the page.
fn outcome_out<T: serde::Serialize>(outcome: Result<T, String>) -> *mut u8 {
    let json = outcome.and_then(|value| serde_json::to_vec(&value).map_err(|error| error.to_string()));
    bytes_out(json.unwrap_or_else(|error| serde_json::json!({ "error": error }).to_string().into_bytes()))
}

/// A review from its setup (`session::Setup` as JSON); null when the setup cannot be read or the video has no frames.
///
/// # Safety
/// `setup` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn review_new(setup: *const u8, len: usize) -> *mut Review {
    let setup = serde_json::from_slice::<Setup>(unsafe { std::slice::from_raw_parts(setup, len) });
    match setup.map_err(|error| error.to_string()).and_then(Review::new) {
        Ok(review) => Box::into_raw(Box::new(review)),
        Err(_) => std::ptr::null_mut(),
    }
}

/// The detector model's settings file (detector_<name>.json, UTF-8: src/model.rs), before the review's runs start.
/// Returns a text as `tracker_finish` does: empty when the file was read, else why it was not.
///
/// # Safety
/// `review` from `review_new`; `text` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn review_set_model(review: *mut Review, text: *const u8, len: usize) -> *mut u8 {
    let text = String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(text, len) });
    match crate::model::ModelSettings::from_json(&text) {
        Ok(model) => {
            unsafe { &mut *review }.set_model(model);
            bytes_out(Vec::new())
        }
        Err(error) => bytes_out(error.into_bytes()),
    }
}

/// The review's runs as JSON (`session::Run` each). Free the result as `tracker_finish`'s.
///
/// # Safety
/// `review` from `review_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn review_runs(review: *const Review) -> *mut u8 {
    bytes_out(serde_json::to_vec(unsafe { &*review }.runs()).unwrap_or_default())
}

/// # Safety
/// `review` from `review_new`, not used again (what it made lives on).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn review_free(review: *mut Review) {
    drop(unsafe { Box::from_raw(review) });
}

/// The review's key frames' pass.
///
/// # Safety
/// `review` from `review_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn review_keys(review: *const Review) -> *mut Keys {
    Box::into_raw(Box::new(unsafe { &*review }.keys()))
}

/// One key frame: YUV 4:2:0 at 1280 x 720 (`converter_yuv420p`'s output), and its Y plane as decoded.
///
/// # Safety
/// `keys` from `review_keys`; `small` must hold 1280 * 720 * 3 / 2 bytes, `y` `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn keys_add(keys: *mut Keys, small: *const u8, y: *const u8, len: usize) {
    let small = unsafe { std::slice::from_raw_parts(small, DST_W * DST_H * 3 / 2) };
    unsafe { &mut *keys }.add(small, unsafe { std::slice::from_raw_parts(y, len) });
}

/// The fixed map into `fixed` (1280 * 720 bytes, 1 fixed), and where the HUD's boxes are as JSON (`hud::HudKeys`), for
/// `review_watching`; frees the pass. Free the result as `tracker_finish`'s.
///
/// # Safety
/// `keys` from `review_keys`, not used again; `fixed` must hold 1280 * 720 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn keys_finish(keys: *mut Keys, fixed: *mut u8) -> *mut u8 {
    let read = unsafe { Box::from_raw(keys) }.finish();
    unsafe { std::slice::from_raw_parts_mut(fixed, DST_W * DST_H) }.copy_from_slice(&read.fixed);
    bytes_out(serde_json::to_vec(&read.hud).unwrap_or_default())
}

/// Run `run`'s tracking.
///
/// # Safety
/// `review` from `review_new`; `run` one of its runs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn review_tracking(review: *const Review, run: usize) -> *mut RunTracking {
    Box::into_raw(Box::new(unsafe { &*review }.tracking(run)))
}

/// What the next decoded frame is for (`RunTracking::next_frame`): 2 the run's (track it), 1 the next run's first (only
/// the watches read it), 0 past the run (stop decoding).
///
/// # Safety
/// `tracking` from `review_tracking`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tracking_next(tracking: *mut RunTracking) -> u32 {
    match unsafe { &mut *tracking }.next_frame() {
        NextFrame::Track => 2,
        NextFrame::Watch => 1,
        NextFrame::Stop => 0,
    }
}

/// A tracked frame as RGB24 at 1280 x 720: its excluded areas are watched for pop-ups.
///
/// # Safety
/// `tracking` from `review_tracking`; `rgb` must hold 1280 * 720 * 3 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tracking_watch(tracking: *mut RunTracking, rgb: *const u8) {
    unsafe { &mut *tracking }.watch(unsafe { std::slice::from_raw_parts(rgb, DST_W * DST_H * 3) });
}

/// The detector's maps for the next tracked frame: the score map (gh x gw) and reg maps (4 x gh x gw), f32.
///
/// # Safety
/// `tracking` from `review_tracking`; `score` and `reg` must hold `gw * gh` and `4 * gw * gh` f32s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tracking_maps(
    tracking: *mut RunTracking,
    score: *const f32,
    reg: *const f32,
    gw: usize,
    gh: usize,
) {
    let score = unsafe { std::slice::from_raw_parts(score, gw * gh) };
    let reg = unsafe { std::slice::from_raw_parts(reg, 4 * gw * gh) };
    unsafe { &mut *tracking }.maps(score, reg, gw, gh);
}

/// The run's part of the tracking as JSON (`TrackPart`), or {error}; frees the tracking. Free the result as
/// `tracker_finish`'s.
///
/// # Safety
/// `tracking` from `review_tracking`, not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tracking_part(tracking: *mut RunTracking) -> *mut u8 {
    outcome_out(unsafe { Box::from_raw(tracking) }.part())
}

/// Run `run`'s watches, from what the key frames gave: the fixed map (1280 * 720 bytes) and the HUD's boxes
/// (`keys_finish`'s JSON). Null when that JSON cannot be read.
///
/// # Safety
/// `review` from `review_new`; `run` one of its runs; `fixed` must hold 1280 * 720 bytes, `hud` `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn review_watching(
    review: *const Review,
    run: usize,
    fixed: *const u8,
    hud: *const u8,
    len: usize,
) -> *mut RunWatching {
    let Ok(hud) = serde_json::from_slice(unsafe { std::slice::from_raw_parts(hud, len) }) else {
        return std::ptr::null_mut();
    };
    let fixed = unsafe { std::slice::from_raw_parts(fixed, DST_W * DST_H) }.to_vec();
    Box::into_raw(Box::new(unsafe { &*review }.watching(run, &KeysRead { fixed, hud })))
}

/// One frame the run reads: its Y plane as decoded, then its countdown rows (`camera_rgb_rows` of its 720p RGB).
///
/// # Safety
/// `watching` from `review_watching`; `frame` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn watching_frame(watching: *mut RunWatching, frame: *const u8, len: usize) {
    let watching = unsafe { &mut *watching };
    let (y, rows) = unsafe { std::slice::from_raw_parts(frame, len) }.split_at(watching.y_bytes());
    watching.frame(y, rows);
}

/// The run's part of the watches as JSON (`WatchPart`), or {error}; frees the watches. Free the result as
/// `tracker_finish`'s.
///
/// # Safety
/// `watching` from `review_watching`, not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn watching_part(watching: *mut RunWatching) -> *mut u8 {
    outcome_out(unsafe { Box::from_raw(watching) }.part())
}

/// The runs' parts joined, in order, from the key frames' fixed map (1280 * 720 bytes).
///
/// # Safety
/// `review` from `review_new`; `fixed` must hold 1280 * 720 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn review_joining(review: *const Review, fixed: *const u8) -> *mut Joining {
    let fixed = unsafe { std::slice::from_raw_parts(fixed, DST_W * DST_H) };
    Box::into_raw(Box::new(unsafe { &*review }.joining(fixed)))
}

/// The next run's parts: `tracking_part`'s and `watching_part`'s JSON. Returns 1 when both were read, else 0 (the join
/// then fails).
///
/// # Safety
/// `joining` from `review_joining`; `track` must hold `track_len` bytes, `watch` `watch_len`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn joining_add(
    joining: *mut Joining,
    track: *const u8,
    track_len: usize,
    watch: *const u8,
    watch_len: usize,
) -> u32 {
    let track = serde_json::from_slice::<TrackPart>(unsafe { std::slice::from_raw_parts(track, track_len) });
    let watch = serde_json::from_slice::<WatchPart>(unsafe { std::slice::from_raw_parts(watch, watch_len) });
    let (Ok(track), Ok(watch)) = (track, watch) else { return 0 };
    unsafe { &mut *joining }.add(track, watch);
    1
}

/// The joined review as JSON (`session::Joined`: {tracks, readings, hud}), or {error}; frees the join. `detector`: the
/// detector that ran, as tracks.json names it (UTF-8). Free the result as `tracker_finish`'s.
///
/// # Safety
/// `joining` from `review_joining`, not used again; `detector` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn joining_finish(joining: *mut Joining, detector: *const u8, len: usize) -> *mut u8 {
    let detector = String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(detector, len) }).into_owned();
    outcome_out(unsafe { Box::from_raw(joining) }.finish(detector))
}
