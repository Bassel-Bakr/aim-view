//! The core's interface to the browser (WebAssembly builds only): plain exports over the module's memory, so no
//! binding generator is needed. The page reserves memory with `alloc`, fills it, calls a function with pointers, and
//! frees it with `dealloc`. ui/src/app/modes/wasm/core.ts wraps these.

use std::alloc::{Layout, alloc as raw_alloc, dealloc as raw_dealloc};

use crate::convert::{Converter, DST_H, DST_W, Matrix};
use crate::fixed::FixedMap;
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

/// The frame the next boxes are from, as RGB24 at 1280 x 720: its excluded areas are watched for pop-ups.
///
/// # Safety
/// `tracker` from `tracker_new`; `rgb` must hold 1280 * 720 * 3 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tracker_watch(tracker: *mut Tracker, rgb: *const u8) {
    let t = unsafe { &mut *tracker };
    t.watch(unsafe { std::slice::from_raw_parts(rgb, DST_W * DST_H * 3) });
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
    let t = unsafe { &mut *tracker };
    let score = unsafe { std::slice::from_raw_parts(score, gw * gh) };
    let reg = unsafe { std::slice::from_raw_parts(reg, 4 * gw * gh) };
    t.push_maps(score, reg, gw, gh)
}

/// One frame's boxes, already decoded: `n` boxes of [cx, cy, w, h, score] (f32, frame pixels). Returns the boxes kept.
///
/// # Safety
/// `tracker` from `tracker_new`; `boxes` must hold `5 * n` f32s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tracker_push_boxes(tracker: *mut Tracker, boxes: *const f32, n: usize) -> usize {
    let t = unsafe { &mut *tracker };
    let flat = unsafe { std::slice::from_raw_parts(boxes, 5 * n) };
    let raw: Vec<RawBox> = flat
        .chunks_exact(5)
        .map(|b| RawBox { cx: b[0], cy: b[1], w: b[2], h: b[3], score: b[4] })
        .collect();
    t.push_boxes(&raw)
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
    let t = unsafe { Box::from_raw(tracker) };
    bytes_out(serde_json::to_vec(&t.part()).unwrap_or_default())
}

/// The next run's part (`tracker_part`'s JSON), after the frames the tracker has (`Tracker::add_part`). Returns the
/// frames added; none when the part cannot be read.
///
/// # Safety
/// `tracker` from `tracker_new`; `part` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tracker_add_part(tracker: *mut Tracker, part: *const u8, len: usize) -> usize {
    let t = unsafe { &mut *tracker };
    match serde_json::from_slice::<TrackPart>(unsafe { std::slice::from_raw_parts(part, len) }) {
        Ok(part) => t.add_part(part),
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
    let t = unsafe { Box::from_raw(tracker) };
    bytes_out(serde_json::to_vec(&t.finish()).unwrap_or_default())
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

/// A converter for a recording's frames (w x h, YUV 4:2:0). matrix: 0 BT.709, 1 BT.601 (also unspecified),
/// 2 FCC, 3 SMPTE 240M, 4 BT.2020; full: 1 for full ("pc") range.
#[unsafe(no_mangle)]
pub extern "C" fn converter_new(w: usize, h: usize, matrix: u32, full: u32) -> *mut Converter {
    let matrix = match matrix {
        0 => Matrix::Bt709,
        2 => Matrix::Fcc,
        3 => Matrix::Smpte240m,
        4 => Matrix::Bt2020,
        _ => Matrix::Bt601,
    };
    Box::into_raw(Box::new(Converter::new(w, h, matrix, full == 1)))
}

/// One frame (YUV 4:2:0 at the converter's size) as RGB24 at 1280 x 720, into `out` (1280 * 720 * 3 bytes).
///
/// # Safety
/// `c` from `converter_new`; `yuv` must hold the frame, `out` 1280 * 720 * 3 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn converter_rgb24(c: *mut Converter, yuv: *const u8, yuv_len: usize, out: *mut u8) {
    let (c, yuv) = unsafe { (&mut *c, std::slice::from_raw_parts(yuv, yuv_len)) };
    c.rgb24(yuv, unsafe { std::slice::from_raw_parts_mut(out, DST_W * DST_H * 3) });
}

/// One frame as YUV 4:2:0 at 1280 x 720, into `out` (1280 * 720 * 3 / 2 bytes).
///
/// # Safety
/// As `converter_rgb24`, with `out` 1280 * 720 * 3 / 2 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn converter_yuv420p(c: *mut Converter, yuv: *const u8, yuv_len: usize, out: *mut u8) {
    let (c, yuv) = unsafe { (&mut *c, std::slice::from_raw_parts(yuv, yuv_len)) };
    c.yuv420p(yuv, unsafe { std::slice::from_raw_parts_mut(out, DST_W * DST_H * 3 / 2) });
}

/// The frame's luma at 1280 x 720 from its Y plane alone (`y`: the source's w x h bytes), into `out` (1280 x 720
/// bytes): the bytes `converter_yuv420p` gives for Y.
///
/// # Safety
/// `c` from `converter_new`; `y` must hold `y_len` bytes, `out` 1280 x 720.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn converter_luma(c: *mut Converter, y: *const u8, y_len: usize, out: *mut u8) {
    let (c, y) = unsafe { (&mut *c, std::slice::from_raw_parts(y, y_len)) };
    c.luma(y, unsafe { std::slice::from_raw_parts_mut(out, DST_W * DST_H) });
}

/// The rows of a frame's RGB the camera watch reads (the countdown bar's), as from + (to << 16).
#[unsafe(no_mangle)]
pub extern "C" fn camera_rgb_rows() -> u32 {
    let (from, to) = crate::camera::COUNTDOWN_ROWS;
    (from | (to << 16)) as u32
}

/// # Safety
/// `c` from `converter_new`, not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn converter_free(c: *mut Converter) {
    drop(unsafe { Box::from_raw(c) });
}

/// A fixed map, built from a recording's key frames.
#[unsafe(no_mangle)]
pub extern "C" fn fixed_new() -> *mut FixedMap {
    Box::into_raw(Box::default())
}

/// One key frame, YUV 4:2:0 at 1280 x 720.
///
/// # Safety
/// `f` from `fixed_new`; `yuv` must hold 1280 * 720 * 3 / 2 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fixed_add(f: *mut FixedMap, yuv: *const u8) {
    let f = unsafe { &mut *f };
    f.add(unsafe { std::slice::from_raw_parts(yuv, DST_W * DST_H * 3 / 2) });
}

/// The map (1 fixed, 0 not), into `out` (1280 * 720 bytes), and frees the builder.
///
/// # Safety
/// `f` from `fixed_new`, not used again; `out` must hold 1280 * 720 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fixed_finish(f: *mut FixedMap, out: *mut u8) {
    let f = unsafe { Box::from_raw(f) };
    unsafe { std::slice::from_raw_parts_mut(out, DST_W * DST_H) }.copy_from_slice(&f.map());
}

/// A scenario file's facts (its text, UTF-8, at least up to "[Map Data]"), as JSON: {kind, limit, targets}. Free the
/// result as `tracker_finish`'s.
///
/// # Safety
/// `text` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scenario_facts(text: *const u8, len: usize) -> *mut u8 {
    let bytes = unsafe { std::slice::from_raw_parts(text, len) };
    let facts = crate::scenario::facts(&String::from_utf8_lossy(bytes));
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
/// `c` from `camera_new`; `yuv` and `rgb` must hold one frame each.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn camera_add(c: *mut crate::camera::CameraWatch, yuv: *const u8, rgb: *const u8) {
    let c = unsafe { &mut *c };
    let gray = unsafe { std::slice::from_raw_parts(yuv, DST_W * DST_H) };
    c.add(gray, unsafe { std::slice::from_raw_parts(rgb, DST_W * DST_H * 3) });
}

/// The run's part of the watch as JSON (src/camera.rs: `CameraPart`), and frees the watch. Free the result as
/// `tracker_finish`'s.
///
/// # Safety
/// `c` from `camera_new`, not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn camera_part(c: *mut crate::camera::CameraWatch) -> *mut u8 {
    let c = unsafe { Box::from_raw(c) };
    bytes_out(serde_json::to_vec(&c.part()).unwrap_or_default())
}

/// The next run's part (`camera_part`'s JSON), after the frames the watch has (`CameraWatch::join`). Returns the
/// watch's frames after it; none when the part cannot be read.
///
/// # Safety
/// `c` from `camera_new`; `part` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn camera_add_part(c: *mut crate::camera::CameraWatch, part: *const u8, len: usize) -> usize {
    let c = unsafe { &mut *c };
    let slice = unsafe { std::slice::from_raw_parts(part, len) };
    let Ok(part) = serde_json::from_slice::<crate::camera::CameraPart>(slice) else {
        return 0;
    };
    c.join(part);
    c.countdown.len()
}

/// The readings, the tracks known (`frames`: the JSON `tracker_finish` gave), as JSON {camera, countdown}, and frees the
/// watch. Free the result as `tracker_finish`'s.
///
/// # Safety
/// `c` from `camera_new`, not used again; `frames` must hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn camera_finish(c: *mut crate::camera::CameraWatch, frames: *const u8, len: usize) -> *mut u8 {
    let c = unsafe { Box::from_raw(c) };
    let frames: Vec<TrackFrame> =
        serde_json::from_slice(unsafe { std::slice::from_raw_parts(frames, len) }).unwrap_or_default();
    bytes_out(serde_json::to_vec(&c.finish(&frames)).unwrap_or_default())
}
