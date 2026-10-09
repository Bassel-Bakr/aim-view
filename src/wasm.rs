//! The core's interface to the browser (WebAssembly builds only): plain exports over the module's memory, so no
//! binding generator is needed. The page reserves memory with `alloc`, fills it, calls a function with pointers, and
//! frees it with `dealloc`. ui/src/app/modes/wasm/core.ts wraps these.
//!
//! In: the page's decoded frames, the detector's maps, and JSON requests (a review's setup, a cut-off, a crop's
//! scene, the area finder's calls). Out: the objects the page drives (converters, the fixed map, the area finder, the
//! HUD watch, the review session's passes) as pointers, and their results as JSON or bytes in buffers that start with
//! their length (`bytes_out`). The exports the page stopped calling are in retired/wasm_exports.rs.

use std::alloc::{Layout, alloc as raw_alloc, dealloc as raw_dealloc};

use crate::convert::{Converter, DST_H, DST_W, Matrix};
use crate::fixed::FixedMap;
use crate::session::{Joining, Keys, KeysRead, NextFrame, Review, RunTracking, RunWatching, Setup, WatchPart};
use crate::tracker::TrackPart;

/// The layout of a buffer of `len` bytes, 8-byte aligned (at least 1 byte, as the allocator needs).
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

/// A byte buffer handed to the page: its length (u32, little-endian), then the bytes; the page frees it with
/// `dealloc(ptr, 4 + length)`.
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

/// The rows of a frame's RGB the camera watch reads (the countdown bar's), as from + (to << 16).
#[unsafe(no_mangle)]
pub extern "C" fn camera_rgb_rows() -> u32 {
    let (from, to) = crate::camera::COUNTDOWN_ROWS;
    (from | (to << 16)) as u32
}

/// Frees a converter.
///
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

/// The crops a submitted faint-target cut-off gives as detector labels: the request as JSON (src/faint.rs:
/// `CutoffRequest`), the crops as JSON (an array of `CutoffCrop`, or {error}). Free the result as `bytes_out`'s.
///
/// # Safety
/// `request` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cutoff_crops(request: *const u8, len: usize) -> *mut u8 {
    bytes_out(crate::faint::cutoff_json(unsafe { std::slice::from_raw_parts(request, len) }))
}

/// What a crop's shapes show (src/shapes.rs): the request as JSON ({scene, width, height}), the answer as JSON (a
/// `SceneView`, or {error}). Free the result as `bytes_out`'s.
///
/// # Safety
/// `request` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn shapes_visible(request: *const u8, len: usize) -> *mut u8 {
    bytes_out(crate::shapes::visible_json(unsafe { std::slice::from_raw_parts(request, len) }))
}

// ---- the area finder (src/areas.rs) ---------------------------------------------------------------------------------

/// An area finder (`areas::AreaFinder`). Give it the frames `areas_sample` picks, then `areas_finish`.
#[unsafe(no_mangle)]
pub extern "C" fn areas_new() -> *mut crate::areas::AreaFinder {
    Box::into_raw(Box::new(crate::areas::AreaFinder::new()))
}

/// One frame as YUV 4:2:0 at 1280 x 720 (`converter_yuv420p`'s output).
///
/// # Safety
/// `finder` from `areas_new`; `yuv` must hold 1280 * 720 * 3 / 2 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn areas_add(finder: *mut crate::areas::AreaFinder, yuv: *const u8) {
    unsafe { &mut *finder }.add(unsafe { std::slice::from_raw_parts(yuv, crate::areas::FRAME) });
}

/// The found areas and their maps as JSON (`areas::Found`: {frames, areas: [{box, feat, rule}, ...], maps}), and
/// frees the finder. `session`: `hud_session_box`'s JSON, or no bytes (or null) without a session box. Free the result
/// as `bytes_out`'s.
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
/// `areas_finish`. Call it after the watch's last key frame. Free the result as `bytes_out`'s.
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

/// Which frames the area finder reads: {keys, times, duration} in; null (the key frames) or [frame index, ...] out
/// (`areas::sample_json`). Free the result as `bytes_out`'s.
///
/// # Safety
/// `input` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn areas_sample(input: *const u8, len: usize) -> *mut u8 {
    unsafe { areas_call(input, len, crate::areas::sample_json) }
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
/// Returns a text as `bytes_out` hands it: empty when the file was read, else why it was not.
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

/// The review's runs as JSON (`session::Run` each). Free the result as `bytes_out`'s.
///
/// # Safety
/// `review` from `review_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn review_runs(review: *const Review) -> *mut u8 {
    bytes_out(serde_json::to_vec(unsafe { &*review }.runs()).unwrap_or_default())
}

/// Frees a review.
///
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
/// `review_watching`; frees the pass. Free the result as `bytes_out`'s.
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
/// `bytes_out`'s.
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
/// `bytes_out`'s.
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
/// detector that ran, as tracks.json names it (UTF-8). Free the result as `bytes_out`'s.
///
/// # Safety
/// `joining` from `review_joining`, not used again; `detector` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn joining_finish(joining: *mut Joining, detector: *const u8, len: usize) -> *mut u8 {
    let detector = String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(detector, len) }).into_owned();
    outcome_out(unsafe { Box::from_raw(joining) }.finish(detector))
}
