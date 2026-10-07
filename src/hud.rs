//! A recording's on-screen HUD, read frame by frame for a run without a stats file (python/hud.py's algorithm):
//! KovaaK's session box (Kill Count, and Accuracy as hits/shots) or, when there is none, Aim Lab's POINTS and TIME
//! boxes. It gives the kill frames, the shots and hits, and the run's totals. The digits are learned from the recording
//! itself (no font): KovaaK's Kill Count counts up one kill at a time, Aim Lab's timer down one second at a time.
//!
//! In: every key frame's Y plane, then every frame's, from the review (src/session.rs: `Keys` reads the key frames,
//! each run part's `RunWatching` its frames, beside the camera watch). Out: each run part's `HudPart`, joined into one
//! watch (`Joining`), whose `HudReading` gives src/review.rs the kill times of a run without a stats file; and
//! KovaaK's box (`SessionRows`) for the area finder (src/areas.rs).
//!
//! It reads each frame's Y plane (brightness) as the decoder gives it, at the recording's size, never ffmpeg's grey
//! conversion: only the digits read must agree, not the pixels. A limited-range ("tv") recording's Y is stretched to
//! 0..255 first, as ffmpeg's grey conversion does. Each frame keeps only its value rows' glyphs (grey images of
//! GLYPH_WIDTH_PX x GLYPH_HEIGHT_PX), and a row's glyphs are kept once while they stay the same from frame to frame,
//! so a long recording stays small.
//!
//! KovaaK's stats files are the reference, not python/hud.py: where it misreads and the stats files show it, the
//! reading here departs from it, and each place says so.

use std::cell::RefCell;
use std::collections::HashMap;
use std::iter::repeat_n;
use std::ops::Range;
use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::capped::Capped;
use crate::statistics::median;

/// Where KovaaK's box can be, as shares of the frame (x0, y0, x1, y1): it grows to fit its widest number. The region
/// is scaled to BW x BH pixels (a 2560 x 1440 frame's).
pub(crate) const BOX: [f64; 4] = [0.0, 0.0, 900.0 / 2560.0, 330.0 / 1440.0];
/// The scaled region's width in pixels (the box region's width in a 2560 x 1440 frame).
pub(crate) const BW: usize = 900;
/// The scaled region's height in pixels.
pub(crate) const BH: usize = 330;
/// The most key frames kept for the box's layout: past it every other one is dropped (a median needs no more). The
/// layout needs at least MIN_KEY_FRAMES (python/hud.py's layout).
const MAX_KEY_FRAMES: usize = 64;
/// The fewest key frames the box's layout is worked out from; with fewer there is no box.
const MIN_KEY_FRAMES: usize = 3;
/// The first patch tried for the box's level, its top left corner (x, y) in the region: inside the box's left edge,
/// between the header and Kill Count.
const FIRST_PATCH: (usize, usize) = (49, 100);
/// Other players' boxes are smaller or placed elsewhere: then the patches on a grid are tried, their corners
/// PATCH_STEP_PX apart over these columns and rows of the region.
const PATCH_COLUMNS: Range<usize> = 20..320;
/// The rows of the region the grid's patch corners cover.
const PATCH_ROWS: Range<usize> = 40..240;
/// The grid's step between patch corners, pixels.
const PATCH_STEP_PX: usize = 12;
/// A patch is PATCH_SIDE_PX pixels square.
const PATCH_SIDE_PX: usize = 8;
/// The pixels in a patch.
const PATCH_PIXELS: usize = PATCH_SIDE_PX * PATCH_SIDE_PX;
/// A pixel is at a patch's level when it is less than SAME_LEVEL grey levels from it; a patch has a level when at least
/// MIN_PATCH_SHARE of its pixels are at it.
const SAME_LEVEL: f64 = 15.0;
/// The share of a patch's pixels at its level for the patch to have one (to lie on a flat area such as the box).
const MIN_PATCH_SHARE: f64 = 0.9;
/// The box is at least these shares of the region high and wide, ends more than REGION_MARGIN_PX before the region's
/// bottom and right edges (else it is the open scene running off the region), and fills at least MIN_BOX_FILL of its
/// bounds (a filled rectangle with text holes).
const MIN_BOX_HEIGHT_SHARE: f64 = 0.3;
/// The least width of the box, as a share of the region's.
const MIN_BOX_WIDTH_SHARE: f64 = 0.2;
/// How far before the region's bottom and right edges the box must end, pixels.
const REGION_MARGIN_PX: usize = 2;
/// The least share of its bounds the box's pixels fill.
const MIN_BOX_FILL: f64 = 0.6;
/// The box's text is read this far inside its edges, off its rounded corners.
const BOX_INSET_PX: usize = 4;
/// A text row is at least MIN_TEXT_ROW_PX rows of pixels, each with more than TEXT_ROW_INK_PX ink pixels.
const MIN_TEXT_ROW_PX: usize = 6;
/// The ink pixels a row of pixels needs, more than this, to be part of a text row.
const TEXT_ROW_INK_PX: usize = 2;
/// The box has a header and at least three rows under it; the compact HUD has a header and four.
const MIN_TEXT_ROWS: usize = 4;
/// The text rows of the compact HUD: a header and four rows in two columns.
const COMPACT_TEXT_ROWS: usize = 5;
/// The box's rows read above and below each value row.
const ROW_PAD_PX: usize = 3;
/// The colon after a label is at most COLON_MAX_WIDTH_PX wide, or COLON_MAX_WIDTH_SHARE of the row's height when that
/// is more, and each of its dots is at most DOT_MAX_HEIGHT_SHARE of the row's height.
const COLON_MAX_WIDTH_PX: f64 = 3.0;
/// The colon's widest, as a share of the row's height (for large text).
const COLON_MAX_WIDTH_SHARE: f64 = 0.3;
/// The tallest a colon's dot is, as a share of the row's height.
const DOT_MAX_HEIGHT_SHARE: f64 = 0.35;

/// Ink (text) is farther from the background (its median level) than MIN_INK_LEVELS grey levels, or than
/// INK_SHARE_OF_TOP of the TOP_PERCENTILE-th percentile distance when that is more.
const MIN_INK_LEVELS: f64 = 25.0;
/// The share of the top distance from the background past which a pixel is ink.
const INK_SHARE_OF_TOP: f64 = 0.5;
/// The percentile of the distances from the background taken as the text's full strength (near the top, so a few
/// stray pixels do not set it).
const TOP_PERCENTILE: f64 = 99.5;
/// Distances between grey levels are counted doubled, so the median of an even count stays a whole number; this is the
/// largest.
const MAX_TWICE_DISTANCE: usize = 2 * 255;
/// The scene past the box's right edge starts past this share of the band (the value is right of its label).
const MIN_EDGE_SHARE: f64 = 0.2;
/// A column that is ink on more than this share of its rows is the scene, not text.
const SCENE_COLUMN_INK_SHARE: f64 = 0.85;
/// The scene seen past the box's edge is at least MIN_SCENE_COLUMNS wide, and text keeps more than
/// TEXT_MARGIN_COLUMNS inside the edge: ink closer to it is the edge's blended border.
const MIN_SCENE_COLUMNS: usize = 2;
/// The columns inside the box's edge that text keeps clear of: ink this close to the edge is its blended border.
const TEXT_MARGIN_COLUMNS: usize = 2;
/// A gap wider than this share of the band parts the value from its label (or from the next label).
const LABEL_GAP_SHARE: f64 = 0.07;
/// Small, blurred text joins neighboring digits. A digit is at most 0.9 times as wide as it is tall, so a glyph at
/// least SPLIT_MIN_ASPECT times as wide as it is tall is split, one piece per DIGIT_ASPECT of its height, each cut at
/// the thinnest column within CUT_SEARCH_SHARE of a piece of the even cut.
const SPLIT_MIN_ASPECT: f64 = 1.0;
/// A digit's width as a share of its height, for counting the digits in a joined glyph.
const DIGIT_ASPECT: f64 = 0.6;
/// How far from an even cut the thinnest column is looked for, as a share of a piece's width.
const CUT_SEARCH_SHARE: f64 = 0.25;
/// Glyphs are compared at GLYPH_WIDTH_PX x GLYPH_HEIGHT_PX.
const GLYPH_WIDTH_PX: usize = 16;
/// The height glyphs are scaled to for comparing, pixels.
const GLYPH_HEIGHT_PX: usize = 24;
/// The pixels in a scaled glyph image.
const GLYPH_PIXELS: usize = GLYPH_WIDTH_PX * GLYPH_HEIGHT_PX;
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

/// Aim Lab's POINTS and TIME value line, as shares of a 16:9 frame, scaled to AW x AH pixels (twice 720p), and the two
/// values' columns in it.
pub(crate) const AIM_BAND: [f64; 4] = [0.30, 40.0 / 720.0, 0.565, 63.0 / 720.0];
/// The scaled value line's width in pixels.
pub(crate) const AW: usize = 678;
/// The scaled value line's height in pixels.
const AH: usize = 46;
/// The POINTS value's columns in the scaled line (from, to).
pub(crate) const AIM_POINTS: (usize, usize) = (26, 356);
/// The TIME value's columns in the scaled line (from, to).
pub(crate) const AIM_TIME: (usize, usize) = (368, 656);
/// A box shows a value when its TOP_PERCENTILE-th percentile level is at least this many grey levels above its
/// background (the value is white).
const AIM_MIN_TOP_LEVELS: f64 = 40.0;
/// A glyph of two dots or more, at most this many times as wide as it is tall, is the colon.
const COLON_MAX_ASPECT: f64 = 0.5;

/// A row's glyphs are the ones before while every glyph has the same ink size and differs from the kept image by at
/// most NEAR_MAX at any pixel and NEAR_SUM in all (the box is see-through: the scene behind it moves the grey levels).
const NEAR_MAX: u8 = 24;
/// The most two images of the same glyph differ by over all their pixels (3 levels a pixel on average).
const NEAR_SUM: u32 = 3 * GLYPH_PIXELS as u32;
/// The glyphs a row keeps for new lines to reuse.
const RECENT_GLYPHS: usize = 32;
/// The rows each frame keeps: KovaaK's Kill Count and Accuracy, Aim Lab's POINTS and TIME. This one: Kill Count.
const KILL_COUNT_ROW: usize = 0;
/// KovaaK's Accuracy row: hits/shots (percent).
const ACCURACY_ROW: usize = 1;
/// Aim Lab's POINTS row.
const POINTS_ROW: usize = 2;
/// Aim Lab's TIME row.
const TIME_ROW: usize = 3;
/// How many rows each frame keeps.
const ROWS: usize = 4;

/// Glyphs this alike (cosine of their grey images) are the same shape.
const SAME_SHAPE: f64 = 0.97;
/// A shape is the mean of its first SHAPE_MEAN_GLYPHS glyphs.
const SHAPE_MEAN_GLYPHS: u32 = 50;
/// The least length an image is divided by (an empty image's is 0).
const MIN_NORM: f64 = 1e-6;
/// A glyph's shape when it is like none and new shapes are not learned.
const NO_SHAPE: i32 = -1;
/// A glyph at least this share of its band high is tall: a digit, or the Accuracy line's "/" and "(".
const TALL_SHARE: f64 = 0.5;
/// A reading is stable when it stays the same for at least this many frames.
const STABLE_FRAMES: usize = 3;
/// A shape left over when the digits are learned is the digit it is at least this alike to (cosine), and a POINTS
/// glyph, or an Accuracy line's hits or shots glyph, less alike than this to every digit spoils its number.
const DIGIT_LIKENESS: f64 = 0.9;
/// The Kill Count's likenesses tried, each with the share of its steps that must be +1. A very blurred upload can split
/// one digit into two shapes at the usual likeness, and the digits are not learned; then looser likenesses are tried,
/// trusting only a Kill Count that counts up by one at almost every step.
const KILL_COUNT_TRIES: [(f64, f64); 5] = [(SAME_SHAPE, 0.8), (0.96, 0.95), (0.95, 0.95), (0.94, 0.95), (0.93, 0.95)];
/// A Kill Count is read from at least this many steps.
const MIN_KILL_COUNT_STEPS: usize = 3;
/// A step of more kills than this is a misread.
const MAX_KILL_STEP: i64 = 3;
/// A lone misread lasts at most this many frames: a real restart's 0 stays up for a second or more.
const MAX_MISREAD_FRAMES: usize = 10;
/// A step of more hits or shots than this is a misread.
const MAX_SHOT_STEP: i64 = 50;
/// The Accuracy line's marks ("/" and "(") are the mean of their first MARK_MEAN_LINES lines that read as digits, and
/// need MIN_MARK_LINES of them.
const MARK_MEAN_LINES: u32 = 50;
/// The fewest lines read as digits that the marks are learned from; with fewer, the Kill Count's likeness reads them.
const MIN_MARK_LINES: u32 = 3;
/// "--/-- ( %)" (no shot yet) has at most this many tall glyphs.
const NO_SHOT_TALL_GLYPHS: usize = 4;
/// Aim Lab's TIME likenesses tried.
const TIME_TRIES: [f64; 3] = [SAME_SHAPE, 0.96, 0.95];
/// Aim Lab's TIME line has this many glyphs, the colon left out.
const TIME_GLYPHS: usize = 4;
/// The TIME box counts down one second at a time on at least TIME_STEP_SHARE of at least MIN_TIME_STEPS steps.
const MIN_TIME_STEPS: usize = 10;
/// The least share of the TIME box's steps that must be one second down.
const TIME_STEP_SHARE: f64 = 0.95;
/// A POINTS glyph more than this many times as wide as it is tall, before any digit, is a minus sign.
const MINUS_MIN_ASPECT: f64 = 1.2;
/// One POINTS step is up to this many hits and as many misses (two hits, or a hit and a miss, in one step).
const MAX_STEP_EVENTS: i64 = 3;
/// A POINTS step is its hits and misses when it is within STEP_TOLERANCE_POINTS of them, or STEP_TOLERANCE_SHARE of a
/// hit's points when that is more.
const STEP_TOLERANCE_POINTS: f64 = 1.0;
/// A POINTS step's tolerance as a share of a hit's points, when that is more than STEP_TOLERANCE_POINTS.
const STEP_TOLERANCE_SHARE: f64 = 0.15;
/// Aim Lab's HUD is read when it gives at least MIN_AIM_HITS hits and explains at least MIN_AIM_CHECKED of the steps.
const MIN_AIM_HITS: usize = 10;
/// The least share of the POINTS steps that must be some hits and misses for Aim Lab's HUD to count.
const MIN_AIM_CHECKED: f64 = 0.85;

/// Which game's HUD was read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub enum HudGame {
    /// KovaaK's session box: Kill Count and Accuracy.
    Kovaak,
    /// Aim Lab's POINTS and TIME boxes.
    Aimlab,
}

/// The run's totals as the HUD shows them at the end: the kills counted, and the hits and shots (None where the
/// Accuracy line was not read).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct HudFinal {
    /// The kills counted.
    pub kills: i64,
    /// The hits (in KovaaK's, with the hits of the kills the Accuracy line had not shown yet); None where the Accuracy
    /// line was not read.
    pub hits: Option<i64>,
    /// The shots, those kills' shots added likewise; None where the Accuracy line was not read.
    pub shots: Option<i64>,
}

/// What the HUD read (python/hud.py: read() and read_aimlab()). Frames are the recording's frame indexes (0 is the
/// first frame from time 0 on), on the video's own clock.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct HudReading {
    /// Which game's HUD this is.
    pub game: HudGame,
    /// One entry per kill: the frame its count went up.
    pub kills: Vec<i64>,
    /// One entry per shot: the frame its count went up.
    pub shots: Vec<i64>,
    /// One entry per hit: the frame its count went up.
    pub hits: Vec<i64>,
    /// The run's totals at the end ("final" in the JSON).
    #[serde(rename = "final")]
    pub totals: HudFinal,
    /// The share of the count's steps that were a plausible step (+1 kill, a hit or a miss).
    pub checked: f64,
    /// Aim Lab's points at the end (it shows no score otherwise); None for KovaaK's.
    pub points: Option<f64>,
}

// ---- scaling --------------------------------------------------------------------------------------------------------

/// How a crop of the frame is scaled, as python/hud.py had ffmpeg scale it: `area` for KovaaK's box, bicubic for Aim
/// Lab's line.
#[derive(Clone, Copy)]
enum Filter {
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
struct Axis {
    /// Each output pixel's first source pixel (in the frame) and its weights.
    taps: Box<[(usize, Box<[f32]>)]>,
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
struct Scale {
    /// The columns' taps.
    x: Axis,
    /// The rows' taps.
    y: Axis,
    /// The bytes in a row of the frame's plane (its width).
    stride: usize,
}

impl Scale {
    /// The crop of a `width` x `height` frame between the shares `share` (x0, y0, x1, y1), scaled to `out` (width,
    /// height) with `filter`.
    fn new(width: usize, height: usize, share: [f64; 4], out: (usize, usize), filter: Filter) -> Scale {
        Scale {
            x: Axis::new(width, share[0], share[2], out.0, filter),
            y: Axis::new(height, share[1], share[3], out.1, filter),
            stride: width,
        }
    }

    /// The scaled crop's pixels in `rows` x `columns` (row by row), each source byte through `levels`.
    fn rect(&self, plane: &[u8], levels: &[u8; 256], rows: Range<usize>, columns: Range<usize>) -> Box<[u8]> {
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
fn glyph_image(strength: &[f32], width: usize, height: usize) -> [u8; GLYPH_PIXELS] {
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
fn read_levels(full_range: bool) -> [u8; 256] {
    std::array::from_fn(|level| {
        if full_range {
            level as u8
        } else {
            ((level as f64 - LIMITED_BLACK) * 255.0 / LIMITED_SPAN).round().clamp(0.0, 255.0) as u8
        }
    })
}

// ---- levels and spans -----------------------------------------------------------------------------------------------

/// How many there are of each byte value.
fn histogram<'a>(levels: impl IntoIterator<Item = &'a u8>) -> [u32; 256] {
    let mut counts = [0u32; 256];
    levels.into_iter().for_each(|&level| counts[level as usize] += 1);
    counts
}

/// The `rank`-th smallest (from 0) of a histogram's values (the bin's index).
fn nth_smallest(histogram: &[u32], rank: usize) -> usize {
    let mut seen = 0;
    for (value, &count) in histogram.iter().enumerate() {
        seen += count as usize;
        if seen > rank {
            return value;
        }
    }
    histogram.len() - 1
}

/// numpy's percentile (linear) of a histogram's `count` values, as a bin index.
fn percentile(histogram: &[u32], count: usize, percent: f64) -> f64 {
    let at = percent / 100.0 * (count - 1) as f64;
    let below = at.floor() as usize;
    let a = nth_smallest(histogram, below) as f64;
    let b = nth_smallest(histogram, (below + 1).min(count - 1)) as f64;
    a + (at - below as f64) * (b - a)
}

/// Twice the median of `count` bytes from their histogram (an even count's median can fall between two values).
fn twice_median(histogram: &[u32; 256], count: usize) -> i32 {
    if count % 2 == 1 {
        2 * nth_smallest(histogram, count / 2) as i32
    } else {
        (nth_smallest(histogram, count / 2 - 1) + nth_smallest(histogram, count / 2)) as i32
    }
}

/// numpy's percentile (linear) of sorted values.
fn percentile_sorted(sorted: &[f64], percent: f64) -> f64 {
    let at = percent / 100.0 * (sorted.len() - 1) as f64;
    let below = at.floor() as usize;
    let (a, b) = (sorted[below], sorted[(below + 1).min(sorted.len() - 1)]);
    a + (at - below as f64) * (b - a)
}

/// Text pixels of an image (python/hud.py: _ink, which reads the levels as whole numbers): far from its most common
/// level (the box), in either direction.
fn ink(image: &[f64]) -> Vec<bool> {
    let image: Vec<f64> = image.iter().map(|level| level.trunc()).collect();
    let background = median(&image);
    let mut distances: Vec<f64> = image.iter().map(|level| (level - background).abs()).collect();
    distances.sort_by(f64::total_cmp);
    let threshold = MIN_INK_LEVELS.max(INK_SHARE_OF_TOP * percentile_sorted(&distances, TOP_PERCENTILE));
    image.iter().map(|level| (level - background).abs() > threshold).collect()
}

/// Runs of true, as (start, end), at least `min_len` long.
fn spans(mask: impl IntoIterator<Item = bool>, min_len: usize) -> Vec<(usize, usize)> {
    let (mut found, mut start, mut len) = (Vec::new(), None, 0);
    for (i, on) in mask.into_iter().enumerate() {
        match (on, start) {
            (true, None) => start = Some(i),
            (false, Some(first)) => {
                if i - first >= min_len {
                    found.push((first, i));
                }
                start = None;
            }
            _ => {}
        }
        len = i + 1;
    }
    if let Some(first) = start
        && len - first >= min_len
    {
        found.push((first, len));
    }
    found
}

/// A band's ink pixels, row by row.
#[derive(Clone, Copy)]
struct Ink<'a> {
    /// Whether each pixel is ink, row by row.
    pixels: &'a [bool],
    /// The band's width in pixels.
    width: usize,
}

impl<'a> Ink<'a> {
    /// The band's height in pixels.
    fn height(self) -> usize {
        self.pixels.len() / self.width
    }

    /// Whether row `y` has ink in columns a..b.
    fn row_has_ink(self, y: usize, (a, b): (usize, usize)) -> bool {
        self.pixels[y * self.width + a..y * self.width + b].iter().any(|&on| on)
    }

    /// Whether each column has ink.
    fn columns(self) -> impl Iterator<Item = bool> + 'a {
        let height = self.height();
        (0..self.width).map(move |x| (0..height).any(|y| self.pixels[y * self.width + x]))
    }

    /// Whether each row has ink in columns a..b.
    fn rows(self, columns: (usize, usize)) -> impl Iterator<Item = bool> + 'a {
        (0..self.height()).map(move |y| self.row_has_ink(y, columns))
    }

    /// The rows with ink in columns a..b.
    fn inked_rows(self, columns: (usize, usize)) -> Vec<usize> {
        (0..self.height()).filter(|&y| self.row_has_ink(y, columns)).collect()
    }
}

// ---- KovaaK's box ---------------------------------------------------------------------------------------------------

/// A value row of KovaaK's box: the band read (rows y0..y1 of the scaled region: the row and ROW_PAD_PX around it), and
/// where its value can start, from the box's x0 (the compact HUD: just past its label's colon; None: the rightmost
/// group).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct Row {
    /// The band's first row in the scaled region.
    y0: usize,
    /// The row after the band's last.
    y1: usize,
    /// The column the value can start at, from the box's x0; None: the rightmost group of ink.
    start: Option<usize>,
}

/// Where KovaaK's box has its values: the columns x0..x1 of the scaled region, and the Kill Count and Accuracy rows.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct Layout {
    /// The first column read, inside the box's left edge.
    x0: usize,
    /// The column after the last one read, inside the box's right edge.
    x1: usize,
    /// The Kill Count row.
    kills: Row,
    /// The Accuracy row.
    accuracy: Row,
}

/// KovaaK's session box as python/hud.py's layout finds it, for the area finder (src/areas.rs): the columns of its text
/// rows and the top of the first row and the bottom of the last, in the scaled region (BW x BH: the pixels of a
/// 2560 x 1440 frame, from its top left corner).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRows {
    /// The text rows' first column, inside the box's left edge.
    pub x0: usize,
    /// The first text row's top.
    pub y0: usize,
    /// The column after the text rows' last, inside the box's right edge.
    pub x1: usize,
    /// The row after the last text row's bottom.
    pub y1: usize,
}

/// The column just past the first colon in a row of text: a narrow glyph made of dots only (an i has a stem; the
/// colon's upper dot can fade in a blurred recording).
fn label_end(ink: Ink) -> Option<usize> {
    let height = ink.height() as f64;
    spans(ink.columns(), 1).into_iter().find_map(|(a, b)| {
        let dots = spans(ink.rows((a, b)), 1);
        let narrow = (b - a) as f64 <= COLON_MAX_WIDTH_PX.max(COLON_MAX_WIDTH_SHARE * height);
        let all_dots = dots.iter().all(|&(top, bottom)| (bottom - top) as f64 <= DOT_MAX_HEIGHT_SHARE * height);
        (narrow && !dots.is_empty() && all_dots).then_some(b)
    })
}

/// The pixels a flood fill reached at one level (4-connected, as ndimage.label): the first in raster order, how many,
/// their bounds in the region (rows top..bottom, columns left..right) and how many of the patch's pixels are among
/// them.
#[derive(Clone, Copy)]
struct Component {
    /// The first of its pixels in raster order (an index into the region), for ndimage's order among equals.
    first_pixel: usize,
    /// How many pixels it has.
    pixels: usize,
    /// Its first row.
    top: usize,
    /// The row after its last.
    bottom: usize,
    /// Its first column.
    left: usize,
    /// The column after its last.
    right: usize,
    /// How many of the patch's pixels it holds.
    in_patch: usize,
}

impl Component {
    /// Its bounds' height in pixels.
    fn height(&self) -> usize {
        self.bottom - self.top
    }

    /// Its bounds' width in pixels.
    fn width(&self) -> usize {
        self.right - self.left
    }

    /// Too small for the box, or the open scene running off the region.
    fn too_small_or_open(&self) -> bool {
        (self.height() as f64) < MIN_BOX_HEIGHT_SHARE * BH as f64
            || (self.width() as f64) < MIN_BOX_WIDTH_SHARE * BW as f64
            || self.bottom >= BH - REGION_MARGIN_PX
            || self.right >= BW - REGION_MARGIN_PX
    }

    /// Not the filled rectangle (with text holes) a box is.
    fn is_hollow(&self) -> bool {
        (self.pixels as f64) < MIN_BOX_FILL * (self.height() * self.width()) as f64
    }
}

/// Labels the region's pixels by component; its buffers are kept from patch to patch.
struct FloodFill {
    /// Each region pixel's component, from 1; 0 for none.
    labels: Vec<u32>,
    /// The pixels the fill has still to visit.
    stack: Vec<usize>,
}

impl FloodFill {
    /// The components of the pixels at a level (`near`) that the patch is in, and of them the one with the most of the
    /// patch (ties: the first in raster order, as ndimage numbers them).
    fn biggest_in_patch(&mut self, patch: &[usize], near: impl Fn(usize) -> bool) -> Option<Component> {
        self.labels.fill(0);
        let mut components: Vec<Component> = Vec::new();
        for &pixel in patch {
            if near(pixel) && self.labels[pixel] == 0 {
                let id = components.len() as u32 + 1;
                components.push(self.fill(pixel, id, &near));
            }
        }
        for &pixel in patch {
            if self.labels[pixel] != 0 {
                components[self.labels[pixel] as usize - 1].in_patch += 1;
            }
        }
        components.into_iter().max_by(|a, b| a.in_patch.cmp(&b.in_patch).then(b.first_pixel.cmp(&a.first_pixel)))
    }

    /// The component of pixel `seed`, labelled `id`.
    fn fill(&mut self, seed: usize, id: u32, near: impl Fn(usize) -> bool) -> Component {
        let (y, x) = (seed / BW, seed % BW);
        let mut component =
            Component { first_pixel: seed, pixels: 0, top: y, bottom: y + 1, left: x, right: x + 1, in_patch: 0 };
        self.labels[seed] = id;
        self.stack.push(seed);
        while let Some(pixel) = self.stack.pop() {
            let (y, x) = (pixel / BW, pixel % BW);
            component.first_pixel = component.first_pixel.min(pixel);
            component.pixels += 1;
            component.top = component.top.min(y);
            component.bottom = component.bottom.max(y + 1);
            component.left = component.left.min(x);
            component.right = component.right.max(x + 1);
            let neighbors = [
                (y > 0).then(|| pixel - BW),
                (y + 1 < BH).then(|| pixel + BW),
                (x > 0).then(|| pixel - 1),
                (x + 1 < BW).then(|| pixel + 1),
            ];
            for neighbor in neighbors.into_iter().flatten() {
                if self.labels[neighbor] == 0 && near(neighbor) {
                    self.labels[neighbor] = id;
                    self.stack.push(neighbor);
                }
            }
        }
        component
    }
}

/// The patches tried for the box's level, their top left corners (x, y).
fn patch_corners() -> impl Iterator<Item = (usize, usize)> {
    let grid =
        PATCH_ROWS.step_by(PATCH_STEP_PX).flat_map(|y| PATCH_COLUMNS.step_by(PATCH_STEP_PX).map(move |x| (x, y)));
    std::iter::once(FIRST_PATCH).chain(grid)
}

/// Each pixel's median over the key frames' regions.
fn median_of_keys(keys: &[Box<[u8]>]) -> Vec<f64> {
    let mut levels = vec![0u8; keys.len()];
    (0..BW * BH)
        .map(|i| {
            levels.iter_mut().zip(keys).for_each(|(level, key)| *level = key[i]);
            levels.sort_unstable();
            let count = levels.len();
            if count % 2 == 1 {
                levels[count / 2] as f64
            } else {
                (levels[count / 2 - 1] as f64 + levels[count / 2] as f64) / 2.0
            }
        })
        .collect()
}

/// KovaaK's box from the key frames' median (BW x BH): its value rows and its text rows, neither without a box
/// (python/hud.py: layout, and read's choice of rows). The median keeps the box and its labels and washes out the
/// moving scene and the changing numbers. The box is the area at the level of a patch inside its left edge; other
/// players' HUDs are smaller or placed elsewhere, so other patches are tried until one gives a box.
fn find_box(key_median: &[f64]) -> HudKeys {
    let mut flood = FloodFill { labels: vec![0u32; BW * BH], stack: Vec::new() };
    let mut tried = Vec::new();
    for (corner_x, corner_y) in patch_corners() {
        let patch: [usize; PATCH_PIXELS] =
            std::array::from_fn(|i| (corner_y + i / PATCH_SIDE_PX) * BW + corner_x + i % PATCH_SIDE_PX);
        let level = median(&patch.map(|pixel| key_median[pixel]));
        let near = |pixel: usize| (key_median[pixel] - level).abs() < SAME_LEVEL;
        if (patch.iter().filter(|&&pixel| near(pixel)).count() as f64) < MIN_PATCH_SHARE * PATCH_PIXELS as f64 {
            continue;
        }
        let Some(found) = flood.biggest_in_patch(&patch, near) else {
            continue;
        };
        if tried.contains(&(found.top, found.left)) || found.too_small_or_open() {
            continue;
        }
        tried.push((found.top, found.left));
        if found.is_hollow() {
            continue;
        }
        if let Some(keys) = box_rows(key_median, &found) {
            return keys;
        }
    }
    HudKeys::default()
}

/// A box's text rows and value rows, or None when it has too few text rows. A compact box whose labels' colons are not
/// found has text rows but no value rows (python/hud.py's layout gives its rows; its read then reads the rightmost
/// glyphs, which this reading does not).
fn box_rows(key_median: &[f64], found: &Component) -> Option<HudKeys> {
    let (x0, x1) = (found.left + BOX_INSET_PX, found.right - BOX_INSET_PX);
    let width = x1 - x0;
    let rows_between = |top: usize, bottom: usize| -> Vec<f64> {
        (top..bottom).flat_map(|y| key_median[y * BW + x0..y * BW + x1].iter().copied()).collect()
    };
    let text_top = found.top + BOX_INSET_PX;
    let box_ink = ink(&rows_between(text_top, found.bottom - BOX_INSET_PX));
    let inked = box_ink.chunks(width).map(|row| row.iter().filter(|&&on| on).count() > TEXT_ROW_INK_PX);
    let rows: Vec<(usize, usize)> =
        spans(inked, MIN_TEXT_ROW_PX).into_iter().map(|(a, b)| (a + text_top, b + text_top)).collect();
    if rows.len() < MIN_TEXT_ROWS {
        return None;
    }
    let session = Some(SessionRows { x0, y0: rows[0].0, x1, y1: rows[rows.len() - 1].1 });
    // a header (SESSION and the clock) and six rows under it: Kill Count is the first of them, Accuracy the third. The
    // compact HUD has four rows in two columns (Kill Count and SPM, Accuracy, Damage, Avg TTK and KPS), each value just
    // after its label's colon
    let compact = rows.len() == COMPACT_TEXT_ROWS;
    let (kills, accuracy) = if compact { (rows[1], rows[2]) } else { (rows[1], rows[3]) };
    let value_start = |row: (usize, usize)| {
        if !compact {
            return None;
        }
        let (top, bottom) = padded(row);
        label_end(Ink { pixels: &ink(&rows_between(top, bottom)), width })
    };
    let (kills_start, accuracy_start) = (value_start(kills), value_start(accuracy));
    if compact && (kills_start.is_none() || accuracy_start.is_none()) {
        return Some(HudKeys { layout: None, session });
    }
    let value_row = |row: (usize, usize), start| {
        let (y0, y1) = padded(row);
        Row { y0, y1, start }
    };
    let layout = Layout { x0, x1, kills: value_row(kills, kills_start), accuracy: value_row(accuracy, accuracy_start) };
    Some(HudKeys { layout: Some(layout), session })
}

/// A row with ROW_PAD_PX rows above and below it, inside the region.
fn padded((top, bottom): (usize, usize)) -> (usize, usize) {
    (top.saturating_sub(ROW_PAD_PX), (bottom + ROW_PAD_PX).min(BH))
}

/// A glyph cut from a frame: its strength image, and its ink's height and width in pixels.
#[derive(Clone, Debug, PartialEq)]
struct Cut {
    /// The ink's strength scaled to GLYPH_WIDTH_PX x GLYPH_HEIGHT_PX (0 to 255).
    image: [u8; GLYPH_PIXELS],
    /// The ink's height in the band, pixels.
    height: u16,
    /// The glyph's width in the band, pixels.
    width: u16,
}

/// The glyph at columns a..b of a band, cropped to its ink rows, or None when it has no ink.
fn cut(strength: &[f32], ink: Ink, (a, b): (usize, usize)) -> Option<Cut> {
    let rows = ink.inked_rows((a, b));
    let (top, bottom) = (*rows.first()?, *rows.last()? + 1);
    let width = ink.width;
    let glyph: Vec<f32> = (top..bottom).flat_map(|y| strength[y * width + a..y * width + b].iter().copied()).collect();
    let image = glyph_image(&glyph, b - a, bottom - top);
    Some(Cut { image, height: (bottom - top) as u16, width: (b - a) as u16 })
}

/// A band's grey levels against its background (its median): the ink threshold and the distance of full strength. The
/// background is kept doubled, so an even count's median stays a whole number.
struct BoxLevels {
    /// Twice the band's median grey level.
    twice_background: i32,
    /// A pixel farther than this from the background is ink, grey levels.
    threshold: f64,
    /// The distance from the background at which ink has full strength (1), grey levels.
    full_strength: f64,
}

impl BoxLevels {
    /// A band's levels from its bytes: the ink threshold is INK_SHARE_OF_TOP of the TOP_PERCENTILE-th distance from
    /// the background, full strength that distance, both at least MIN_INK_LEVELS.
    fn of(band: &[u8]) -> BoxLevels {
        let counts = histogram(band);
        let twice_background = twice_median(&counts, band.len());
        let mut twice_distances = [0u32; MAX_TWICE_DISTANCE + 1];
        for (level, &count) in counts.iter().enumerate() {
            twice_distances[(2 * level as i32 - twice_background).unsigned_abs() as usize] += count;
        }
        let top_distance = percentile(&twice_distances, band.len(), TOP_PERCENTILE) / 2.0;
        BoxLevels {
            twice_background,
            threshold: MIN_INK_LEVELS.max(INK_SHARE_OF_TOP * top_distance),
            full_strength: MIN_INK_LEVELS.max(top_distance),
        }
    }

    /// A grey level's distance from the background.
    fn distance(&self, level: u8) -> f64 {
        (2 * level as i32 - self.twice_background).unsigned_abs() as f64 / 2.0
    }
}

/// The value's glyphs in one row of KovaaK's box (a band `width` wide, python/hud.py: _value_glyphs): the rightmost
/// group of ink columns, cut from the label by a wide gap (or, given start, the first group from there on).
fn value_glyphs(band: &[u8], width: usize, start: Option<usize>) -> Vec<Cut> {
    if band.is_empty() || width == 0 {
        return Vec::new();
    }
    let levels = BoxLevels::of(band);
    let is_ink: [bool; 256] = std::array::from_fn(|level| levels.distance(level as u8) > levels.threshold);
    let strength_of: [f32; 256] =
        std::array::from_fn(|level| (levels.distance(level as u8) / levels.full_strength).clamp(0.0, 1.0) as f32);
    let ink_pixels: Vec<bool> = band.iter().map(|&level| is_ink[level as usize]).collect();
    let strength: Vec<f32> = band.iter().map(|&level| strength_of[level as usize]).collect();
    let ink = Ink { pixels: &ink_pixels, width };
    let mut column_ink = vec![0usize; width];
    for row in ink_pixels.chunks(width) {
        row.iter().zip(&mut column_ink).for_each(|(&on, count)| *count += usize::from(on));
    }
    let edge = box_edge(band, &levels, ink, &column_ink);
    let mut columns = spans(column_ink[..edge].iter().map(|&count| count > 0), 1);
    // ink that runs into the scene past the box's edge is the edge's blended border, not text, which keeps a margin
    // inside the box (python/hud.py read it as a glyph: a box narrower than at most key frames, at a run's start)
    if edge < width && columns.last().is_some_and(|last| last.1 + TEXT_MARGIN_COLUMNS >= edge) {
        columns.pop();
    }
    let group = value_columns(columns, start, LABEL_GAP_SHARE * width as f64);
    split_joined(group, ink, &column_ink).into_iter().filter_map(|piece| cut(&strength, ink, piece)).collect()
}

/// Where the box ends in a band; the band's width when the box fills it. The box widens as its numbers grow: past its
/// right edge the scene fills whole columns with ink, which text never does, or at least leaves no pixel of its columns
/// at the box's level up to the band's end (text leaves the rows above and below it, and a scene line seen through the
/// box is a few columns wide). python/hud.py tested only the first, and read a scene of middle levels past a box
/// narrower than at most key frames as glyphs.
fn box_edge(band: &[u8], levels: &BoxLevels, ink: Ink, column_ink: &[usize]) -> usize {
    let (width, height) = (ink.width, ink.height());
    let past_label = |x: usize| x as f64 > MIN_EDGE_SHARE * width as f64;
    let solid = (0..width)
        .find(|&x| past_label(x) && column_ink[x] as f64 / height as f64 > SCENE_COLUMN_INK_SHARE)
        .unwrap_or(width);
    let off_box_level = |x: &usize| (0..height).all(|y| levels.distance(band[y * width + x]) > levels.threshold / 2.0);
    let scene = (0..width)
        .rev()
        .take_while(off_box_level)
        .last()
        .filter(|&x| past_label(x) && x + MIN_SCENE_COLUMNS <= width)
        .unwrap_or(width);
    solid.min(scene)
}

/// The value's group of ink columns: given `start`, the first columns from there up to a gap wider than `gap` (the
/// next label); else the rightmost columns back to such a gap (the label).
fn value_columns(columns: Vec<(usize, usize)>, start: Option<usize>, gap: f64) -> Vec<(usize, usize)> {
    let mut group: Vec<(usize, usize)> = Vec::new();
    if let Some(start) = start {
        for column in columns.into_iter().filter(|column| column.0 >= start) {
            if group.last().is_some_and(|last| (column.0 - last.1) as f64 > gap) {
                break;
            }
            group.push(column);
        }
    } else {
        for column in columns.into_iter().rev() {
            if group.last().is_some_and(|first| (first.0 - column.1) as f64 > gap) {
                break;
            }
            group.push(column);
        }
        group.reverse();
    }
    group
}

/// The group's glyphs, each glyph of digits run together split into one piece per digit.
fn split_joined(group: Vec<(usize, usize)>, ink: Ink, column_ink: &[usize]) -> Vec<(usize, usize)> {
    let mut pieces = Vec::new();
    for (a, b) in group {
        let rows = ink.inked_rows((a, b));
        let glyph_height = (rows[rows.len() - 1] - rows[0] + 1) as f64;
        let glyph_width = (b - a) as f64;
        let digit_count = (glyph_width / glyph_height / DIGIT_ASPECT).round_ties_even() as usize;
        if glyph_width / glyph_height < SPLIT_MIN_ASPECT || digit_count < 2 {
            pieces.push((a, b));
            continue;
        }
        let cuts = split_columns((a, b), digit_count, column_ink);
        pieces.extend(cuts.windows(2).filter(|pair| pair[1] > pair[0]).map(|pair| (pair[0], pair[1])));
    }
    pieces
}

/// Where a glyph at columns a..b, `digit_count` digits wide, is cut: at its thinnest column (the least ink) near each
/// even cut, with a and b at the ends.
fn split_columns((a, b): (usize, usize), digit_count: usize, column_ink: &[usize]) -> Vec<usize> {
    let glyph_width = (b - a) as f64;
    let mut cuts = Vec::with_capacity(digit_count + 1);
    cuts.push(a);
    for j in 1..digit_count {
        let even = glyph_width * j as f64 / digit_count as f64;
        let reach = CUT_SEARCH_SHARE * glyph_width / digit_count as f64;
        let (from, to) = ((even - reach) as usize, ((even + reach) as usize + 1).min(b - a));
        let thinnest = (from..to).fold(from, |best, i| if column_ink[a + i] < column_ink[a + best] { i } else { best });
        cuts.push(a + thinnest);
    }
    cuts.push(b);
    cuts
}

// ---- Aim Lab's boxes ------------------------------------------------------------------------------------------------

/// The white value's glyphs in one of Aim Lab's boxes (columns c0..c1 of the band, python/hud.py: _aim_glyphs), the
/// colon left out.
fn aim_glyphs(band: &[u8], (c0, c1): (usize, usize)) -> Vec<Cut> {
    let width = c1 - c0;
    let levels: Vec<u8> = band.chunks(AW).flat_map(|row| row[c0..c1].iter().copied()).collect();
    let counts = histogram(&levels);
    let count = width * AH;
    let twice_background = twice_median(&counts, count);
    // twice the distance from the background, signed (white text is above it), offset to index from 0
    let offset = MAX_TWICE_DISTANCE as i32;
    let mut twice_distances = [0u32; 2 * MAX_TWICE_DISTANCE + 1];
    for (level, &level_count) in counts.iter().enumerate() {
        twice_distances[(2 * level as i32 - twice_background + offset) as usize] += level_count;
    }
    let top = (percentile(&twice_distances, count, TOP_PERCENTILE) - f64::from(offset)) / 2.0;
    if top < AIM_MIN_TOP_LEVELS {
        return Vec::new();
    }
    let distance = |level: u8| (2 * level as i32 - twice_background) as f64 / 2.0;
    let ink_pixels: Vec<bool> = levels.iter().map(|&level| distance(level) > INK_SHARE_OF_TOP * top).collect();
    let strength: Vec<f32> = levels.iter().map(|&level| (distance(level) / top).clamp(0.0, 1.0) as f32).collect();
    let ink = Ink { pixels: &ink_pixels, width };
    spans(ink.columns(), 1)
        .into_iter()
        .filter(|&columns| !is_colon(ink, columns))
        .filter_map(|columns| cut(&strength, ink, columns))
        .collect()
}

/// Whether the glyph at columns a..b is the colon: two dots or more, narrow for their height.
fn is_colon(ink: Ink, (a, b): (usize, usize)) -> bool {
    let dots = spans(ink.rows((a, b)), 1);
    let height = dots.last().map_or(0, |dot| dot.1) - dots.first().map_or(0, |dot| dot.0);
    dots.len() >= 2 && (b - a) as f64 <= COLON_MAX_ASPECT * height as f64
}

// ---- what each frame keeps ------------------------------------------------------------------------------------------

/// A stored glyph: its image's index, and its ink's height and width.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Glyph {
    /// The image's index in the store's images.
    image: u32,
    /// The ink's height in the band, pixels.
    height: u16,
    /// The glyph's width in the band, pixels.
    width: u16,
}

/// The glyphs each frame read in each row: the distinct lines of glyphs, and each row's line frame by frame.
#[derive(Clone, Debug, PartialEq)]
struct Store {
    /// The glyph images, GLYPH_PIXELS bytes each (the ink's strength, 0 to 255).
    images: Vec<u8>,
    /// The distinct lines of glyphs; line 0 is the empty one.
    lines: Vec<Box<[Glyph]>>,
    /// Each row's lines frame by frame, as runs (line, frames).
    rows: [Vec<(u32, u32)>; ROWS],
}

impl Default for Store {
    /// A store with only the empty line 0 and no frames.
    fn default() -> Store {
        Store { images: Vec::new(), lines: vec![Box::default()], rows: Default::default() }
    }
}

/// Whether two glyph images are near enough to be the same glyph (NEAR_MAX, NEAR_SUM).
fn near(a: &[u8], b: &[u8]) -> bool {
    let mut sum = 0;
    for (&level_a, &level_b) in a.iter().zip(b) {
        let difference = level_a.abs_diff(level_b);
        if difference > NEAR_MAX {
            return false;
        }
        sum += difference as u32;
    }
    sum <= NEAR_SUM
}

impl Store {
    /// The glyph image at `index`, GLYPH_PIXELS bytes.
    fn image(&self, index: u32) -> &[u8] {
        &self.images[index as usize * GLYPH_PIXELS..(index as usize + 1) * GLYPH_PIXELS]
    }

    /// Whether a stored glyph and a cut are the same glyph: the same ink size and a near image.
    fn same_glyph(&self, glyph: &Glyph, cut: &Cut) -> bool {
        glyph.height == cut.height && glyph.width == cut.width && near(self.image(glyph.image), &cut.image)
    }

    /// A line's tall glyphs, in a band `band` rows high.
    fn tall_glyphs(&self, line: u32, band: usize) -> impl Iterator<Item = &Glyph> {
        self.lines[line as usize].iter().filter(move |glyph| tall(glyph, band))
    }

    /// Adds `frames` frames of `line` to a row: the last run grows when it is the same line; no frames add nothing.
    fn add_run(&mut self, row: usize, line: u32, frames: u32) {
        match self.rows[row].last_mut() {
            Some(last) if last.0 == line => last.1 += frames,
            _ if frames > 0 => self.rows[row].push((line, frames)),
            _ => {}
        }
    }

    /// One frame's glyphs in a row: the row's line before when they are the same glyphs, else a new line, whose
    /// glyphs reuse the images of the row's `recent` glyphs they are the same as (a number changes a digit at a time).
    fn push(&mut self, row: usize, cuts: Vec<Cut>, recent: &mut Capped<Glyph, RECENT_GLYPHS>) {
        let last = self.rows[row].last().map_or(0, |run| run.0);
        let line = if cuts.is_empty() {
            0
        } else if self.is_line(last, &cuts) {
            last
        } else {
            let glyphs = cuts.iter().map(|cut| self.stored_glyph(cut, recent)).collect();
            self.lines.push(glyphs);
            (self.lines.len() - 1) as u32
        };
        self.add_run(row, line, 1);
    }

    /// Whether `line` has the same glyphs as `cuts`.
    fn is_line(&self, line: u32, cuts: &[Cut]) -> bool {
        let glyphs = &self.lines[line as usize];
        glyphs.len() == cuts.len() && glyphs.iter().zip(cuts).all(|(glyph, cut)| self.same_glyph(glyph, cut))
    }

    /// A cut's glyph: a recent glyph that is the same, else a new image, which becomes a recent glyph.
    fn stored_glyph(&mut self, cut: &Cut, recent: &mut Capped<Glyph, RECENT_GLYPHS>) -> Glyph {
        if let Some(&glyph) = recent.iter().rev().find(|glyph| self.same_glyph(glyph, cut)) {
            return glyph;
        }
        self.images.extend_from_slice(&cut.image);
        let image = (self.images.len() / GLYPH_PIXELS - 1) as u32;
        let glyph = Glyph { image, height: cut.height, width: cut.width };
        if recent.len() == RECENT_GLYPHS {
            recent.remove(0);
        }
        recent.push(glyph);
        glyph
    }

    /// A row's line in each frame.
    fn per_frame(&self, row: usize) -> Vec<u32> {
        self.rows[row].iter().flat_map(|&(line, frames)| repeat_n(line, frames as usize)).collect()
    }

    /// The next run part's store after this one's, its first `left_out` frames left out.
    fn append(&mut self, next: Store, left_out: usize) {
        let image_offset = (self.images.len() / GLYPH_PIXELS) as u32;
        let line_offset = self.lines.len() as u32 - 1;
        self.images.extend(next.images);
        self.lines.extend(next.lines.into_iter().skip(1).map(|line| {
            line.into_iter().map(|glyph| Glyph { image: glyph.image + image_offset, ..glyph }).collect()
        }));
        for (row, runs) in next.rows.into_iter().enumerate() {
            let mut to_drop = left_out as u32;
            for (line, frames) in runs {
                let kept = frames - to_drop.min(frames);
                to_drop -= frames - kept;
                self.add_run(row, if line == 0 { 0 } else { line + line_offset }, kept);
            }
        }
    }
}

// ---- the watch ------------------------------------------------------------------------------------------------------

/// A run part's share of the watch (a review split into run parts: each part's watch reads its own frames, the page
/// joins them).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "PartText", into = "PartText")]
pub struct HudPart {
    /// The frames the part's watch read, skipped ones included.
    frames: usize,
    /// KovaaK's box, None without one (each run part's watch works it out from the same key frames).
    layout: Option<Layout>,
    /// The box's text rows, None without a box.
    session: Option<SessionRows>,
    /// The glyphs the part's frames read.
    store: Store,
}

/// What a watch reads in the key frames (`HudWatch::keys`): KovaaK's box, None without one, and its text rows. Each
/// run's watch starts from it (`HudWatch::from_keys`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HudKeys {
    /// KovaaK's box's value rows; None without a box, or a compact box whose colons were not found.
    layout: Option<Layout>,
    /// The box's text rows; None without a box.
    session: Option<SessionRows>,
}

impl HudKeys {
    /// KovaaK's session box's text rows, for src/areas.rs; None without a box.
    pub fn session(&self) -> Option<SessionRows> {
        self.session
    }
}

/// A part as JSON: the glyph images as hex.
#[derive(Serialize, Deserialize)]
struct PartText {
    /// The part's frames (`HudPart::frames`).
    frames: usize,
    /// KovaaK's box (`HudPart::layout`).
    layout: Option<Layout>,
    /// The box's text rows, left out without a box.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session: Option<SessionRows>,
    /// The glyph images, GLYPH_PIXELS bytes each, as hex.
    images: String,
    /// Each line's glyphs after the empty line 0: [image, ink height, ink width].
    lines: Vec<Vec<[u32; 3]>>,
    /// Each row's runs: [line, frames].
    rows: [Vec<[u32; 2]>; ROWS],
}

impl From<HudPart> for PartText {
    /// The part as JSON's fields: its images as hex, its lines without the empty line 0.
    fn from(part: HudPart) -> PartText {
        PartText {
            frames: part.frames,
            layout: part.layout,
            session: part.session,
            images: part.store.images.iter().map(|byte| format!("{byte:02x}")).collect(),
            lines: part.store.lines[1..]
                .iter()
                .map(|line| line.iter().map(|glyph| [glyph.image, glyph.height as u32, glyph.width as u32]).collect())
                .collect(),
            rows: part.store.rows.map(|runs| runs.into_iter().map(|(line, frames)| [line, frames]).collect()),
        }
    }
}

impl TryFrom<PartText> for HudPart {
    /// Why the text is no part.
    type Error = String;

    /// The part back from JSON; an error when its images are not whole glyphs in hex, a line names an image it does
    /// not have, or a row's runs do not name its lines or add up to its frames.
    fn try_from(text: PartText) -> Result<HudPart, String> {
        let images = from_hex(&text.images)
            .filter(|bytes| bytes.len() % GLYPH_PIXELS == 0)
            .ok_or("a HUD part's images are not hex glyphs")?;
        let count = (images.len() / GLYPH_PIXELS) as u32;
        let mut lines = vec![Box::default()];
        for line in text.lines {
            let glyphs = line
                .into_iter()
                .map(|[image, height, width]| {
                    (image < count && height <= u16::MAX as u32 && width <= u16::MAX as u32).then_some(Glyph {
                        image,
                        height: height as u16,
                        width: width as u16,
                    })
                })
                .collect::<Option<Box<[Glyph]>>>()
                .ok_or("a HUD part's line has a glyph it does not have")?;
            lines.push(glyphs);
        }
        let mut rows: [Vec<(u32, u32)>; ROWS] = Default::default();
        for (row, runs) in rows.iter_mut().zip(text.rows) {
            if runs.iter().any(|&[line, _]| line as usize >= lines.len())
                || runs.iter().map(|&[_, frames]| frames as usize).sum::<usize>() != text.frames
            {
                return Err("a HUD part's rows do not match its lines or frames".into());
            }
            *row = runs.into_iter().map(|[line, frames]| (line, frames)).collect();
        }
        let store = Store { images, lines, rows };
        Ok(HudPart { frames: text.frames, layout: text.layout, session: text.session, store })
    }
}

/// Bytes from hex, two digits each; None when it is not hex.
fn from_hex(text: &str) -> Option<Vec<u8>> {
    (0..text.len()).step_by(2).map(|i| text.get(i..i + 2).and_then(|pair| u8::from_str_radix(pair, 16).ok())).collect()
}

/// Reads a recording's HUD: first every key frame (`add_key`, for where KovaaK's box and its rows are), then every
/// frame in order (`add`), then `finish`. A review split into run parts gives each part a watch that reads every key
/// frame and then only its part's frames (and the next part's first, as the camera watch does); `part` and `join` put
/// them together.
pub struct HudWatch {
    /// The frames' width in pixels.
    width: usize,
    /// The frames' height in pixels.
    height: usize,
    /// The Y levels as read: a limited-range recording's stretched to 0..255.
    levels: [u8; 256],
    /// KovaaK's box region (BOX) scaled to BW x BH.
    region: Scale,
    /// Aim Lab's value line (AIM_BAND) scaled to AW x AH.
    aim: Scale,
    /// The key frames' box regions (BW x BH), every `key_step`-th of the `keys_seen`.
    keys: Capped<Box<[u8]>, { MAX_KEY_FRAMES + 1 }>,
    /// The key frames added so far.
    keys_seen: usize,
    /// One key frame in this many is kept; it doubles each time `keys` passes MAX_KEY_FRAMES.
    key_step: usize,
    /// KovaaK's box: None until worked out (at the first frame), then Some(None) when there is none.
    layout: Option<Option<Layout>>,
    /// The box's text rows, worked out with `layout`.
    session: Option<SessionRows>,
    /// The frames read so far, skipped ones included.
    frames: usize,
    /// The glyphs read so far.
    store: Store,
    /// Each row's latest new glyphs, whose images a new line can reuse.
    recent: [Capped<Glyph, RECENT_GLYPHS>; ROWS],
}

impl HudWatch {
    /// For a recording whose frames are `width` x `height`; `full_range`: its Y spans 0..255 (else 16..235).
    pub fn new(width: usize, height: usize, full_range: bool) -> HudWatch {
        HudWatch {
            width,
            height,
            levels: read_levels(full_range),
            region: Scale::new(width, height, BOX, (BW, BH), Filter::Area),
            aim: Scale::new(width, height, AIM_BAND, (AW, AH), Filter::Cubic),
            keys: Capped::new(),
            keys_seen: 0,
            key_step: 1,
            layout: None,
            session: None,
            frames: 0,
            store: Store::default(),
            recent: Default::default(),
        }
    }

    /// The rows of a frame's Y plane `add` reads, from the top (KovaaK's box and Aim Lab's value line): the rows below
    /// them need not hold the frame's levels, though the plane it is given must still be `width` x `height` bytes.
    pub fn rows_read(&self) -> usize {
        let last = |scale: &Scale| scale.y.taps.iter().map(|(first, weights)| first + weights.len()).max().unwrap_or(0);
        last(&self.region).max(last(&self.aim)).min(self.height)
    }

    /// Whether a Y plane can be read: the watch has a size and the plane is at least `width` x `height` bytes.
    fn readable(&self, luma: &[u8]) -> bool {
        self.width > 0 && self.height > 0 && luma.len() >= self.width * self.height
    }

    /// One key frame's Y plane (`width` x `height` bytes), in order, before any frame is added.
    pub fn add_key(&mut self, luma: &[u8]) {
        if self.layout.is_some() || !self.readable(luma) {
            return;
        }
        if self.keys_seen.is_multiple_of(self.key_step) {
            self.keys.push(self.region.rect(luma, &self.levels, 0..BH, 0..BW));
            if self.keys.len() > MAX_KEY_FRAMES {
                let kept = std::mem::take(&mut self.keys).into_iter().step_by(2).collect();
                self.keys = kept;
                self.key_step *= 2;
            }
        }
        self.keys_seen += 1;
    }

    /// KovaaK's box from the key frames (at least 3: python/hud.py's layout), once.
    fn work_out_layout(&mut self) {
        if self.layout.is_some() {
            return;
        }
        let keys = std::mem::take(&mut self.keys);
        let found = if keys.len() < MIN_KEY_FRAMES { HudKeys::default() } else { find_box(&median_of_keys(&keys)) };
        self.layout = Some(found.layout);
        self.session = found.session;
    }

    /// KovaaK's session box's text rows from the key frames (python/hud.py's layout, as src/areas.rs needs it), or None
    /// without a box. Call it after the last key frame: the box is worked out from the key frames added so far.
    pub fn session_box(&mut self) -> Option<SessionRows> {
        self.work_out_layout();
        self.session
    }

    /// What the key frames gave: KovaaK's box and its rows. Call it after the last key frame.
    pub fn keys(&mut self) -> HudKeys {
        self.work_out_layout();
        HudKeys { layout: self.layout.flatten(), session: self.session }
    }

    /// A watch that starts from what another read in the key frames (`keys`), as if it had read them itself.
    pub fn from_keys(width: usize, height: usize, full_range: bool, keys: &HudKeys) -> HudWatch {
        let mut watch = HudWatch::new(width, height, full_range);
        watch.layout = Some(keys.layout);
        watch.session = keys.session;
        watch
    }

    /// Frames not reviewed before the first one added (a review from part way in, the user's run window).
    pub fn skip(&mut self, frames: usize) {
        for row in 0..ROWS {
            self.store.add_run(row, 0, frames as u32);
        }
        self.frames += frames;
    }

    /// One frame's Y plane (`width` x `height` bytes), in order.
    pub fn add(&mut self, luma: &[u8]) {
        self.work_out_layout();
        let mut cuts: [Vec<Cut>; ROWS] = Default::default();
        if self.readable(luma) {
            if let Some(Some(layout)) = self.layout {
                for (row, value_row) in [(KILL_COUNT_ROW, layout.kills), (ACCURACY_ROW, layout.accuracy)] {
                    let band = self.region.rect(luma, &self.levels, value_row.y0..value_row.y1, layout.x0..layout.x1);
                    cuts[row] = value_glyphs(&band, layout.x1 - layout.x0, value_row.start);
                }
            }
            let band = self.aim.rect(luma, &self.levels, 0..AH, 0..AW);
            cuts[POINTS_ROW] = aim_glyphs(&band, AIM_POINTS);
            cuts[TIME_ROW] = aim_glyphs(&band, AIM_TIME);
        }
        for ((row, row_cuts), recent) in cuts.into_iter().enumerate().zip(&mut self.recent) {
            self.store.push(row, row_cuts, recent);
        }
        self.frames += 1;
    }

    /// The frames read so far (skipped ones included).
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// The run part's share of the watch.
    pub fn part(mut self) -> HudPart {
        self.work_out_layout();
        HudPart { frames: self.frames, layout: self.layout.flatten(), session: self.session, store: self.store }
    }

    /// The next run part's share. Each part but the last also reads the next part's first frame, so when the watch
    /// already has frames the next part's first frame is left out. A watch that read no key frames takes the parts'
    /// box.
    pub fn join(&mut self, next: HudPart) {
        if self.layout.is_none() {
            self.layout = Some(next.layout);
            self.session = next.session;
        }
        let left_out = usize::from(self.frames > 0);
        self.store.append(next.store, left_out);
        self.frames += next.frames.saturating_sub(left_out);
    }

    /// What the HUD read; None when there is no readable HUD (the review then finds the kills in the video alone).
    pub fn finish(mut self) -> Option<HudReading> {
        self.work_out_layout();
        self.layout.flatten().and_then(|layout| kovaak(&self.store, &layout)).or_else(|| aimlab(&self.store))
    }
}

// ---- reading the counts ---------------------------------------------------------------------------------------------

/// Glyph shapes seen so far; a glyph joins the most alike shape, or starts a new one (python/hud.py: _Shapes). A
/// shape is the mean of its first SHAPE_MEAN_GLYPHS glyphs.
struct Shapes<'a> {
    /// The glyph images the shapes are learned from.
    store: &'a Store,
    /// The least likeness (cosine) for a glyph to join a shape.
    same: f64,
    /// Each shape's mean image (0 to 1 a pixel).
    shapes: Vec<[f32; GLYPH_PIXELS]>,
    /// Each shape's image's length, for the cosine.
    norms: Vec<f64>,
    /// How many glyphs each shape has taken.
    glyph_counts: Vec<u32>,
    /// Bumped whenever a shape changes; each image's most alike shape is kept with the version it was found at.
    version: u32,
    /// Per store image: the version and the most alike shape found then (None: like none).
    alike_cache: Vec<Option<(u32, Option<usize>)>>,
}

/// A vector's length (the square root of its squares' sum).
fn norm(values: impl Iterator<Item = f32>) -> f64 {
    (values.map(|x| x * x).sum::<f32>() as f64).sqrt()
}

/// An image divided by its length (MIN_NORM at least), so its length is 1.
fn to_unit(image: [f32; GLYPH_PIXELS]) -> [f32; GLYPH_PIXELS] {
    let length = norm(image.iter().copied()).max(MIN_NORM) as f32;
    image.map(|x| x / length)
}

/// The dot product of two images; of two unit images, their likeness (cosine).
fn dot(a: &[f32], b: &[f32]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>() as f64
}

impl<'a> Shapes<'a> {
    /// No shapes yet, for the glyphs of `store`, joining at likeness `same`.
    fn new(store: &'a Store, same: f64) -> Shapes<'a> {
        Shapes {
            store,
            same,
            shapes: Vec::new(),
            norms: Vec::new(),
            glyph_counts: Vec::new(),
            version: 0,
            alike_cache: vec![None; store.images.len() / GLYPH_PIXELS],
        }
    }

    /// A store image as strengths from 0 to 1.
    fn glyph(&self, image: u32) -> [f32; GLYPH_PIXELS] {
        let image = self.store.image(image);
        std::array::from_fn(|i| image[i] as f32 / 255.0)
    }

    /// A store image as a unit image (length 1).
    fn unit_glyph(&self, image: u32) -> [f32; GLYPH_PIXELS] {
        to_unit(self.glyph(image))
    }

    /// The shape most alike to an image, when one is at least `same` alike.
    fn most_alike(&self, image: u32) -> Option<usize> {
        let glyph = self.glyph(image);
        let glyph_norm = norm(glyph.iter().copied()).max(MIN_NORM);
        let mut best: Option<(f64, usize)> = None;
        for (shape, (pixels, &shape_norm)) in self.shapes.iter().zip(&self.norms).enumerate() {
            let likeness = dot(&glyph, pixels) / glyph_norm / shape_norm.max(MIN_NORM);
            if likeness >= self.same && best.is_none_or(|(best_likeness, _)| likeness > best_likeness) {
                best = Some((likeness, shape));
            }
        }
        best.map(|(_, shape)| shape)
    }

    /// The glyph's shape; NO_SHAPE when it is like none and `learn` is off.
    fn id(&mut self, image: u32, learn: bool) -> i32 {
        let best = match self.alike_cache[image as usize] {
            Some((version, best)) if version == self.version => best,
            _ => {
                let best = self.most_alike(image);
                self.alike_cache[image as usize] = Some((self.version, best));
                best
            }
        };
        let shape = match best {
            Some(shape) => shape,
            None if !learn => return NO_SHAPE,
            None => self.add_shape(image),
        };
        self.glyph_counts[shape] += 1;
        // the shape is the mean of its first Kill Count glyphs (the Accuracy row's slashes and brackets, cut into
        // pieces, would blur it)
        let glyph_count = self.glyph_counts[shape];
        if learn && glyph_count <= SHAPE_MEAN_GLYPHS && glyph_count > 1 {
            let (glyph, count) = (self.glyph(image), glyph_count as f32);
            self.shapes[shape].iter_mut().zip(&glyph).for_each(|(mean, &level)| *mean += (level - *mean) / count);
            self.norms[shape] = norm(self.shapes[shape].iter().copied());
            self.version += 1;
        }
        shape as i32
    }

    /// A new shape: the image's glyph.
    fn add_shape(&mut self, image: u32) -> usize {
        let glyph = self.glyph(image);
        self.norms.push(norm(glyph.iter().copied()));
        self.shapes.push(glyph);
        self.glyph_counts.push(0);
        self.version += 1;
        self.shapes.len() - 1
    }

    /// A line's tall glyphs (in a band `band` rows high) as shapes, learning new ones; None without any.
    fn learn_line(&mut self, line: u32, band: usize) -> Option<Vec<i32>> {
        let store = self.store;
        let ids: Vec<i32> = store.tall_glyphs(line, band).map(|glyph| self.id(glyph.image, true)).collect();
        (!ids.is_empty()).then_some(ids)
    }

    /// A shape's mean image as a unit image (length 1).
    fn unit(&self, shape: usize) -> [f32; GLYPH_PIXELS] {
        let length = self.norms[shape].max(MIN_NORM) as f32;
        self.shapes[shape].map(|x| x / length)
    }
}

/// A reading that stays the same over the frames first..=last.
struct Stretch<T> {
    /// What the frames read.
    reading: T,
    /// The stretch's first frame.
    first: usize,
    /// Its last frame.
    last: usize,
}

/// The stable readings: each stretch of STABLE_FRAMES frames or more with the same reading (python/hud.py: _runs).
fn stable_stretches<T: Clone + PartialEq>(readings: &[Option<T>]) -> Vec<Stretch<T>> {
    let mut stretches: Vec<Stretch<Option<T>>> = Vec::new();
    for (frame, reading) in readings.iter().enumerate() {
        match stretches.last_mut() {
            Some(last) if last.reading == *reading => last.last = frame,
            _ => stretches.push(Stretch { reading: reading.clone(), first: frame, last: frame }),
        }
    }
    stretches
        .into_iter()
        .filter(|stretch| stretch.last - stretch.first + 1 >= STABLE_FRAMES)
        .filter_map(|stretch| Some(Stretch { reading: stretch.reading?, first: stretch.first, last: stretch.last }))
        .collect()
}

/// Counts in the order first seen (Python's Counter: most_common keeps that order among equal counts).
struct Counter<K>(Vec<(K, usize)>);

impl<K: PartialEq + Copy> Counter<K> {
    /// An empty counter.
    fn new() -> Counter<K> {
        Counter(Vec::new())
    }

    /// Counts `key` once more.
    fn add(&mut self, key: K) {
        match self.0.iter_mut().find(|entry| entry.0 == key) {
            Some(entry) => entry.1 += 1,
            None => self.0.push((key, 1)),
        }
    }

    /// The keys and their counts, most first; equal counts in the order first seen (the sort is stable).
    fn most_common(&self) -> Vec<(K, usize)> {
        let mut sorted = self.0.clone();
        sorted.sort_by_key(|entry| std::cmp::Reverse(entry.1));
        sorted
    }

    /// The most counted key, the first seen among equals; None when nothing was counted.
    fn top(&self) -> Option<K> {
        self.0
            .iter()
            .fold(None, |best: Option<(K, usize)>, &entry| {
                if best.is_none_or(|best| entry.1 > best.1) { Some(entry) } else { best }
            })
            .map(|entry| entry.0)
    }
}

/// Which shape is which digit, from stable readings counting up (python/hud.py: _learn_digits): by shape, its digit.
fn learn_digits(readings: &[&[i32]], shape_count: usize) -> Option<Vec<Option<u8>>> {
    let (ends_in_zero, next_shape) = step_votes(readings);
    let zero = ends_in_zero.top()?;
    let mut next: Vec<(i32, i32)> = Vec::new();
    for ((shape, after), _) in next_shape.most_common() {
        if shape != after && !next.iter().any(|pair| pair.0 == shape) && !next.iter().any(|pair| pair.1 == after) {
            next.push((shape, after));
        }
    }
    let after = |shape: i32| next.iter().find(|pair| pair.0 == shape).map(|pair| pair.1);
    let mut digits = vec![None; shape_count];
    let (mut shape, mut seen) = (zero, Capped::<i32, 10>::from_iter([zero]));
    digits[zero as usize] = Some(0);
    for digit in 1..10 {
        shape = after(shape)?;
        if seen.contains(&shape) {
            return None;
        }
        seen.push(shape);
        digits[shape as usize] = Some(digit);
    }
    (after(shape) == Some(zero)).then_some(digits)
}

/// What each step between stable readings says: the last shape of a number that ends in 0 (the tens place changed, or
/// a digit was added), and the last digit's next shape (usually one more kill).
fn step_votes(readings: &[&[i32]]) -> (Counter<i32>, Counter<(i32, i32)>) {
    let (mut ends_in_zero, mut next_shape) = (Counter::new(), Counter::new());
    for pair in readings.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let (a_last, b_last) = (a[a.len() - 1], b[b.len() - 1]);
        // a digit added after the same digits is no count's step (python/hud.py took it as one): it is a glyph that
        // is not part of the number, such as the edge of KovaaK's results panel fading out at the start of a run
        let grew = b.len() == a.len() + 1 && b[..a.len()] != *a;
        if (b.len() == a.len() && a[..a.len() - 1] != b[..b.len() - 1]) || grew {
            ends_in_zero.add(b_last);
        }
        if b.len() == a.len() || grew {
            next_shape.add((a_last, b_last));
        }
    }
    (ends_in_zero, next_shape)
}

/// The number a reading's shapes spell, if every one is a digit.
fn number(reading: &[i32], digits: &[Option<u8>]) -> Option<i64> {
    let reading_digits = reading
        .iter()
        .map(|&shape| digits.get(usize::try_from(shape).ok()?).copied().flatten())
        .collect::<Option<Vec<u8>>>()?;
    value(&reading_digits)
}

/// The number digits spell (None without any).
fn value(digits: &[u8]) -> Option<i64> {
    if digits.is_empty() {
        return None;
    }
    digits.iter().try_fold(0i64, |total, &digit| total.checked_mul(10)?.checked_add(digit as i64))
}

/// Whether a glyph is tall in a band `band` rows high.
fn tall(glyph: &Glyph, band: usize) -> bool {
    glyph.height as f64 / band as f64 >= TALL_SHARE
}

/// The value rounded to 3 decimals (halves away from 0).
fn round_to_thousandths(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// The Kill Count read: the shapes, the digits, the stable values and the share of steps that were +1.
struct KillCount<'a> {
    /// The shapes learned from the Kill Count, which the Accuracy line is read with too.
    shapes: Shapes<'a>,
    /// By shape, its digit; None for a shape that is no digit.
    digits: Vec<Option<u8>>,
    /// The stable values, in order.
    values: Vec<Stretch<i64>>,
    /// The share of the steps between them that were +1.
    checked: f64,
}

/// The Kill Count read at one likeness (python/hud.py: _count), or None when the digits are not learned or fewer than
/// `need` of the steps are +1. The shapes are learned from the Kill Count alone.
fn count<'a>(store: &'a Store, kill_lines: &[u32], band: usize, same: f64, need: f64) -> Option<KillCount<'a>> {
    let mut shapes = Shapes::new(store, same);
    let readings: Vec<Option<Vec<i32>>> = kill_lines.iter().map(|&line| shapes.learn_line(line, band)).collect();
    let stable = stable_stretches(&readings);
    let stable_readings: Vec<&[i32]> = stable.iter().map(|stretch| stretch.reading.as_slice()).collect();
    let mut digits = learn_digits(&stable_readings, shapes.shapes.len())?;
    join_leftover_shapes(&shapes, &mut digits);
    // the stable values, from each frame's number: a digit caught mid-change can leave a shape of its own for a frame
    // or two, which joined its digit above, so the new value is read from its first frame (python/hud.py reads stable
    // shapes, and starts the value up to two frames late)
    let numbers: Vec<Option<i64>> =
        readings.iter().map(|reading| reading.as_ref().and_then(|reading| number(reading, &digits))).collect();
    let values = stable_stretches(&numbers);
    let steps: Vec<i64> = values.windows(2).map(|pair| pair[1].reading - pair[0].reading).collect();
    if steps.len() < MIN_KILL_COUNT_STEPS {
        return None;
    }
    let checked = steps.iter().filter(|&&step| step == 1).count() as f64 / steps.len() as f64;
    (checked >= need).then_some(KillCount { shapes, digits, values, checked })
}

/// In a blurred recording one digit can leave more than one shape: the others join the most alike digit's shape, when
/// they are DIGIT_LIKENESS alike.
fn join_leftover_shapes(shapes: &Shapes, digits: &mut [Option<u8>]) {
    for shape in 0..shapes.shapes.len() {
        if digits[shape].is_none() {
            let unit = shapes.unit(shape);
            let best = (0..digits.len())
                .filter(|&digit_shape| digits[digit_shape].is_some())
                .map(|digit_shape| (dot(&unit, &shapes.unit(digit_shape)), digit_shape))
                .fold(None, |best: Option<(f64, usize)>, entry| {
                    if best.is_none_or(|best| entry >= best) { Some(entry) } else { best }
                });
            if let Some((likeness, digit_shape)) = best
                && likeness >= DIGIT_LIKENESS
            {
                digits[shape] = digits[digit_shape];
            }
        }
    }
}

/// KovaaK's session box over the recording (python/hud.py: read).
fn kovaak(store: &Store, layout: &Layout) -> Option<HudReading> {
    let kill_lines = store.per_frame(KILL_COUNT_ROW);
    let band = layout.kills.y1 - layout.kills.y0;
    let KillCount { shapes, digits, values, checked } =
        KILL_COUNT_TRIES.into_iter().find_map(|(same, need)| count(store, &kill_lines, band, same, need))?;
    let values = without_lone_misreads(values);
    let values = counted_values(&values)?;
    let kills = kill_frames(values);
    let (since, until) = (values[0].first, values[values.len() - 1].last);
    let reader = AccuracyReader { shapes, digits, band: layout.accuracy.y1 - layout.accuracy.y0, marks: None };
    let accuracy = accuracy_stretches(reader, since, until);
    let mut frames = shot_and_hit_frames(&accuracy, values[0].reading == 0);
    // the totals: the kills counted, and the fullest Accuracy reading (the HUD resets to 0 when the run ends)
    let fullest = accuracy.iter().map(|stretch| stretch.reading).fold(None, |best: Option<(i64, i64)>, reading| {
        if best.is_none_or(|best| reading.1 > best.1) { Some(reading) } else { best }
    });
    let totals = add_late_kills(&mut frames, fullest, &kills);
    Some(HudReading {
        game: HudGame::Kovaak,
        totals: HudFinal {
            kills: kills.len() as i64,
            hits: totals.map(|(hits, _)| hits),
            shots: totals.map(|(_, shots)| shots),
        },
        kills,
        shots: frames.shots,
        hits: frames.hits,
        checked: round_to_thousandths(checked),
        points: None,
    })
}

/// The stable values without a lone misread between two values that follow on (0, 9, 1: a 1 caught mid-change): it
/// looked like a restart, which split the run and lost the kills before it. Only a short one: a real restart's 0
/// between two 1s stays up for a second or more.
fn without_lone_misreads(values: Vec<Stretch<i64>>) -> Vec<Stretch<i64>> {
    let follows = |step: i64| (0..=MAX_KILL_STEP).contains(&step);
    let keep: Vec<bool> = (0..values.len())
        .map(|i| {
            i == 0
                || i == values.len() - 1
                || values[i].last - values[i].first > MAX_MISREAD_FRAMES
                || follows(values[i].reading - values[i - 1].reading)
                || !follows(values[i + 1].reading - values[i - 1].reading)
        })
        .collect();
    values.into_iter().zip(keep).filter_map(|(value, kept)| kept.then_some(value)).collect()
}

/// The values of the run that counts: a drop is a restart (or the end screen), and the run is the stretch between
/// drops with the most kills.
fn counted_values(values: &[Stretch<i64>]) -> Option<&[Stretch<i64>]> {
    let mut drops = vec![0];
    drops.extend((1..values.len()).filter(|&i| values[i].reading < values[i - 1].reading));
    drops.push(values.len());
    let (start, end) = drops
        .windows(2)
        .map(|pair| (pair[0], pair[1]))
        .max_by_key(|&(start, end)| (values[end - 1].reading - values[start].reading, start))?;
    Some(&values[start..end])
}

/// Each kill's frame: every step of up to MAX_KILL_STEP kills counts its kills at the frame its value shows (a bigger
/// jump is a misread).
fn kill_frames(values: &[Stretch<i64>]) -> Vec<i64> {
    let mut kills = Vec::new();
    for pair in values.windows(2) {
        let step = pair[1].reading - pair[0].reading;
        if 0 < step && step <= MAX_KILL_STEP {
            kills.extend(repeat_n(pair[1].first as i64, step as usize));
        }
    }
    kills
}

/// Reads the Accuracy line, hits/shots (percent), with the Kill Count's shapes and digits (`band`: the line's height).
struct AccuracyReader<'a> {
    /// The Kill Count's shapes; no new ones are learned here.
    shapes: Shapes<'a>,
    /// By shape, its digit.
    digits: Vec<Option<u8>>,
    /// The Accuracy band's height in pixels, which says which glyphs are tall.
    band: usize,
    /// The "/" and the "(" as unit images, once they are learned.
    marks: Option<[[f32; GLYPH_PIXELS]; 2]>,
}

impl AccuracyReader<'_> {
    /// A line's tall glyphs by the rule of python/hud.py: a digit when it is like a digit shape at the likeness the
    /// Kill Count was read at (Some), else a mark (None).
    fn by_likeness(&mut self, line: u32) -> Vec<Option<u8>> {
        let store = self.shapes.store;
        store
            .tall_glyphs(line, self.band)
            .map(|glyph| usize::try_from(self.shapes.id(glyph.image, false)).ok().and_then(|shape| self.digits[shape]))
            .collect()
    }

    /// The "/" and the "(" (unit images): the mean of each in the `lines` that read as digits, the "/", digits and the
    /// "(". None when too few lines did.
    fn find_marks(&mut self, lines: &[u32]) -> Option<[[f32; GLYPH_PIXELS]; 2]> {
        let mut sums = [[0f32; GLYPH_PIXELS]; 2];
        let mut counts = [0; 2];
        let store = self.shapes.store;
        for &line in lines {
            let read = self.by_likeness(line);
            let mut marks_at = read.iter().enumerate().filter(|(_, digit)| digit.is_none()).map(|(at, _)| at);
            let (Some(slash), Some(paren)) = (marks_at.next(), marks_at.next()) else {
                continue;
            };
            if slash == 0 || paren == slash + 1 {
                continue;
            }
            let glyphs: Vec<&Glyph> = store.tall_glyphs(line, self.band).collect();
            for (mark, at) in [slash, paren].into_iter().enumerate() {
                if counts[mark] < MARK_MEAN_LINES {
                    let unit = self.shapes.unit_glyph(glyphs[at].image);
                    sums[mark].iter_mut().zip(&unit).for_each(|(sum, level)| *sum += level);
                    counts[mark] += 1;
                }
            }
        }
        if counts.iter().any(|&count| count < MIN_MARK_LINES) {
            return None;
        }
        Some(sums.map(to_unit))
    }

    /// One Accuracy line's hits and shots. The "/" and the "(" are the tall glyphs that are not digits: hits before the
    /// first, shots between them. Small or limited-range text can leave a digit just under the Kill Count's likeness
    /// (python/hud.py then reads it as a mark, and the line as nothing or as no shot yet), so with the marks known each
    /// glyph is the most alike of the digits, the "/" and the "(" instead.
    fn read(&mut self, line: u32) -> Option<(i64, i64)> {
        let read = match self.marks {
            None => self.by_likeness(line),
            Some(marks) => self.by_marks(line, &marks)?,
        };
        hits_and_shots(&read)
    }

    /// A line's tall glyphs as the most alike of the digits (Some) and the marks (None). None when a glyph of the hits
    /// or shots (before the second mark) is less than DIGIT_LIKENESS alike to it: a digit drawn closer to the next one
    /// (the left 4 of "44", the 7 of "74") is like no learned shape, and the most alike is another digit (an 8, a 1).
    fn by_marks(&self, line: u32, marks: &[[f32; GLYPH_PIXELS]; 2]) -> Option<Vec<Option<u8>>> {
        let prototypes: Vec<([f32; GLYPH_PIXELS], Option<u8>)> = (0..self.digits.len())
            .filter_map(|shape| self.digits[shape].map(|digit| (self.shapes.unit(shape), Some(digit))))
            .chain(marks.iter().map(|&mark| (mark, None)))
            .collect();
        let mut marks_seen = 0;
        let mut read = Vec::new();
        for glyph in self.shapes.store.tall_glyphs(line, self.band) {
            let unit = self.shapes.unit_glyph(glyph.image);
            let (likeness, digit) = prototypes
                .iter()
                .map(|(prototype, digit)| (dot(&unit, prototype), *digit))
                .fold((f64::MIN, None), |best, entry| if entry.0 > best.0 { entry } else { best });
            if digit.is_none() {
                marks_seen += 1;
            } else if marks_seen < 2 && likeness < DIGIT_LIKENESS {
                return None;
            }
            read.push(digit);
        }
        Some(read)
    }
}

/// The hits and shots an Accuracy line's tall glyphs spell (None: a mark): hits before the first mark, shots before the
/// second.
fn hits_and_shots(read: &[Option<u8>]) -> Option<(i64, i64)> {
    let (mut parts, mut current) = (Vec::new(), Vec::new());
    for glyph in read {
        match glyph {
            Some(digit) => current.push(*digit),
            None => parts.push(std::mem::take(&mut current)),
        }
    }
    if parts.len() < 2 {
        None
    } else if parts[0].is_empty() && parts[1].is_empty() {
        // "--/-- ( %)": no shot yet (its tall glyphs are the marks and "%)"; a line of unread digits has more)
        (read.len() <= NO_SHOT_TALL_GLYPHS).then_some((0, 0))
    } else {
        match (value(&parts[0]), value(&parts[1])) {
            (Some(hits), Some(shots)) if hits <= shots => Some((hits, shots)),
            _ => None,
        }
    }
}

/// The Accuracy line's stable (hits, shots) over the frames since..=until.
fn accuracy_stretches(mut reader: AccuracyReader, since: usize, until: usize) -> Vec<Stretch<(i64, i64)>> {
    let lines = reader.shapes.store.per_frame(ACCURACY_ROW);
    let mut window: Vec<u32> = lines[since..=until.min(lines.len() - 1)].to_vec();
    window.sort_unstable();
    window.dedup();
    reader.marks = reader.find_marks(&window);
    let mut read: HashMap<u32, Option<(i64, i64)>> = HashMap::new();
    let readings: Vec<Option<(i64, i64)>> = lines
        .into_iter()
        .enumerate()
        .map(|(frame, line)| {
            if frame < since || frame > until {
                return None;
            }
            *read.entry(line).or_insert_with(|| reader.read(line))
        })
        .collect();
    let mut stretches = stable_stretches(&readings);
    // the Accuracy line is redrawn three times a second, so a run started again first shows the run before's reading
    // for a moment: a first reading that drops right after is that one
    while stretches.len() >= 2 && stretches[1].reading.1 < stretches[0].reading.1 {
        stretches.remove(0);
    }
    stretches
}

/// Each shot's and each hit's frame.
struct ShotFrames {
    /// One frame per shot, in order.
    shots: Vec<i64>,
    /// One frame per hit, in order.
    hits: Vec<i64>,
}

/// Each shot and hit at the frame its Accuracy reading shows it (a step of more than MAX_SHOT_STEP is a misread). When
/// the Kill Count starts at 0 (`from_zero`), the counts of the first reading are the run's first shots and hits
/// (python/hud.py counts only the steps after it).
fn shot_and_hit_frames(accuracy: &[Stretch<(i64, i64)>], from_zero: bool) -> ShotFrames {
    let mut frames = ShotFrames { shots: Vec::new(), hits: Vec::new() };
    if let Some(first) = accuracy.first()
        && from_zero
        && first.reading.1 <= MAX_SHOT_STEP
    {
        let (hits, shots) = first.reading;
        frames.shots.extend(repeat_n(first.first as i64, shots as usize));
        frames.hits.extend(repeat_n(first.first as i64, hits as usize));
    }
    for pair in accuracy.windows(2) {
        let (before, after) = (&pair[0], &pair[1]);
        let (new_hits, new_shots) = (after.reading.0 - before.reading.0, after.reading.1 - before.reading.1);
        if (0..=MAX_SHOT_STEP).contains(&new_hits) && 0 < new_shots && new_shots <= MAX_SHOT_STEP {
            frames.shots.extend(repeat_n(after.first as i64, new_shots as usize));
            frames.hits.extend(repeat_n(after.first as i64, new_hits as usize));
        }
    }
    frames
}

/// The totals (hits, shots) with the late kills. Each kill takes a hit. The Accuracy line is redrawn three times a
/// second, and a run can end before it shows its last kills: those kills' hits (each a shot) are added at their kill
/// frames (python/hud.py gives the last reading).
fn add_late_kills(frames: &mut ShotFrames, totals: Option<(i64, i64)>, kills: &[i64]) -> Option<(i64, i64)> {
    if let Some((hits, shots)) = totals
        && hits < kills.len() as i64
    {
        let late = &kills[kills.len() - (kills.len() as i64 - hits) as usize..];
        frames.hits.extend(late);
        frames.shots.extend(late);
        frames.hits.sort_unstable();
        frames.shots.sort_unstable();
        return Some((hits + late.len() as i64, shots + late.len() as i64));
    }
    totals
}

/// Aim Lab's HUD (python/hud.py: read_aimlab): every hit counted as a kill (one-hit targets). A hit adds points and a
/// miss takes some off, so the POINTS number gives every hit and miss. The digits are learned from the TIME box,
/// which counts down one second at a time (read backwards it counts up, as the Kill Count does).
fn aimlab(store: &Store) -> Option<HudReading> {
    let time_lines = store.per_frame(TIME_ROW);
    let (shapes, digits) = TIME_TRIES.into_iter().find_map(|same| time_digits(store, &time_lines, same))?;
    let points = points_stretches(store, &shapes, &digits);
    let changes: Vec<(i64, usize)> =
        points.windows(2).map(|pair| (pair[1].reading - pair[0].reading, pair[1].first)).collect();
    let (hit, miss) = step_points(&changes)?;
    let (mut hits, mut misses, mut explained) = (Vec::new(), Vec::new(), 0);
    for &(change, frame) in &changes {
        if let Some((step_hits, step_misses)) = step_events(change, hit, miss) {
            hits.extend(repeat_n(frame as i64, step_hits as usize));
            misses.extend(repeat_n(frame as i64, step_misses as usize));
            explained += 1;
        }
    }
    let checked = explained as f64 / changes.len().max(1) as f64;
    if hits.len() < MIN_AIM_HITS || checked < MIN_AIM_CHECKED {
        return None;
    }
    let mut shots: Vec<i64> = hits.iter().chain(&misses).copied().collect();
    shots.sort_unstable();
    Some(HudReading {
        game: HudGame::Aimlab,
        totals: HudFinal {
            kills: hits.len() as i64,
            hits: Some(hits.len() as i64),
            shots: Some((hits.len() + misses.len()) as i64),
        },
        kills: hits.clone(),
        shots,
        hits,
        checked: round_to_thousandths(checked),
        points: points.last().map(|stretch| stretch.reading as f64),
    })
}

/// The shapes and digits learned from Aim Lab's TIME box at one likeness, when it then counts down one second at a
/// time.
fn time_digits<'a>(store: &'a Store, time_lines: &[u32], same: f64) -> Option<(Shapes<'a>, Vec<Option<u8>>)> {
    let mut shapes = Shapes::new(store, same);
    let readings: Vec<Option<Vec<i32>>> = time_lines
        .iter()
        .map(|&line| {
            if store.lines[line as usize].len() != TIME_GLYPHS {
                return None;
            }
            shapes.learn_line(line, AH)
        })
        .collect();
    let stable = stable_stretches(&readings);
    let backwards: Vec<&[i32]> = stable.iter().rev().map(|stretch| stretch.reading.as_slice()).collect();
    let digits = learn_digits(&backwards, shapes.shapes.len())?;
    let seconds: Vec<i64> =
        stable.iter().filter_map(|stretch| number(&stretch.reading, &digits)).map(clock_seconds).collect();
    let steps: Vec<i64> = seconds.windows(2).map(|pair| pair[0] - pair[1]).collect();
    let counts_down = steps.len() >= MIN_TIME_STEPS
        && steps.iter().filter(|&&step| step == 1).count() as f64 >= TIME_STEP_SHARE * steps.len() as f64;
    counts_down.then_some((shapes, digits))
}

/// A clock's m:ss, read as the number mss, in seconds.
fn clock_seconds(clock: i64) -> i64 {
    60 * (clock / 100) + clock % 100
}

/// The POINTS number's stable values. Each glyph is the most alike digit shape (a minus sign is short and wide).
fn points_stretches(store: &Store, shapes: &Shapes, digits: &[Option<u8>]) -> Vec<Stretch<i64>> {
    let prototypes: Vec<([f32; GLYPH_PIXELS], u8)> = digits
        .iter()
        .enumerate()
        .filter_map(|(shape, digit)| digit.map(|digit| (shapes.unit(shape), digit)))
        .collect();
    let mut read: HashMap<u32, Option<i64>> = HashMap::new();
    let values: Vec<Option<i64>> = store
        .per_frame(POINTS_ROW)
        .into_iter()
        .map(|line| *read.entry(line).or_insert_with(|| points(&store.lines[line as usize], shapes, &prototypes)))
        .collect();
    stable_stretches(&values)
}

/// A POINTS line's number; None when a digit is less than DIGIT_LIKENESS alike to every digit shape.
fn points(glyphs: &[Glyph], shapes: &Shapes, prototypes: &[([f32; GLYPH_PIXELS], u8)]) -> Option<i64> {
    let (mut number_digits, mut sign) = (Vec::new(), 1);
    for glyph in glyphs {
        if !tall(glyph, AH) {
            if number_digits.is_empty() && glyph.width as f64 / glyph.height as f64 > MINUS_MIN_ASPECT {
                sign = -1;
            }
            continue;
        }
        let unit = shapes.unit_glyph(glyph.image);
        let (likeness, digit) = prototypes
            .iter()
            .map(|(prototype, digit)| (dot(&unit, prototype), *digit))
            .fold((f64::MIN, 0), |best, entry| if entry >= best { entry } else { best });
        if likeness < DIGIT_LIKENESS {
            number_digits.clear();
            break;
        }
        number_digits.push(digit);
    }
    value(&number_digits).map(|number| sign * number)
}

/// A hit's points and a miss's (None when no step went down): the most common step up and down.
fn step_points(changes: &[(i64, usize)]) -> Option<(i64, Option<i64>)> {
    let (mut ups, mut downs) = (Counter::new(), Counter::new());
    for &(change, _) in changes {
        if change > 0 {
            ups.add(change);
        } else if change < 0 {
            downs.add(change);
        }
    }
    Some((ups.top()?, downs.top()))
}

/// The hits and misses one POINTS change is (two hits, or a hit and a miss, can come in one step), when it is within
/// the tolerance of them.
fn step_events(change: i64, hit: i64, miss: Option<i64>) -> Option<(i64, i64)> {
    let most_misses = if miss.is_some() { MAX_STEP_EVENTS } else { 0 };
    let (error, hits, misses) = (0..=MAX_STEP_EVENTS)
        .flat_map(|hits| (0..=most_misses).map(move |misses| (hits, misses)))
        .filter(|&(hits, misses)| hits + misses > 0)
        .map(|(hits, misses)| ((change - hits * hit - misses * miss.unwrap_or(0)).abs(), hits, misses))
        .min()?;
    (error as f64 <= STEP_TOLERANCE_POINTS.max(STEP_TOLERANCE_SHARE * hit as f64)).then_some((hits, misses))
}

/// Tests of the glyph cutting, the digit learning, the counts' reading and the joining of run parts, on synthetic
/// glyphs and bands.
#[cfg(test)]
mod tests {
    use super::*;

    /// A glyph for `pattern` (0 to 11): two rows of its own lit, so no two patterns are alike.
    fn glyph(pattern: usize, height: u16) -> Cut {
        let mut image = [0u8; GLYPH_PIXELS];
        image[2 * pattern * GLYPH_WIDTH_PX..(2 * pattern + 2) * GLYPH_WIDTH_PX].fill(255);
        Cut { image, height, width: 10 }
    }

    /// The glyphs of a number's digits, each its digit's pattern, `height` pixels high.
    fn line(value: i64, height: u16) -> Vec<Cut> {
        value.to_string().bytes().map(|b| glyph((b - b'0') as usize, height)).collect()
    }

    /// Two digits joined by a bridge are cut in two at the bridge, and the label left of a wide gap is left out.
    #[test]
    fn a_wide_glyph_is_split_and_the_label_left_out() {
        let (width, height) = (200, 30);
        let mut band = vec![60u8; width * height];
        let mut fill = |x0: usize, x1: usize, y0: usize, y1: usize| {
            for y in y0..y1 {
                band[y * width + x0..y * width + x1].fill(230);
            }
        };
        fill(10, 40, 8, 22); // the label
        fill(150, 162, 5, 25); // two digits run together by a one-pixel bridge
        fill(164, 176, 5, 25);
        fill(162, 164, 15, 16);
        let glyphs = value_glyphs(&band, width, None);
        let sizes: Vec<(u16, u16)> = glyphs.iter().map(|glyph| (glyph.width, glyph.height)).collect();
        assert_eq!(sizes, vec![(12, 20), (14, 20)]);
        assert!(glyphs[0].image.iter().all(|&level| level == 255));
    }

    /// Shape ids for the digits 0 to 9, in an order of their own.
    const SHAPE_OF_DIGIT: [i32; 10] = [7, 3, 9, 0, 5, 1, 8, 2, 6, 4];

    /// The shape ids of a number's digits (SHAPE_OF_DIGIT).
    fn shapes_of(value: u32) -> Vec<i32> {
        value.to_string().bytes().map(|b| SHAPE_OF_DIGIT[(b - b'0') as usize]).collect()
    }

    /// A count from 0 to 25 teaches every digit's shape; one that never changes its tens place teaches none.
    #[test]
    fn digits_are_learned_from_a_count() {
        let readings: Vec<Vec<i32>> = (0..=25).map(shapes_of).collect();
        let stable: Vec<&[i32]> = readings.iter().map(|reading| reading.as_slice()).collect();
        let digits = learn_digits(&stable, 10).unwrap();
        for (digit, &shape) in SHAPE_OF_DIGIT.iter().enumerate() {
            assert_eq!(digits[shape as usize], Some(digit as u8));
        }
        assert_eq!(learn_digits(&stable[..8], 10), None); // no tens place changed: no 0
    }

    /// A synthetic HUD (`hud`) with a miss before every fifth kill.
    fn counting(frames: Range<usize>) -> (Store, Layout) {
        hud(frames, |frame, kills| {
            let fifth_kills = (1..=kills).filter(|kill| kill % 5 == 0).count() as i64;
            let misses = fifth_kills + i64::from(kills % 5 == 4 && frame % 10 >= 5);
            (kills, kills + misses)
        })
    }

    /// A store fed with a synthetic HUD: the Kill Count counting to 40, ten frames a value, and Accuracy as
    /// `accuracy(frame, kills)`, hits/shots (percent).
    fn hud(frames: Range<usize>, accuracy: impl Fn(usize, i64) -> (i64, i64)) -> (Store, Layout) {
        hud_drawn(frames, accuracy, |shots| line(shots, 18))
    }

    /// `hud` with the shots drawn by `draw_shots`.
    fn hud_drawn(
        frames: Range<usize>,
        accuracy: impl Fn(usize, i64) -> (i64, i64),
        draw_shots: impl Fn(i64) -> Vec<Cut>,
    ) -> (Store, Layout) {
        let layout = Layout {
            x0: 0,
            x1: 100,
            kills: Row { y0: 0, y1: 20, start: None },
            accuracy: Row { y0: 30, y1: 50, start: None },
        };
        let mut store = Store::default();
        let mut recent: [Capped<Glyph, RECENT_GLYPHS>; ROWS] = Default::default();
        for frame in frames {
            let kills = (frame / 10).min(40) as i64;
            let (hits, shots) = accuracy(frame, kills);
            let mut accuracy_line = line(hits, 18);
            accuracy_line.push(glyph(10, 18)); // "/"
            accuracy_line.extend(draw_shots(shots));
            accuracy_line.push(glyph(11, 18)); // "("
            accuracy_line.extend(line(90, 18));
            let rows = [
                (KILL_COUNT_ROW, line(kills, 18)),
                (ACCURACY_ROW, accuracy_line),
                (POINTS_ROW, Vec::new()),
                (TIME_ROW, Vec::new()),
            ];
            for (row, cuts) in rows {
                store.push(row, cuts, &mut recent[row]);
            }
        }
        (store, layout)
    }

    /// A Kill Count counting to 40 gives a kill every 10 frames, and the Accuracy line the hits, shots and misses.
    #[test]
    fn a_counting_hud_reads_its_kills_and_shots() {
        let (store, layout) = counting(0..430);
        let reading = kovaak(&store, &layout).unwrap();
        assert_eq!(reading.kills, (1..=40).map(|kill| 10 * kill).collect::<Vec<i64>>());
        assert_eq!(reading.totals, HudFinal { kills: 40, hits: Some(40), shots: Some(48) });
        assert_eq!(reading.hits.len(), 40);
        assert_eq!(reading.shots.iter().filter(|&&frame| frame % 10 == 5).count(), 8); // the misses, between kills
        assert_eq!(reading.checked, 1.0);
        assert!(aimlab(&store).is_none());
    }

    /// The run before's reading at the start is dropped, and a last kill the Accuracy line never showed adds its hit.
    #[test]
    fn a_restart_and_an_early_end_still_count_every_hit() {
        // the Accuracy line is redrawn every 7 frames: first it shows the run before's 8/9, and the run ends before it
        // shows the last kill
        let (store, layout) = hud(0..430, |frame, _| {
            let redrawn = (frame / 7 * 7).min(399);
            let kills = (redrawn / 10) as i64;
            if frame < 15 { (8, 9) } else { (kills, kills) }
        });
        let reading = kovaak(&store, &layout).unwrap();
        assert_eq!(reading.totals, HudFinal { kills: 40, hits: Some(40), shots: Some(40) });
        assert_eq!((reading.hits.len(), reading.shots.len()), (40, 40));
        assert_eq!((reading.hits[0], reading.hits[39]), (15, 400)); // the run's first reading, and the last kill
    }

    /// An Accuracy line with a digit less than DIGIT_LIKENESS alike to every digit is not read, so no shot is
    /// miscounted.
    #[test]
    fn a_digit_like_no_digit_spoils_its_accuracy_line() {
        // the left 4 of "44" drawn closer to its neighbor: most like an 8 (0.83), but less than DIGIT_LIKENESS
        let squeezed_four = || {
            let mut cut = glyph(8, 18);
            cut.image[8 * GLYPH_WIDTH_PX..10 * GLYPH_WIDTH_PX].fill(170);
            cut
        };
        let draw_shots = |shots: i64| {
            let mut cuts = line(shots, 18);
            if shots == 44 {
                cuts[0] = squeezed_four();
            }
            cuts
        };
        let misses = |kills: i64| (1..=kills).filter(|kill| kill % 5 == 0).count() as i64;
        let (store, layout) = hud_drawn(0..430, |_, kills| (kills, kills + misses(kills)), draw_shots);
        let reading = kovaak(&store, &layout).unwrap();
        assert_eq!(reading.totals, HudFinal { kills: 40, hits: Some(40), shots: Some(48) });
        assert_eq!((reading.hits.len(), reading.shots.len()), (40, 48));
    }

    /// A glyph that shows for a moment after the number is no step of the count and gets no digit.
    #[test]
    fn a_glyph_after_the_number_teaches_no_digit() {
        let mut readings: Vec<Vec<i32>> = Vec::new();
        for value in 0..=25u32 {
            readings.push(shapes_of(value));
            if value % 4 == 1 {
                let mut junk = readings[readings.len() - 1].clone();
                junk.push(10); // an edge or a panel beside the number, for a moment
                readings.push(junk);
                readings.push(readings[readings.len() - 2].clone());
            }
        }
        let stable: Vec<&[i32]> = readings.iter().map(|reading| reading.as_slice()).collect();
        let digits = learn_digits(&stable, 11).unwrap();
        assert_eq!(digits[10], None);
        assert!(SHAPE_OF_DIGIT.iter().enumerate().all(|(digit, &shape)| digits[shape as usize] == Some(digit as u8)));
    }

    /// Three run parts, each through JSON and back, join into the same glyphs and reading as one watch.
    #[test]
    fn parts_join_as_one_watch() {
        let (whole, layout) = counting(0..430);
        let part = |frames: Range<usize>| HudPart {
            frames: frames.len(),
            layout: Some(layout),
            session: None,
            store: counting(frames).0,
        };
        // each run part but the last also reads the next part's first frame
        let parts = [part(0..201), part(200..301), part(300..430)];
        let mut watch = HudWatch::new(0, 0, true);
        for part in parts {
            let text = serde_json::to_string(&part).unwrap();
            let back: HudPart = serde_json::from_str(&text).unwrap();
            assert_eq!(back, part);
            watch.join(back);
        }
        assert_eq!(watch.frames(), 430);
        let glyphs = |store: &Store, row: usize| -> Vec<Vec<Vec<u8>>> {
            store
                .per_frame(row)
                .into_iter()
                .map(|line| store.lines[line as usize].iter().map(|glyph| store.image(glyph.image).to_vec()).collect())
                .collect()
        };
        for row in 0..ROWS {
            assert_eq!(glyphs(&watch.store, row), glyphs(&whole, row));
        }
        assert_eq!(watch.finish(), kovaak(&whole, &layout));
    }

    /// A part whose rows' frames do not add up to its frames is refused when read from JSON.
    #[test]
    fn a_part_that_does_not_add_up_is_refused() {
        let (store, layout) = counting(0..50);
        let mut text =
            serde_json::to_value(HudPart { frames: 50, layout: Some(layout), session: None, store }).unwrap();
        text["frames"] = 51.into();
        assert!(serde_json::from_value::<HudPart>(text).is_err());
    }

    /// Skipped frames count as frames with the empty line in every row, and give no reading.
    #[test]
    fn skipped_frames_read_nothing() {
        let mut watch = HudWatch::new(64, 36, true);
        watch.skip(5);
        watch.add(&[0u8; 64 * 36]);
        assert_eq!(watch.frames(), 6);
        assert_eq!(watch.store.per_frame(KILL_COUNT_ROW), vec![0; 6]);
        assert_eq!(watch.finish(), None);
    }
}
