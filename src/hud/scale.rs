//! The HUD's scaling (src/hud/mod.rs): a frame's region resampled as ffmpeg's `scale` filter does (area, bilinear
//! and bicubic taps along each axis), the glyphs cut from it made GLYPH_WIDTH_PX x GLYPH_HEIGHT_PX, and a
//! limited-range Y stretched to 0..255.
//!
//! In: the Y plane's rows of the boxes the watch reads (watch.rs) and the glyphs the layout cuts (layout.rs). Out:
//! the scaled regions and glyph images; the area finder (src/areas.rs) shares the taps.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use super::{GLYPH_HEIGHT_PX, GLYPH_PIXELS, GLYPH_WIDTH_PX};

/// Pillow's bilinear and bicubic filters reach this many source pixels either side.
const BILINEAR_SUPPORT: f64 = 1.0;
/// Pillow's bicubic filter's reach either side, source pixels.
const BICUBIC_SUPPORT: f64 = 2.0;
/// Pillow's bicubic filter's `a`.
const BICUBIC_A: f64 = -0.5;
/// A limited-range recording's Y levels: black at LIMITED_BLACK, white LIMITED_SPAN above it.
const LIMITED_BLACK: f64 = 16.0;
/// The levels from black to white in a limited-range recording (16 to 235).
const LIMITED_SPAN: f64 = 219.0;

/// How a crop of the frame is scaled, as python/hud.py had ffmpeg scale it: `area` for KovaaK's box, bicubic for Aim
/// Lab's line.
#[derive(Clone, Copy)]
pub(super) enum Filter {
    /// ffmpeg's `area`: the mean of the pixels covered when scaling down, bilinear when scaling up.
    Area,
    /// Bicubic, widened when scaling down (Pillow's).
    Cubic,
}

/// Pillow's bilinear (triangle) filter: the weight of a source pixel `x` pixels from the output pixel's center.
pub(crate) fn bilinear(x: f64) -> f64 {
    let x = x.abs();
    if x < 1.0 { 1.0 - x } else { 0.0 }
}

/// Pillow's bicubic filter (`a` = BICUBIC_A): the weight of a source pixel `x` pixels from the output pixel's center.
fn bicubic(x: f64) -> f64 {
    let x = x.abs();
    if x < 1.0 {
        ((BICUBIC_A + 2.0) * x - (BICUBIC_A + 3.0)) * x * x + 1.0
    } else if x < 2.0 {
        (((x - 5.0) * x + 8.0) * x - 4.0) * BICUBIC_A
    } else {
        0.0
    }
}

/// Pillow's resampling weights (precompute_coeffs) for `len` pixels scaled to `out`: each output pixel's first source
/// pixel and weights.
pub(crate) fn taps(len: usize, out: usize, support: f64, filter: fn(f64) -> f64) -> Vec<(usize, Vec<f64>)> {
    let scale = len as f64 / out as f64;
    let filter_scale = scale.max(1.0);
    let (support, inverse_scale) = (support * filter_scale, 1.0 / filter_scale);
    (0..out)
        .map(|i| {
            let center = (i as f64 + 0.5) * scale;
            let first = ((center - support + 0.5) as i64).max(0) as usize;
            let end = ((center + support + 0.5) as i64).min(len as i64).max(first as i64) as usize;
            let mut weights: Vec<f64> =
                (first..end).map(|x| filter((x as f64 - center + 0.5) * inverse_scale)).collect();
            let sum: f64 = weights.iter().sum();
            if sum != 0.0 {
                weights.iter_mut().for_each(|weight| *weight /= sum);
            }
            (first, weights)
        })
        .collect()
}

/// One axis of a crop of the frame scaled to a size: each output pixel's first source pixel and weights.
pub(super) struct Axis {
    /// Each output pixel's first source pixel (in the frame) and its weights.
    pub(super) taps: Box<[(usize, Box<[f32]>)]>,
    /// Whether the crop is already the output's size, so each output pixel is one source pixel.
    identity: bool,
}

impl Axis {
    /// An axis of `size` pixels cropped from share `from` to share `to` as ffmpeg's crop does (the size rounded, then
    /// both made even for the chroma), scaled to `out` pixels.
    fn new(size: usize, from: f64, to: f64, out: usize, filter: Filter) -> Axis {
        let start = (((size as f64 * from) as usize) & !1).min(size.saturating_sub(1));
        let len = (((size as f64 * (to - from)).round_ties_even() as usize) & !1).clamp(1, (size - start).max(1));
        let step = len as f64 / out as f64;
        let mut cubic = match filter {
            Filter::Cubic if len != out => taps(len, out, BICUBIC_SUPPORT, bicubic),
            _ => Vec::new(),
        };
        let taps = (0..out)
            .map(|i| {
                let (first, weights): (usize, Vec<f64>) = match filter {
                    _ if len == out => (i, vec![1.0]),
                    Filter::Area => area_tap(i, len, out, step),
                    Filter::Cubic => std::mem::take(&mut cubic[i]),
                };
                (start + first, weights.into_iter().map(|weight| weight as f32).collect())
            })
            .collect();
        Axis { taps, identity: len == out }
    }
}

/// ffmpeg's `area` weights of output pixel `i` when `len` source pixels are scaled to `out`, `step` source pixels
/// each: its first source pixel and weights.
fn area_tap(i: usize, len: usize, out: usize, step: f64) -> (usize, Vec<f64>) {
    if len > out {
        // scaling down: the share of each source pixel the output pixel covers
        let (left, right) = (i as f64 * step, (i + 1) as f64 * step);
        let first = left.floor() as usize;
        let last = (right.ceil() as usize).min(len);
        (first, (first..last).map(|pixel| (right.min(pixel as f64 + 1.0) - left.max(pixel as f64)) / step).collect())
    } else if len == 1 {
        (0, vec![1.0])
    } else {
        // scaling up: bilinear between the two nearest source pixels
        let center = ((i as f64 + 0.5) * step - 0.5).clamp(0.0, (len - 1) as f64);
        let pixel = (center.floor() as usize).min(len - 2);
        (pixel, vec![1.0 - (center - pixel as f64), center - pixel as f64])
    }
}

/// A crop of the frame scaled to a size.
pub(super) struct Scale {
    /// The columns' taps.
    x: Axis,
    /// The rows' taps.
    pub(super) y: Axis,
    /// The bytes in a row of the frame's plane (its width).
    stride: usize,
}

impl Scale {
    /// The crop of a `width` x `height` frame between the shares `share` (x0, y0, x1, y1), scaled to `out` (width,
    /// height) with `filter`.
    pub(super) fn new(width: usize, height: usize, share: [f64; 4], out: (usize, usize), filter: Filter) -> Scale {
        Scale {
            x: Axis::new(width, share[0], share[2], out.0, filter),
            y: Axis::new(height, share[1], share[3], out.1, filter),
            stride: width,
        }
    }

    /// The scaled crop's pixels in `rows` x `columns` (row by row), each source byte through `levels`.
    pub(super) fn rect(
        &self,
        plane: &[u8],
        levels: &[u8; 256],
        rows: Range<usize>,
        columns: Range<usize>,
    ) -> Box<[u8]> {
        let mut out = Vec::with_capacity(rows.len() * columns.len());
        if self.x.identity && self.y.identity {
            let first_x = self.x.taps[columns.start].0;
            for out_y in rows {
                let at = self.y.taps[out_y].0 * self.stride + first_x;
                out.extend(plane[at..at + columns.len()].iter().map(|&level| levels[level as usize]));
            }
            return out.into_boxed_slice();
        }
        let first_x = columns.clone().map(|x| self.x.taps[x].0).min().unwrap_or(0);
        let end_x = columns.clone().map(|x| self.x.taps[x].0 + self.x.taps[x].1.len()).max().unwrap_or(first_x);
        let mut row_sums = vec![0f32; end_x - first_x];
        for out_y in rows {
            row_sums.fill(0.0);
            let (first_y, weights) = &self.y.taps[out_y];
            for (tap, &weight) in weights.iter().enumerate() {
                let at = (first_y + tap) * self.stride;
                for (sum, &level) in row_sums.iter_mut().zip(&plane[at + first_x..at + end_x]) {
                    *sum += weight * levels[level as usize] as f32;
                }
            }
            for out_x in columns.clone() {
                let (first, weights) = &self.x.taps[out_x];
                let value: f32 = weights.iter().zip(&row_sums[first - first_x..]).map(|(&a, &b)| a * b).sum();
                out.push(value.round().clamp(0.0, 255.0) as u8);
            }
        }
        out.into_boxed_slice()
    }
}

/// A glyph's strength image (`width` x `height`) scaled to GLYPH_WIDTH_PX x GLYPH_HEIGHT_PX as Pillow's bilinear
/// resize does, as bytes (0 to 255).
pub(super) fn glyph_image(strength: &[f32], width: usize, height: usize) -> [u8; GLYPH_PIXELS] {
    let (x_taps, y_taps) = (glyph_taps(width, GLYPH_WIDTH_PX), glyph_taps(height, GLYPH_HEIGHT_PX));
    let mut columns_scaled = vec![0f32; height * GLYPH_WIDTH_PX];
    for y in 0..height {
        for (x, (first, weights)) in x_taps.iter().enumerate() {
            let row = &strength[y * width + first..];
            columns_scaled[y * GLYPH_WIDTH_PX + x] =
                weights.iter().zip(row).map(|(&a, &b)| a * b as f64).sum::<f64>() as f32;
        }
    }
    let mut out = [0u8; GLYPH_PIXELS];
    for (y, (first, weights)) in y_taps.iter().enumerate() {
        for x in 0..GLYPH_WIDTH_PX {
            let source = |tap: usize| columns_scaled[(first + tap) * GLYPH_WIDTH_PX + x] as f64;
            let value: f64 = weights.iter().enumerate().map(|(tap, &weight)| weight * source(tap)).sum();
            out[y * GLYPH_WIDTH_PX + x] = (value as f32 * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

/// Pillow's resampling weights for each output pixel: its first source pixel and weights (`taps`).
type Taps = Vec<(usize, Vec<f64>)>;

/// Pillow's bilinear taps scaling `len` pixels to `out`, made once a size on each thread: a HUD's glyphs come in few
/// sizes, and made for each glyph they were 1.8 million small allocations in a review of av1's 0:20 to 0:40.
fn glyph_taps(len: usize, out: usize) -> Rc<Taps> {
    thread_local! {
        static MADE: RefCell<HashMap<(usize, usize), Rc<Taps>>> = RefCell::new(HashMap::new());
    }
    MADE.with(|made| {
        let made = &mut *made.borrow_mut();
        Rc::clone(made.entry((len, out)).or_insert_with(|| Rc::new(taps(len, out, BILINEAR_SUPPORT, bilinear))))
    })
}

/// The Y levels as read: a limited-range recording's stretched to 0..255, as ffmpeg's grey conversion does.
pub(super) fn read_levels(full_range: bool) -> [u8; 256] {
    std::array::from_fn(|level| {
        if full_range {
            level as u8
        } else {
            ((level as f64 - LIMITED_BLACK) * 255.0 / LIMITED_SPAN).round().clamp(0.0, 255.0) as u8
        }
    })
}
