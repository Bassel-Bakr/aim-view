//! The core's interface to the browser (WebAssembly builds only): plain exports over the module's memory, so no
//! binding generator is needed. The page reserves memory with `alloc`, fills it, calls a function with pointers, and
//! frees it with `dealloc`. ui/src/app/modes/wasm/core.ts wraps these.

use std::alloc::{Layout, alloc as raw_alloc, dealloc as raw_dealloc};

use crate::convert::{Converter, DST_H, DST_W, Matrix};
use crate::detect;
use crate::fixed::FixedMap;
use crate::popup::AreaWatch;
use crate::track::{Mask, RawBox, Spot, TrackFrame, keep, link, reopen};

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

/// The track step for one recording: each frame's boxes kept or dropped (raw boxes kept too), its excluded areas
/// watched for pop-ups, then the frames where a pop-up is off kept again, and all linked when the frames are in.
pub struct Tracker {
    areas: Vec<[f64; 4]>,
    mask: Mask,
    cap: Option<usize>,
    watch: AreaWatch,
    raw: Vec<Vec<RawBox>>,
    frames: Vec<Vec<Spot>>,
}

impl Tracker {
    fn new(areas: Vec<[f64; 4]>, cap: usize) -> Tracker {
        Tracker {
            mask: Mask::without(&areas),
            watch: AreaWatch::new(&areas),
            areas,
            cap: (cap > 0).then_some(cap),
            raw: Vec::new(),
            frames: Vec::new(),
        }
    }
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
    Box::into_raw(Box::new(Tracker::new(crate::geometry::overlay_shares(), cap)))
}

/// The frame the next boxes are from, as RGB24 at 1280 x 720: its excluded areas are watched for pop-ups.
///
/// # Safety
/// `tracker` from `tracker_new`; `rgb` must hold 1280 * 720 * 3 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tracker_watch(tracker: *mut Tracker, rgb: *const u8) {
    let t = unsafe { &mut *tracker };
    t.watch.add(unsafe { std::slice::from_raw_parts(rgb, DST_W * DST_H * 3) });
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
    push(t, &detect::decode(score, reg, gw, gh, detect::THRESHOLD))
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
    push(t, &raw)
}

fn push(t: &mut Tracker, raw: &[RawBox]) -> usize {
    let kept = keep(raw, &t.mask, t.cap);
    let n = kept.len();
    t.frames.push(kept);
    t.raw.push(raw.to_vec());
    n
}

/// Links the frames and frees the tracker. Returns the tracks as JSON (tracks.json's `frames`), in a buffer that
/// starts with its length (u32, little-endian); free it with `dealloc(ptr, 4 + length)`.
///
/// # Safety
/// `tracker` from `tracker_new`, not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tracker_finish(tracker: *mut Tracker) -> *mut u8 {
    let mut t = unsafe { Box::from_raw(tracker) };
    let shows = t.watch.showing();
    reopen(&t.raw, &mut t.frames, &t.areas, &shows, t.cap);
    let frames: Vec<TrackFrame> = link(&t.frames);
    bytes(serde_json::to_vec(&frames).unwrap_or_default())
}

/// A byte buffer handed to the page: its length (u32), then the bytes.
fn bytes(data: Vec<u8>) -> *mut u8 {
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
