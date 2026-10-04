//! A recording's on-screen HUD, read frame by frame for a run without a stats file (python/hud.py's algorithm):
//! KovaaK's session box (Kill Count, and Accuracy as hits/shots) or, when there is none, Aim Lab's POINTS and TIME
//! boxes. It gives the kill frames, the shots and hits, and the run's totals. The digits are learned from the recording
//! itself (no font): KovaaK's Kill Count counts up one kill at a time, Aim Lab's timer down one second at a time.
//!
//! It reads each frame's Y plane (brightness) as the decoder gives it, at the recording's size, never ffmpeg's grey
//! conversion: only the digits read must agree, not the pixels. A limited-range ("tv") recording's Y is stretched to
//! 0..255 first, as ffmpeg's grey conversion does. Each frame keeps only its value rows' glyphs (GW x GH grey images),
//! and a row's glyphs are kept once while they stay the same from frame to frame, so a long recording stays small.
//!
//! KovaaK's stats files are the reference, not python/hud.py: where it misreads and the stats files show it, the
//! reading here departs from it, and each place says so.

use std::collections::HashMap;
use std::ops::Range;

use serde::{Deserialize, Serialize};

/// Where KovaaK's box can be, as shares of the frame (x0, y0, x1, y1): it grows to fit its widest number. The region
/// is scaled to BW x BH (a 2560 x 1440 frame's pixels).
pub(crate) const BOX: [f64; 4] = [0.0, 0.0, 900.0 / 2560.0, 330.0 / 1440.0];
pub(crate) const BW: usize = 900;
pub(crate) const BH: usize = 330;
/// A patch inside the box's left edge, between the header and Kill Count (x, y).
const SEED: (usize, usize) = (49, 100);
/// Glyphs are compared at GW x GH.
const GW: usize = 16;
const GH: usize = 24;
const GLYPH: usize = GW * GH;
/// Glyphs this alike (cosine of their grey images) are the same shape.
const SAME: f64 = 0.97;
/// The box's rows read above and below each value row.
const PAD: usize = 3;
/// Aim Lab's POINTS and TIME value line, as shares of a 16:9 frame, scaled to AW x AH (twice 720p), and the two
/// values' columns in it.
pub(crate) const AIM_BAND: [f64; 4] = [0.30, 40.0 / 720.0, 0.565, 63.0 / 720.0];
pub(crate) const AW: usize = 678;
const AH: usize = 46;
pub(crate) const AIM_POINTS: (usize, usize) = (26, 356);
pub(crate) const AIM_TIME: (usize, usize) = (368, 656);
/// The most key frames kept for the box's layout: past it every other one is dropped (a median needs no more).
const KEYS: usize = 64;
/// A row's glyphs are the ones before while every glyph has the same ink size and differs from the kept image by at
/// most NEAR_MAX at any pixel and NEAR_SUM in all (the box is see-through: the scene behind it moves the grey levels).
const NEAR_MAX: u8 = 24;
const NEAR_SUM: u32 = 3 * GLYPH as u32;
/// The glyphs a row keeps for new lines to reuse.
const RECENT: usize = 32;
/// The rows each frame keeps: KovaaK's Kill Count and Accuracy, Aim Lab's POINTS and TIME.
const KILLS: usize = 0;
const ACCURACY: usize = 1;
const POINTS: usize = 2;
const TIME: usize = 3;
const ROWS: usize = 4;

/// Which game's HUD was read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub enum HudGame {
    Kovaak,
    Aimlab,
}

/// The run's totals as the HUD shows them at the end: the kills counted, and the hits and shots (None where the
/// Accuracy line was not read).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct HudFinal {
    pub kills: i64,
    pub hits: Option<i64>,
    pub shots: Option<i64>,
}

/// What the HUD read (python/hud.py: read() and read_aimlab()). Frames are the recording's frame indexes (0 is the
/// first frame from time 0 on), on the video's own clock.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct HudReading {
    pub game: HudGame,
    /// One entry per kill: the frame its count went up.
    pub kills: Vec<i64>,
    /// One entry per shot, and per hit: the frame its count went up.
    pub shots: Vec<i64>,
    pub hits: Vec<i64>,
    #[serde(rename = "final")]
    pub totals: HudFinal,
    /// The share of the count's steps that were a plausible step (+1 kill, a hit or a miss).
    pub checked: f64,
    /// Aim Lab's points at the end (it shows no score otherwise); None for KovaaK's.
    pub points: Option<f64>,
}

// ---- scaling --------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Filter {
    /// ffmpeg's `area`: the mean of the pixels covered when scaling down, bilinear when scaling up.
    Area,
    /// Bicubic, widened when scaling down (Pillow's).
    Cubic,
}

pub(crate) fn bilinear(x: f64) -> f64 {
    let x = x.abs();
    if x < 1.0 { 1.0 - x } else { 0.0 }
}

fn bicubic(x: f64) -> f64 {
    let (x, a) = (x.abs(), -0.5);
    if x < 1.0 {
        ((a + 2.0) * x - (a + 3.0)) * x * x + 1.0
    } else if x < 2.0 {
        (((x - 5.0) * x + 8.0) * x - 4.0) * a
    } else {
        0.0
    }
}

/// Pillow's resampling weights (precompute_coeffs) for `len` pixels scaled to `out`: each output pixel's first source
/// pixel and weights.
pub(crate) fn taps(len: usize, out: usize, support: f64, filter: fn(f64) -> f64) -> Vec<(usize, Vec<f64>)> {
    let scale = len as f64 / out as f64;
    let fs = scale.max(1.0);
    let (support, ss) = (support * fs, 1.0 / fs);
    (0..out)
        .map(|i| {
            let center = (i as f64 + 0.5) * scale;
            let lo = ((center - support + 0.5) as i64).max(0) as usize;
            let hi = ((center + support + 0.5) as i64).min(len as i64).max(lo as i64) as usize;
            let mut w: Vec<f64> = (lo..hi).map(|x| filter((x as f64 - center + 0.5) * ss)).collect();
            let sum: f64 = w.iter().sum();
            if sum != 0.0 {
                w.iter_mut().for_each(|v| *v /= sum);
            }
            (lo, w)
        })
        .collect()
}

/// One axis of a crop of the frame scaled to a size: each output pixel's first source pixel and weights.
struct Axis {
    taps: Vec<(usize, Vec<f32>)>,
    identity: bool,
}

impl Axis {
    /// An axis of `size` pixels cropped from share `from` to share `to` as ffmpeg's crop does (the size rounded, then
    /// both made even for the chroma), scaled to `out` pixels.
    fn new(size: usize, from: f64, to: f64, out: usize, filter: Filter) -> Axis {
        let start = (((size as f64 * from) as usize) & !1).min(size.saturating_sub(1));
        let len = (((size as f64 * (to - from)).round_ties_even() as usize) & !1).clamp(1, (size - start).max(1));
        let s = len as f64 / out as f64;
        let mut cubic = match filter {
            Filter::Cubic if len != out => taps(len, out, 2.0, bicubic),
            _ => Vec::new(),
        };
        let taps = (0..out)
            .map(|i| {
                let (first, w): (usize, Vec<f64>) = match filter {
                    _ if len == out => (i, vec![1.0]),
                    Filter::Area if len > out => {
                        let (a, b) = (i as f64 * s, (i + 1) as f64 * s);
                        let first = a.floor() as usize;
                        let last = (b.ceil() as usize).min(len);
                        (first, (first..last).map(|p| (b.min(p as f64 + 1.0) - a.max(p as f64)) / s).collect())
                    }
                    Filter::Area if len == 1 => (0, vec![1.0]),
                    Filter::Area => {
                        let c = ((i as f64 + 0.5) * s - 0.5).clamp(0.0, (len - 1) as f64);
                        let p = (c.floor() as usize).min(len - 2);
                        (p, vec![1.0 - (c - p as f64), c - p as f64])
                    }
                    Filter::Cubic => std::mem::take(&mut cubic[i]),
                };
                (start + first, w.into_iter().map(|v| v as f32).collect())
            })
            .collect();
        Axis { taps, identity: len == out }
    }
}

/// A crop of the frame scaled to a size.
struct Scale {
    x: Axis,
    y: Axis,
    stride: usize,
}

impl Scale {
    fn new(width: usize, height: usize, share: [f64; 4], (w, h): (usize, usize), filter: Filter) -> Scale {
        Scale {
            x: Axis::new(width, share[0], share[2], w, filter),
            y: Axis::new(height, share[1], share[3], h, filter),
            stride: width,
        }
    }

    /// The scaled crop's pixels in `rows` x `cols` (row by row), each source byte through `lut`.
    fn rect(&self, plane: &[u8], lut: &[u8; 256], rows: Range<usize>, cols: Range<usize>) -> Vec<u8> {
        let mut out = Vec::with_capacity(rows.len() * cols.len());
        if self.x.identity && self.y.identity {
            let x0 = self.x.taps[cols.start].0;
            for oy in rows {
                let at = self.y.taps[oy].0 * self.stride + x0;
                out.extend(plane[at..at + cols.len()].iter().map(|&v| lut[v as usize]));
            }
            return out;
        }
        let c0 = cols.clone().map(|x| self.x.taps[x].0).min().unwrap_or(0);
        let c1 = cols.clone().map(|x| self.x.taps[x].0 + self.x.taps[x].1.len()).max().unwrap_or(c0);
        let mut tmp = vec![0f32; c1 - c0];
        for oy in rows {
            tmp.fill(0.0);
            let (first, w) = &self.y.taps[oy];
            for (k, &wk) in w.iter().enumerate() {
                let at = (first + k) * self.stride;
                for (t, &v) in tmp.iter_mut().zip(&plane[at + c0..at + c1]) {
                    *t += wk * lut[v as usize] as f32;
                }
            }
            for ox in cols.clone() {
                let (first, w) = &self.x.taps[ox];
                let v: f32 = w.iter().zip(&tmp[first - c0..]).map(|(&a, &b)| a * b).sum();
                out.push(v.round().clamp(0.0, 255.0) as u8);
            }
        }
        out
    }
}

/// A glyph's strength image (`w` x `h`) scaled to GW x GH as Pillow's bilinear resize does, as bytes (0 to 255).
fn glyph_image(src: &[f32], w: usize, h: usize) -> [u8; GLYPH] {
    let (tx, ty) = (taps(w, GW, 1.0, bilinear), taps(h, GH, 1.0, bilinear));
    let mut tmp = vec![0f32; h * GW];
    for y in 0..h {
        for (x, (first, wt)) in tx.iter().enumerate() {
            let row = &src[y * w + first..];
            tmp[y * GW + x] = wt.iter().zip(row).map(|(&a, &b)| a * b as f64).sum::<f64>() as f32;
        }
    }
    let mut out = [0u8; GLYPH];
    for (y, (first, wt)) in ty.iter().enumerate() {
        for x in 0..GW {
            let v: f64 = wt.iter().enumerate().map(|(k, &a)| a * tmp[(first + k) * GW + x] as f64).sum();
            out[y * GW + x] = (v as f32 * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

// ---- levels and spans -----------------------------------------------------------------------------------------------

/// The k-th smallest value of a histogram's values (the bin's index).
fn nth(hist: &[u32], k: usize) -> usize {
    let mut seen = 0;
    for (v, &c) in hist.iter().enumerate() {
        seen += c as usize;
        if seen > k {
            return v;
        }
    }
    hist.len() - 1
}

/// numpy's percentile (linear) of a histogram's `n` values, as a bin index.
fn percentile(hist: &[u32], n: usize, q: f64) -> f64 {
    let at = q / 100.0 * (n - 1) as f64;
    let lo = at.floor() as usize;
    let (a, b) = (nth(hist, lo) as f64, nth(hist, (lo + 1).min(n - 1)) as f64);
    a + (at - lo as f64) * (b - a)
}

/// Twice the median of bytes from their histogram (an even count's median can fall between two values).
fn median2(hist: &[u32; 256], n: usize) -> i32 {
    if n % 2 == 1 { 2 * nth(hist, n / 2) as i32 } else { (nth(hist, n / 2 - 1) + nth(hist, n / 2)) as i32 }
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

/// numpy's percentile (linear) of sorted values.
fn percentile_sorted(v: &[f64], q: f64) -> f64 {
    let at = q / 100.0 * (v.len() - 1) as f64;
    let lo = at.floor() as usize;
    let (a, b) = (v[lo], v[(lo + 1).min(v.len() - 1)]);
    a + (at - lo as f64) * (b - a)
}

/// Text pixels of an image (python/hud.py: _ink, which reads the levels as whole numbers): far from its most common
/// level (the box), in either direction.
fn ink(img: &[f64]) -> Vec<bool> {
    let img: Vec<f64> = img.iter().map(|v| v.trunc()).collect();
    let bg = median(&mut img.clone());
    let mut d: Vec<f64> = img.iter().map(|v| (v - bg).abs()).collect();
    d.sort_by(f64::total_cmp);
    let thr = 25f64.max(0.5 * percentile_sorted(&d, 99.5));
    img.iter().map(|v| (v - bg).abs() > thr).collect()
}

/// Runs of true, as (start, end), at least `min_len` long.
fn spans(mask: impl IntoIterator<Item = bool>, min_len: usize) -> Vec<(usize, usize)> {
    let (mut out, mut cur, mut n) = (Vec::new(), None, 0);
    for (i, v) in mask.into_iter().enumerate() {
        match (v, cur) {
            (true, None) => cur = Some(i),
            (false, Some(c)) => {
                if i - c >= min_len {
                    out.push((c, i));
                }
                cur = None;
            }
            _ => {}
        }
        n = i + 1;
    }
    if let Some(c) = cur
        && n - c >= min_len
    {
        out.push((c, n));
    }
    out
}

// ---- KovaaK's box ---------------------------------------------------------------------------------------------------

/// A value row of KovaaK's box: the band read (rows y0..y1 of the scaled region: the row and PAD around it), and where
/// its value can start, from the box's x0 (the compact HUD: just past its label's colon; None: the rightmost group).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct Row {
    y0: usize,
    y1: usize,
    start: Option<usize>,
}

/// Where KovaaK's box has its values: the columns x0..x1 of the scaled region, and the Kill Count and Accuracy rows.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct Layout {
    x0: usize,
    x1: usize,
    kills: Row,
    accuracy: Row,
}

/// KovaaK's session box as python/hud.py's layout finds it, for the area finder (src/areas.rs): the columns of its text
/// rows and the top of the first row and the bottom of the last, in the scaled region (BW x BH: the pixels of a
/// 2560 x 1440 frame, from its top left corner).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRows {
    pub x0: usize,
    pub y0: usize,
    pub x1: usize,
    pub y1: usize,
}

/// The column just past the first colon in a row of text (`w` wide): a narrow glyph made of dots only (an i has a
/// stem; the colon's upper dot can fade in a blurred recording).
fn label_end(ink: &[bool], w: usize) -> Option<usize> {
    let h = ink.len() / w;
    let hf = h as f64;
    spans((0..w).map(|x| (0..h).any(|y| ink[y * w + x])), 1).into_iter().find_map(|(a, b)| {
        let runs = spans((0..h).map(|y| ink[y * w + a..y * w + b].iter().any(|&v| v)), 1);
        let narrow = (b - a) as f64 <= 3f64.max(0.3 * hf);
        (narrow && !runs.is_empty() && runs.iter().all(|&(r0, r1)| (r1 - r0) as f64 <= 0.35 * hf)).then_some(b)
    })
}

/// The box's value rows from the key frames' median (BW x BH), or None without a box (python/hud.py: layout, and
/// read's choice of rows), and the box's text rows (None without a box). The median keeps the box and its labels and
/// washes out the moving scene and the changing numbers. The box is the area at the level of a patch inside its left
/// edge; other players' HUDs are smaller or placed elsewhere, so other patches are tried until one gives a box. A
/// compact box whose labels' colons are not found has text rows but no value rows (python/hud.py's layout gives its
/// rows; its read then reads the rightmost glyphs, which this reading does not).
fn layout(med: &[f64]) -> (Option<Layout>, Option<SessionRows>) {
    let seeds =
        std::iter::once(SEED).chain((40..240).step_by(12).flat_map(|y| (20..320).step_by(12).map(move |x| (x, y))));
    let mut tried = Vec::new();
    let mut lab = vec![0u32; BW * BH];
    let mut stack = Vec::new();
    for (sx, sy) in seeds {
        let patch: [usize; 64] = std::array::from_fn(|k| (sy + k / 8) * BW + sx + k % 8);
        let level = median(&mut patch.map(|i| med[i]));
        let near = |i: usize| (med[i] - level).abs() < 15.0;
        if (patch.iter().filter(|&&i| near(i)).count() as f64) < 0.9 * 64.0 {
            continue;
        }
        // the components of the level's area that the patch is in (4-connected, as ndimage.label); the box is the one
        // with the most of the patch (ties: the first in raster order, as ndimage numbers them)
        lab.fill(0);
        let mut comps: Vec<(usize, usize, [usize; 4], usize)> = Vec::new(); // (first pixel, pixels, box, in patch)
        for &p in &patch {
            if !near(p) || lab[p] != 0 {
                continue;
            }
            let id = comps.len() as u32 + 1;
            let (mut first, mut pixels, mut bb) = (p, 0, [p / BW, p / BW + 1, p % BW, p % BW + 1]);
            lab[p] = id;
            stack.push(p);
            while let Some(q) = stack.pop() {
                let (y, x) = (q / BW, q % BW);
                first = first.min(q);
                pixels += 1;
                bb = [bb[0].min(y), bb[1].max(y + 1), bb[2].min(x), bb[3].max(x + 1)];
                let mut visit = |r: usize| {
                    if lab[r] == 0 && near(r) {
                        lab[r] = id;
                        stack.push(r);
                    }
                };
                if y > 0 {
                    visit(q - BW);
                }
                if y + 1 < BH {
                    visit(q + BW);
                }
                if x > 0 {
                    visit(q - 1);
                }
                if x + 1 < BW {
                    visit(q + 1);
                }
            }
            comps.push((first, pixels, bb, 0));
        }
        for &p in &patch {
            if lab[p] != 0 {
                comps[lab[p] as usize - 1].3 += 1;
            }
        }
        let Some(&(_, pixels, [by0, by1, bx0, bx1], _)) = comps.iter().max_by(|a, b| a.3.cmp(&b.3).then(b.0.cmp(&a.0)))
        else {
            continue;
        };
        if tried.contains(&(by0, bx0))
            || ((by1 - by0) as f64) < 0.3 * BH as f64
            || ((bx1 - bx0) as f64) < 0.2 * BW as f64
            || by1 >= BH - 2
            || bx1 >= BW - 2
        {
            continue; // too small, or the open scene running off the region
        }
        tried.push((by0, bx0));
        if (pixels as f64) < 0.6 * ((by1 - by0) * (bx1 - bx0)) as f64 {
            continue; // a box is a filled rectangle (with text holes)
        }
        let m = 4; // keep off the box's rounded edge
        let (x0, x1) = (bx0 + m, bx1 - m);
        let sub = |r0: usize, r1: usize| -> Vec<f64> {
            (r0..r1).flat_map(|y| med[y * BW + x0..y * BW + x1].iter().copied()).collect()
        };
        let w = x1 - x0;
        let box_ink = ink(&sub(by0 + m, by1 - m));
        let rows: Vec<(usize, usize)> = spans(box_ink.chunks(w).map(|r| r.iter().filter(|&&v| v).count() > 2), 6)
            .into_iter()
            .map(|(a, b)| (a + by0 + m, b + by0 + m))
            .collect();
        // a header (SESSION and the clock) and six rows under it: Kill Count is the first of them, Accuracy the third.
        // The compact HUD has four rows in two columns (Kill Count and SPM, Accuracy, Damage, Avg TTK and KPS), each
        // value just after its label's colon
        if rows.len() >= 4 {
            let session = Some(SessionRows { x0, y0: rows[0].0, x1, y1: rows[rows.len() - 1].1 });
            let compact = rows.len() == 5;
            let (k, a) = if compact { (rows[1], rows[2]) } else { (rows[1], rows[3]) };
            let start = |(r0, r1): (usize, usize)| {
                if compact { label_end(&ink(&sub(r0.saturating_sub(3), (r1 + 3).min(BH))), w) } else { None }
            };
            let (ks, accs) = (start(k), start(a));
            if compact && (ks.is_none() || accs.is_none()) {
                return (None, session);
            }
            let row =
                |(r0, r1): (usize, usize), start| Row { y0: r0.saturating_sub(PAD), y1: (r1 + PAD).min(BH), start };
            return (Some(Layout { x0, x1, kills: row(k, ks), accuracy: row(a, accs) }), session);
        }
    }
    (None, None)
}

/// A glyph cut from a frame: its strength image, and its ink's height and width in pixels.
#[derive(Clone, Debug, PartialEq)]
struct Cut {
    image: [u8; GLYPH],
    h: u16,
    w: u16,
}

/// The glyph at columns a..b of a band (`w` wide), cropped to its ink rows, or None when it has no ink.
fn cut(strength: &[f32], ink: &[bool], w: usize, (a, b): (usize, usize)) -> Option<Cut> {
    let h = ink.len() / w;
    let rows: Vec<usize> = (0..h).filter(|&y| ink[y * w + a..y * w + b].iter().any(|&v| v)).collect();
    let (y0, y1) = (*rows.first()?, *rows.last()? + 1);
    let src: Vec<f32> = (y0..y1).flat_map(|y| strength[y * w + a..y * w + b].iter().copied()).collect();
    Some(Cut { image: glyph_image(&src, b - a, y1 - y0), h: (y1 - y0) as u16, w: (b - a) as u16 })
}

/// The value's glyphs in one row of KovaaK's box (a band `w` wide, python/hud.py: _value_glyphs): the rightmost group
/// of ink columns, cut from the label by a wide gap (or, given start, the first group from there on).
fn value_glyphs(band: &[u8], w: usize, start: Option<usize>) -> Vec<Cut> {
    let n = band.len();
    if n == 0 || w == 0 {
        return Vec::new();
    }
    let h = n / w;
    let mut hist = [0u32; 256];
    band.iter().for_each(|&v| hist[v as usize] += 1);
    let bg2 = median2(&hist, n);
    let mut dist = [0u32; 511];
    hist.iter().enumerate().for_each(|(v, &c)| dist[(2 * v as i32 - bg2).unsigned_abs() as usize] += c);
    let p = percentile(&dist, n, 99.5) / 2.0;
    let (thr, top) = (25f64.max(0.5 * p), 25f64.max(p));
    let d = |v: u8| (2 * v as i32 - bg2).unsigned_abs() as f64 / 2.0;
    let is_ink: [bool; 256] = std::array::from_fn(|v| d(v as u8) > thr);
    let level: [f32; 256] = std::array::from_fn(|v| (d(v as u8) / top).clamp(0.0, 1.0) as f32);
    let ink: Vec<bool> = band.iter().map(|&v| is_ink[v as usize]).collect();
    let strength: Vec<f32> = band.iter().map(|&v| level[v as usize]).collect();
    let mut col = vec![0usize; w];
    ink.chunks(w).for_each(|r| r.iter().zip(&mut col).for_each(|(&v, c)| *c += v as usize));
    // the box widens as its numbers grow: past its right edge the scene fills whole columns with ink, which text never
    // does, or at least leaves no pixel of its columns at the box's level up to the band's end (text leaves the rows
    // above and below it, and a scene line seen through the box is a few columns wide). python/hud.py tested only the
    // first, and read a scene of middle levels past a box narrower than at most key frames as glyphs
    let past = |x: usize| x as f64 > 0.2 * w as f64;
    let solid = (0..w).find(|&x| past(x) && col[x] as f64 / h as f64 > 0.85).unwrap_or(w);
    let far = |x: &usize| (0..h).all(|y| d(band[y * w + x]) > thr / 2.0);
    let scene = (0..w).rev().take_while(far).last().filter(|&x| past(x) && x + 2 <= w).unwrap_or(w);
    let end = solid.min(scene);
    let mut cols = spans(col[..end].iter().map(|&c| c > 0), 1);
    // ink that runs into the scene past the box's edge is the edge's blended border, not text, which keeps a margin
    // inside the box (python/hud.py read it as a glyph: a box narrower than at most key frames, at a run's start)
    if end < w && cols.last().is_some_and(|c| c.1 + 2 >= end) {
        cols.pop();
    }
    let gap = 0.07 * w as f64;
    let group: Vec<(usize, usize)> = match start {
        Some(s) => {
            let mut g: Vec<(usize, usize)> = Vec::new();
            for c in cols.into_iter().filter(|c| c.0 >= s) {
                if g.last().is_some_and(|l| (c.0 - l.1) as f64 > gap) {
                    break; // the gap before the next label
                }
                g.push(c);
            }
            g
        }
        None => {
            let mut g: Vec<(usize, usize)> = Vec::new();
            for c in cols.into_iter().rev() {
                if g.last().is_some_and(|f| (f.0 - c.1) as f64 > gap) {
                    break; // the gap between the label and the value
                }
                g.push(c);
            }
            g.reverse();
            g
        }
    };
    // small, blurred text joins neighboring digits. A digit is at most 0.9 times as wide as it is tall, so a wider
    // glyph is split at its thinnest columns, one piece per 0.6 of its height
    let mut pieces = Vec::new();
    for (a, b) in group {
        let rows: Vec<usize> = (0..h).filter(|&y| ink[y * w + a..y * w + b].iter().any(|&v| v)).collect();
        let gh = (rows[rows.len() - 1] - rows[0] + 1) as f64;
        let width = (b - a) as f64;
        let k = (width / gh / 0.6).round_ties_even() as usize;
        if width / gh < 1.0 || k < 2 {
            pieces.push((a, b));
            continue;
        }
        let mut cuts = vec![a];
        for j in 1..k {
            let c = (b - a) as f64 * j as f64 / k as f64;
            let half = 0.25 * width / k as f64;
            let (lo, hi) = ((c - half) as usize, ((c + half) as usize + 1).min(b - a));
            let at = (lo..hi).fold(lo, |m, i| if col[a + i] < col[a + m] { i } else { m });
            cuts.push(a + at);
        }
        cuts.push(b);
        pieces.extend(cuts.windows(2).filter(|p| p[1] > p[0]).map(|p| (p[0], p[1])));
    }
    pieces.into_iter().filter_map(|p| cut(&strength, &ink, w, p)).collect()
}

// ---- Aim Lab's boxes ------------------------------------------------------------------------------------------------

/// The white value's glyphs in one of Aim Lab's boxes (columns c0..c1 of the band, python/hud.py: _aim_glyphs), the
/// colon left out.
fn aim_glyphs(band: &[u8], (c0, c1): (usize, usize)) -> Vec<Cut> {
    let w = c1 - c0;
    let mut hist = [0u32; 256];
    band.chunks(AW).for_each(|r| r[c0..c1].iter().for_each(|&v| hist[v as usize] += 1));
    let n = w * AH;
    let bg2 = median2(&hist, n);
    // the distance from the median, doubled and signed (white text is above it), offset by 510
    let mut dist = [0u32; 1021];
    hist.iter().enumerate().for_each(|(v, &c)| dist[(2 * v as i32 - bg2 + 510) as usize] += c);
    let top = (percentile(&dist, n, 99.5) - 510.0) / 2.0;
    if top < 40.0 {
        return Vec::new();
    }
    let d = |v: u8| (2 * v as i32 - bg2) as f64 / 2.0;
    let sub: Vec<u8> = band.chunks(AW).flat_map(|r| r[c0..c1].iter().copied()).collect();
    let ink: Vec<bool> = sub.iter().map(|&v| d(v) > 0.5 * top).collect();
    let strength: Vec<f32> = sub.iter().map(|&v| (d(v) / top).clamp(0.0, 1.0) as f32).collect();
    spans((0..w).map(|x| (0..AH).any(|y| ink[y * w + x])), 1)
        .into_iter()
        .filter(|&(a, c)| {
            let runs = spans((0..AH).map(|y| ink[y * w + a..y * w + c].iter().any(|&v| v)), 1);
            let gh = runs.last().map_or(0, |r| r.1) - runs.first().map_or(0, |r| r.0);
            !(runs.len() >= 2 && (c - a) as f64 <= 0.5 * gh as f64) // the colon: two dots
        })
        .filter_map(|p| cut(&strength, &ink, w, p))
        .collect()
}

// ---- what each frame keeps ------------------------------------------------------------------------------------------

/// A stored glyph: its image's index, and its ink's height and width.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Glyph {
    image: u32,
    h: u16,
    w: u16,
}

/// The glyphs each frame read in each row: the distinct lines of glyphs, and each row's line frame by frame.
#[derive(Clone, Debug, PartialEq)]
struct Store {
    /// The glyph images, GLYPH bytes each (the ink's strength, 0 to 255).
    images: Vec<u8>,
    /// The distinct lines of glyphs; line 0 is the empty one.
    lines: Vec<Vec<Glyph>>,
    /// Each row's lines frame by frame, as runs (line, frames).
    rows: [Vec<(u32, u32)>; ROWS],
}

impl Default for Store {
    fn default() -> Store {
        Store { images: Vec::new(), lines: vec![Vec::new()], rows: Default::default() }
    }
}

fn near(a: &[u8], b: &[u8]) -> bool {
    let mut sum = 0;
    for (&x, &y) in a.iter().zip(b) {
        let d = x.abs_diff(y);
        if d > NEAR_MAX {
            return false;
        }
        sum += d as u32;
    }
    sum <= NEAR_SUM
}

impl Store {
    fn image(&self, i: u32) -> &[u8] {
        &self.images[i as usize * GLYPH..(i as usize + 1) * GLYPH]
    }

    fn add_run(&mut self, row: usize, line: u32, frames: u32) {
        match self.rows[row].last_mut() {
            Some(r) if r.0 == line => r.1 += frames,
            _ if frames > 0 => self.rows[row].push((line, frames)),
            _ => {}
        }
    }

    /// One frame's glyphs in a row: the row's line before when they are the same glyphs, else a new line, whose
    /// glyphs reuse the images of the row's `recent` glyphs they are the same as (a number changes a digit at a time).
    fn push(&mut self, row: usize, cuts: Vec<Cut>, recent: &mut Vec<Glyph>) {
        let last = self.rows[row].last().map_or(0, |r| r.0);
        let same = |s: &Store| {
            let l = &s.lines[last as usize];
            l.len() == cuts.len()
                && l.iter().zip(&cuts).all(|(g, c)| g.h == c.h && g.w == c.w && near(s.image(g.image), &c.image))
        };
        let line = if cuts.is_empty() {
            0
        } else if same(self) {
            last
        } else {
            let line = cuts
                .iter()
                .map(|c| {
                    let same = |g: &&Glyph| g.h == c.h && g.w == c.w && near(self.image(g.image), &c.image);
                    if let Some(&g) = recent.iter().rev().find(same) {
                        return g;
                    }
                    self.images.extend_from_slice(&c.image);
                    let g = Glyph { image: (self.images.len() / GLYPH - 1) as u32, h: c.h, w: c.w };
                    if recent.len() == RECENT {
                        recent.remove(0);
                    }
                    recent.push(g);
                    g
                })
                .collect();
            self.lines.push(line);
            (self.lines.len() - 1) as u32
        };
        self.add_run(row, line, 1);
    }

    /// A row's line in each frame.
    fn per_frame(&self, row: usize) -> Vec<u32> {
        self.rows[row].iter().flat_map(|&(l, n)| std::iter::repeat_n(l, n as usize)).collect()
    }

    /// The next run's store after this one's, its first `drop` frames left out.
    fn append(&mut self, next: Store, drop: usize) {
        let images = (self.images.len() / GLYPH) as u32;
        let lines = self.lines.len() as u32 - 1;
        self.images.extend(next.images);
        self.lines.extend(
            next.lines
                .into_iter()
                .skip(1)
                .map(|l| l.into_iter().map(|g| Glyph { image: g.image + images, ..g }).collect()),
        );
        for (row, runs) in next.rows.into_iter().enumerate() {
            let mut drop = drop as u32;
            for (l, n) in runs {
                let left = n - drop.min(n);
                drop -= n - left;
                self.add_run(row, if l == 0 { 0 } else { l + lines }, left);
            }
        }
    }
}

// ---- the watch ------------------------------------------------------------------------------------------------------

/// A run's part of the watch (a review split into runs: each run's watch reads its own frames, the page joins them).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "PartText", into = "PartText")]
pub struct HudPart {
    frames: usize,
    /// KovaaK's box, None without one (each run's watch works it out from the same key frames).
    layout: Option<Layout>,
    /// The box's text rows, None without a box.
    session: Option<SessionRows>,
    store: Store,
}

/// What a watch reads in the key frames (`HudWatch::keys`): KovaaK's box, None without one, and its text rows. Each
/// run's watch starts from it (`HudWatch::from_keys`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HudKeys {
    layout: Option<Layout>,
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
    frames: usize,
    layout: Option<Layout>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session: Option<SessionRows>,
    /// The glyph images, GLYPH bytes each, as hex.
    images: String,
    /// Each line's glyphs after the empty line 0: [image, ink height, ink width].
    lines: Vec<Vec<[u32; 3]>>,
    /// Each row's runs: [line, frames].
    rows: [Vec<[u32; 2]>; ROWS],
}

impl From<HudPart> for PartText {
    fn from(p: HudPart) -> PartText {
        PartText {
            frames: p.frames,
            layout: p.layout,
            session: p.session,
            images: p.store.images.iter().map(|b| format!("{b:02x}")).collect(),
            lines: p.store.lines[1..]
                .iter()
                .map(|l| l.iter().map(|g| [g.image, g.h as u32, g.w as u32]).collect())
                .collect(),
            rows: p.store.rows.map(|r| r.into_iter().map(|(l, n)| [l, n]).collect()),
        }
    }
}

impl TryFrom<PartText> for HudPart {
    type Error = String;

    fn try_from(t: PartText) -> Result<HudPart, String> {
        let images = (0..t.images.len())
            .step_by(2)
            .map(|i| t.images.get(i..i + 2).and_then(|b| u8::from_str_radix(b, 16).ok()))
            .collect::<Option<Vec<u8>>>()
            .filter(|b| b.len() % GLYPH == 0)
            .ok_or("a HUD part's images are not hex glyphs")?;
        let count = (images.len() / GLYPH) as u32;
        let mut lines = vec![Vec::new()];
        for l in t.lines {
            let line = l
                .into_iter()
                .map(|[image, h, w]| {
                    (image < count && h <= u16::MAX as u32 && w <= u16::MAX as u32).then_some(Glyph {
                        image,
                        h: h as u16,
                        w: w as u16,
                    })
                })
                .collect::<Option<Vec<Glyph>>>()
                .ok_or("a HUD part's line has a glyph it does not have")?;
            lines.push(line);
        }
        let mut rows: [Vec<(u32, u32)>; ROWS] = Default::default();
        for (row, runs) in rows.iter_mut().zip(t.rows) {
            if runs.iter().any(|&[l, _]| l as usize >= lines.len())
                || runs.iter().map(|&[_, n]| n as usize).sum::<usize>() != t.frames
            {
                return Err("a HUD part's rows do not match its lines or frames".into());
            }
            *row = runs.into_iter().map(|[l, n]| (l, n)).collect();
        }
        Ok(HudPart { frames: t.frames, layout: t.layout, session: t.session, store: Store { images, lines, rows } })
    }
}

/// Reads a recording's HUD: first every key frame (`add_key`, for where KovaaK's box and its rows are), then every
/// frame in order (`add`), then `finish`. A review split into runs gives each run a watch that reads every key frame
/// and then only its run's frames (and the next run's first, as the camera watch does); `part` and `join` put them
/// together.
pub struct HudWatch {
    width: usize,
    height: usize,
    /// The Y levels as read: a limited-range recording's stretched to 0..255.
    lut: [u8; 256],
    region: Scale,
    aim: Scale,
    /// The key frames' box regions (BW x BH), every `key_step`-th of the `keys_seen`.
    keys: Vec<Vec<u8>>,
    keys_seen: usize,
    key_step: usize,
    /// KovaaK's box: None until worked out (at the first frame), then Some(None) when there is none.
    layout: Option<Option<Layout>>,
    /// The box's text rows, worked out with `layout`.
    session: Option<SessionRows>,
    frames: usize,
    store: Store,
    /// Each row's latest new glyphs, whose images a new line can reuse.
    recent: [Vec<Glyph>; ROWS],
}

impl HudWatch {
    /// For a recording whose frames are `width` x `height`; `full_range`: its Y spans 0..255 (else 16..235).
    pub fn new(width: usize, height: usize, full_range: bool) -> HudWatch {
        let mut lut = [0u8; 256];
        for (v, l) in lut.iter_mut().enumerate() {
            *l = if full_range { v as u8 } else { ((v as f64 - 16.0) * 255.0 / 219.0).round().clamp(0.0, 255.0) as u8 };
        }
        HudWatch {
            width,
            height,
            lut,
            region: Scale::new(width, height, BOX, (BW, BH), Filter::Area),
            aim: Scale::new(width, height, AIM_BAND, (AW, AH), Filter::Cubic),
            keys: Vec::new(),
            keys_seen: 0,
            key_step: 1,
            layout: None,
            session: None,
            frames: 0,
            store: Store::default(),
            recent: Default::default(),
        }
    }

    fn readable(&self, y: &[u8]) -> bool {
        self.width > 0 && self.height > 0 && y.len() >= self.width * self.height
    }

    /// One key frame's Y plane (`width` x `height` bytes), in order, before any frame is added.
    pub fn add_key(&mut self, y: &[u8]) {
        if self.layout.is_some() || !self.readable(y) {
            return;
        }
        if self.keys_seen.is_multiple_of(self.key_step) {
            self.keys.push(self.region.rect(y, &self.lut, 0..BH, 0..BW));
            if self.keys.len() > KEYS {
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
        let (layout, session) = if keys.len() < 3 {
            (None, None)
        } else {
            let mut px = vec![0u8; keys.len()];
            let med: Vec<f64> = (0..BW * BH)
                .map(|i| {
                    px.iter_mut().zip(&keys).for_each(|(p, k)| *p = k[i]);
                    px.sort_unstable();
                    let n = px.len();
                    if n % 2 == 1 { px[n / 2] as f64 } else { (px[n / 2 - 1] as f64 + px[n / 2] as f64) / 2.0 }
                })
                .collect();
            layout(&med)
        };
        self.layout = Some(layout);
        self.session = session;
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
    pub fn add(&mut self, y: &[u8]) {
        self.work_out_layout();
        let mut cuts: [Vec<Cut>; ROWS] = Default::default();
        if self.readable(y) {
            if let Some(Some(l)) = self.layout {
                for (row, r) in [(KILLS, l.kills), (ACCURACY, l.accuracy)] {
                    let band = self.region.rect(y, &self.lut, r.y0..r.y1, l.x0..l.x1);
                    cuts[row] = value_glyphs(&band, l.x1 - l.x0, r.start);
                }
            }
            let band = self.aim.rect(y, &self.lut, 0..AH, 0..AW);
            cuts[POINTS] = aim_glyphs(&band, AIM_POINTS);
            cuts[TIME] = aim_glyphs(&band, AIM_TIME);
        }
        for ((row, c), recent) in cuts.into_iter().enumerate().zip(&mut self.recent) {
            self.store.push(row, c, recent);
        }
        self.frames += 1;
    }

    /// The frames read so far (skipped ones included).
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// The run's part of the watch.
    pub fn part(mut self) -> HudPart {
        self.work_out_layout();
        HudPart { frames: self.frames, layout: self.layout.flatten(), session: self.session, store: self.store }
    }

    /// The next run's part. Each run but the last also reads the next run's first frame, so when the watch already has
    /// frames the next part's first frame is left out. A watch that read no key frames takes the parts' box.
    pub fn join(&mut self, next: HudPart) {
        if self.layout.is_none() {
            self.layout = Some(next.layout);
            self.session = next.session;
        }
        let drop = usize::from(self.frames > 0);
        self.store.append(next.store, drop);
        self.frames += next.frames.saturating_sub(drop);
    }

    /// What the HUD read; None when there is no readable HUD (the review then finds the kills in the video alone).
    pub fn finish(mut self) -> Option<HudReading> {
        self.work_out_layout();
        self.layout.flatten().and_then(|l| kovaak(&self.store, &l)).or_else(|| aimlab(&self.store))
    }
}

// ---- reading the counts ---------------------------------------------------------------------------------------------

/// Glyph shapes seen so far; a glyph joins the most alike shape, or starts a new one (python/hud.py: _Shapes). A
/// shape is the mean of its first 50 glyphs.
struct Shapes<'a> {
    store: &'a Store,
    same: f64,
    shapes: Vec<[f32; GLYPH]>,
    norms: Vec<f64>,
    count: Vec<u32>,
    /// Bumped whenever a shape changes; each image's most alike shape is kept with the version it was found at.
    version: u32,
    memo: Vec<Option<(u32, Option<usize>)>>,
}

fn norm(v: impl Iterator<Item = f32>) -> f64 {
    (v.map(|x| x * x).sum::<f32>() as f64).sqrt()
}

impl<'a> Shapes<'a> {
    fn new(store: &'a Store, same: f64) -> Shapes<'a> {
        Shapes {
            store,
            same,
            shapes: Vec::new(),
            norms: Vec::new(),
            count: Vec::new(),
            version: 0,
            memo: vec![None; store.images.len() / GLYPH],
        }
    }

    fn glyph(&self, image: u32) -> [f32; GLYPH] {
        let image = self.store.image(image);
        std::array::from_fn(|k| image[k] as f32 / 255.0)
    }

    fn best(&self, image: u32) -> Option<usize> {
        let g = self.glyph(image);
        let gn = norm(g.iter().copied()).max(1e-6);
        let mut best: Option<(f64, usize)> = None;
        for (k, (s, &sn)) in self.shapes.iter().zip(&self.norms).enumerate() {
            let sim = g.iter().zip(s).map(|(a, b)| a * b).sum::<f32>() as f64 / gn / sn.max(1e-6);
            if sim >= self.same && best.is_none_or(|(b, _)| sim > b) {
                best = Some((sim, k));
            }
        }
        best.map(|b| b.1)
    }

    /// The glyph's shape; -1 when it is like none and `learn` is off.
    fn id(&mut self, image: u32, learn: bool) -> i32 {
        let best = match self.memo[image as usize] {
            Some((v, b)) if v == self.version => b,
            _ => {
                let b = self.best(image);
                self.memo[image as usize] = Some((self.version, b));
                b
            }
        };
        let k = match best {
            Some(k) => k,
            None if !learn => return -1,
            None => {
                let g = self.glyph(image);
                self.norms.push(norm(g.iter().copied()));
                self.shapes.push(g);
                self.count.push(0);
                self.version += 1;
                self.shapes.len() - 1
            }
        };
        self.count[k] += 1;
        // the shape is the mean of its first Kill Count glyphs (the Accuracy row's slashes and brackets, cut into
        // pieces, would blur it)
        if learn && self.count[k] <= 50 && self.count[k] > 1 {
            let (g, n) = (self.glyph(image), self.count[k] as f32);
            self.shapes[k].iter_mut().zip(&g).for_each(|(s, &v)| *s += (v - *s) / n);
            self.norms[k] = norm(self.shapes[k].iter().copied());
            self.version += 1;
        }
        k as i32
    }

    fn unit(&self, k: usize) -> [f32; GLYPH] {
        let n = self.norms[k].max(1e-6) as f32;
        self.shapes[k].map(|v| v / n)
    }
}

fn dot(a: &[f32], b: &[f32]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>() as f64
}

/// Stable readings: (reading, first frame, last frame) for each stretch of 3 frames or more (python/hud.py: _runs).
fn runs<T: Clone + PartialEq>(readings: &[Option<T>]) -> Vec<(T, usize, usize)> {
    let mut out: Vec<(Option<T>, usize, usize)> = Vec::new();
    for (i, r) in readings.iter().enumerate() {
        match out.last_mut() {
            Some(last) if last.0 == *r => last.2 = i,
            _ => out.push((r.clone(), i, i)),
        }
    }
    out.into_iter().filter(|r| r.2 - r.1 + 1 >= 3).filter_map(|(r, a, b)| r.map(|r| (r, a, b))).collect()
}

/// Counts in the order first seen (Python's Counter: most_common keeps that order among equal counts).
struct Counter<K>(Vec<(K, usize)>);

impl<K: PartialEq + Copy> Counter<K> {
    fn add(&mut self, k: K) {
        match self.0.iter_mut().find(|e| e.0 == k) {
            Some(e) => e.1 += 1,
            None => self.0.push((k, 1)),
        }
    }

    fn most_common(&self) -> Vec<(K, usize)> {
        let mut v = self.0.clone();
        v.sort_by_key(|e| std::cmp::Reverse(e.1));
        v
    }

    fn top(&self) -> Option<K> {
        self.0
            .iter()
            .fold(None, |best: Option<(K, usize)>, &e| if best.is_none_or(|b| e.1 > b.1) { Some(e) } else { best })
            .map(|e| e.0)
    }
}

/// Which shape is which digit, from stable readings counting up (python/hud.py: _learn_digits): by shape, its digit.
fn learn_digits(runs: &[&[i32]], shapes: usize) -> Option<Vec<Option<u8>>> {
    let (mut zero, mut succ) = (Counter(Vec::new()), Counter(Vec::new()));
    for w in runs.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (al, bl) = (a[a.len() - 1], b[b.len() - 1]);
        // a digit added after the same digits is no count's step (python/hud.py took it as one): it is a glyph that
        // is not part of the number, such as the edge of KovaaK's results panel fading out at the start of a run
        let grew = b.len() == a.len() + 1 && b[..a.len()] != *a;
        if b.len() == a.len() && a[..a.len() - 1] != b[..b.len() - 1] || grew {
            zero.add(bl); // the tens place changed (or a digit was added): b ends in 0
        }
        if b.len() == a.len() || grew {
            succ.add((al, bl)); // usually one more kill: the last digit's next shape
        }
    }
    let z = zero.top()?;
    let mut next: Vec<(i32, i32)> = Vec::new();
    for ((x, y), _) in succ.most_common() {
        if x != y && !next.iter().any(|e| e.0 == x) && !next.iter().any(|e| e.1 == y) {
            next.push((x, y));
        }
    }
    let after = |s: i32| next.iter().find(|e| e.0 == s).map(|e| e.1);
    let mut digits = vec![None; shapes];
    let (mut s, mut seen) = (z, vec![z]);
    digits[z as usize] = Some(0);
    for d in 1..10 {
        s = after(s)?;
        if seen.contains(&s) {
            return None;
        }
        seen.push(s);
        digits[s as usize] = Some(d);
    }
    (after(s) == Some(z)).then_some(digits)
}

/// The number a reading's shapes spell, if every one is a digit.
fn number(reading: &[i32], digits: &[Option<u8>]) -> Option<i64> {
    let ds = reading
        .iter()
        .map(|&k| digits.get(usize::try_from(k).ok()?).copied().flatten())
        .collect::<Option<Vec<u8>>>()?;
    value(&ds)
}

/// The number digits spell (None without any).
fn value(digits: &[u8]) -> Option<i64> {
    if digits.is_empty() {
        return None;
    }
    digits.iter().try_fold(0i64, |v, &d| v.checked_mul(10)?.checked_add(d as i64))
}

fn tall(g: &Glyph, band: usize) -> bool {
    g.h as f64 / band as f64 >= 0.5
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// The Kill Count read: the shapes, the digits, the stable values (value, first frame, last frame) and the share of
/// steps that were +1.
type Count<'a> = (Shapes<'a>, Vec<Option<u8>>, Vec<(i64, usize, usize)>, f64);

/// The Kill Count read at one likeness (python/hud.py: _count), or None when the digits are not learned or fewer than
/// `need` of the steps are +1. The shapes are learned from the Kill Count alone.
fn count<'a>(store: &'a Store, kill_lines: &[u32], band: usize, same: f64, need: f64) -> Option<Count<'a>> {
    let mut shapes = Shapes::new(store, same);
    let readings: Vec<Option<Vec<i32>>> = kill_lines
        .iter()
        .map(|&l| {
            let ids: Vec<i32> =
                store.lines[l as usize].iter().filter(|g| tall(g, band)).map(|g| shapes.id(g.image, true)).collect();
            (!ids.is_empty()).then_some(ids)
        })
        .collect();
    let stable = runs(&readings);
    let mut digits = learn_digits(&stable.iter().map(|r| r.0.as_slice()).collect::<Vec<_>>(), shapes.shapes.len())?;
    // in a blurred recording one digit can leave more than one shape: the others join the most alike digit
    for k in 0..shapes.shapes.len() {
        if digits[k].is_none() {
            let u = shapes.unit(k);
            let best = (0..digits.len())
                .filter(|&d| digits[d].is_some())
                .map(|d| (dot(&u, &shapes.unit(d)), d))
                .fold(None, |b: Option<(f64, usize)>, e| if b.is_none_or(|b| e >= b) { Some(e) } else { b });
            if let Some((sim, d)) = best
                && sim >= 0.9
            {
                digits[k] = digits[d];
            }
        }
    }
    // the stable values, from each frame's number: a digit caught mid-change can leave a shape of its own for a frame
    // or two, which joined its digit above, so the new value is read from its first frame (python/hud.py reads stable
    // shapes, and starts the value up to two frames late)
    let numbers: Vec<Option<i64>> = readings.iter().map(|r| r.as_ref().and_then(|r| number(r, &digits))).collect();
    let values = runs(&numbers);
    let steps: Vec<i64> = values.windows(2).map(|w| w[1].0 - w[0].0).collect();
    if steps.len() < 3 {
        return None;
    }
    let checked = steps.iter().filter(|&&s| s == 1).count() as f64 / steps.len() as f64;
    (checked >= need).then_some((shapes, digits, values, checked))
}

/// KovaaK's session box over the recording (python/hud.py: read).
fn kovaak(store: &Store, layout: &Layout) -> Option<HudReading> {
    let kill_lines = store.per_frame(KILLS);
    let band = layout.kills.y1 - layout.kills.y0;
    // a very blurred upload can split one digit into two shapes at the usual likeness, and the digits are not learned;
    // then looser likenesses are tried, trusting only a Kill Count that counts up by one at almost every step (95%)
    let (mut shapes, digits, values, checked) = [(SAME, 0.8), (0.96, 0.95), (0.95, 0.95), (0.94, 0.95), (0.93, 0.95)]
        .into_iter()
        .find_map(|(same, need)| count(store, &kill_lines, band, same, need))?;
    // a lone misread between two readings that follow on (0, 9, 1: a 1 caught mid-change) is dropped; it looked like a
    // restart, which split the run and lost the kills before it. Only a short one: a real restart's 0 between two 1s
    // stays up for a second or more
    let n = values.len();
    let keep: Vec<bool> = (0..n)
        .map(|i| {
            i == 0 || i == n - 1 || values[i].2 - values[i].1 > 10 || {
                let follows = |d: i64| (0..=3).contains(&d);
                follows(values[i].0 - values[i - 1].0) || !follows(values[i + 1].0 - values[i - 1].0)
            }
        })
        .collect();
    let values: Vec<(i64, usize, usize)> = values.into_iter().zip(keep).filter(|e| e.1).map(|e| e.0).collect();
    // a drop is a restart (or the end screen): the run that counts is the stretch between drops with the most kills
    let mut cuts = vec![0];
    cuts.extend((1..values.len()).filter(|&i| values[i].0 < values[i - 1].0));
    cuts.push(values.len());
    let (a0, a1) =
        cuts.windows(2).map(|c| (c[0], c[1])).max_by_key(|&(c0, c1)| (values[c1 - 1].0 - values[c0].0, c0))?;
    let values = &values[a0..a1];
    let (since, until) = (values[0].1, values[values.len() - 1].2);
    let mut kills = Vec::new();
    for w in values.windows(2) {
        let d = w[1].0 - w[0].0;
        if 0 < d && d <= 3 {
            kills.extend(std::iter::repeat_n(w[1].1 as i64, d as usize)); // a bigger jump is a misread
        }
    }
    // Accuracy: hits / shots (percent)
    let acc_band = layout.accuracy.y1 - layout.accuracy.y0;
    let acc_lines = store.per_frame(ACCURACY);
    let mut window: Vec<u32> = acc_lines[since..=until.min(acc_lines.len() - 1)].to_vec();
    window.sort_unstable();
    window.dedup();
    let marks = marks(store, &window, acc_band, &mut shapes, &digits);
    let mut read: HashMap<u32, Option<(i64, i64)>> = HashMap::new();
    let acc: Vec<Option<(i64, i64)>> = acc_lines
        .into_iter()
        .enumerate()
        .map(|(i, l)| {
            if i < since || i > until {
                return None;
            }
            *read.entry(l).or_insert_with(|| accuracy(store, l, acc_band, &mut shapes, &digits, marks.as_ref()))
        })
        .collect();
    let mut acc_runs = runs(&acc);
    // the Accuracy line is redrawn three times a second, so a run started again first shows the run before's reading
    // for a moment: a first reading that drops right after is that one. When the Kill Count starts at 0, the counts
    // of the first reading are the run's first shots and hits (python/hud.py counts only the steps after it)
    while acc_runs.len() >= 2 && acc_runs[1].0.1 < acc_runs[0].0.1 {
        acc_runs.remove(0);
    }
    let (mut shots, mut hits) = (Vec::new(), Vec::new());
    if let Some(&((h, s), a, _)) = acc_runs.first()
        && values[0].0 == 0
        && s <= 50
    {
        shots.extend(std::iter::repeat_n(a as i64, s as usize));
        hits.extend(std::iter::repeat_n(a as i64, h as usize));
    }
    for w in acc_runs.windows(2) {
        let ((p0, _, _), (p1, a, _)) = (&w[0], &w[1]);
        let (dh, ds) = (p1.0 - p0.0, p1.1 - p0.1);
        if (0..=50).contains(&dh) && 0 < ds && ds <= 50 {
            shots.extend(std::iter::repeat_n(*a as i64, ds as usize));
            hits.extend(std::iter::repeat_n(*a as i64, dh as usize));
        }
    }
    // the totals: the kills counted, and the fullest Accuracy reading (the HUD resets to 0 when the run ends)
    let mut top = acc_runs
        .iter()
        .map(|r| r.0)
        .fold(None, |b: Option<(i64, i64)>, r| if b.is_none_or(|b| r.1 > b.1) { Some(r) } else { b });
    // each kill takes a hit. The Accuracy line is redrawn three times a second, and a run can end before it shows its
    // last kills: those kills' hits (each a shot) are added at their kill frames (python/hud.py gives the last reading)
    if let Some((h, s)) = top
        && h < kills.len() as i64
    {
        let late = &kills[kills.len() - (kills.len() as i64 - h) as usize..];
        hits.extend(late);
        shots.extend(late);
        hits.sort_unstable();
        shots.sort_unstable();
        top = Some((h + late.len() as i64, s + late.len() as i64));
    }
    Some(HudReading {
        game: HudGame::Kovaak,
        totals: HudFinal { kills: kills.len() as i64, hits: top.map(|t| t.0), shots: top.map(|t| t.1) },
        kills,
        shots,
        hits,
        checked: round3(checked),
        points: None,
    })
}

/// The Accuracy line's tall glyphs by the rule of python/hud.py: a digit when it is like a digit shape at the
/// likeness the Kill Count was read at (Some), else a mark (None).
fn by_likeness(store: &Store, l: u32, band: usize, shapes: &mut Shapes, digits: &[Option<u8>]) -> Vec<Option<u8>> {
    store.lines[l as usize]
        .iter()
        .filter(|g| tall(g, band))
        .map(|g| usize::try_from(shapes.id(g.image, false)).ok().and_then(|k| digits[k]))
        .collect()
}

/// The Accuracy line's "/" and "(" (unit images): the mean of each in the lines that read as digits, the "/", digits
/// and the "(". None when too few lines did.
fn marks(
    store: &Store,
    lines: &[u32],
    band: usize,
    shapes: &mut Shapes,
    digits: &[Option<u8>],
) -> Option<[[f32; GLYPH]; 2]> {
    let mut sums = [[0f32; GLYPH]; 2];
    let mut counts = [0; 2];
    for &l in lines {
        let read = by_likeness(store, l, band, shapes, digits);
        let mut at = read.iter().enumerate().filter(|e| e.1.is_none()).map(|e| e.0);
        let (Some(slash), Some(paren)) = (at.next(), at.next()) else {
            continue;
        };
        if slash == 0 || paren == slash + 1 {
            continue;
        }
        let glyphs: Vec<&Glyph> = store.lines[l as usize].iter().filter(|g| tall(g, band)).collect();
        for (k, at) in [slash, paren].into_iter().enumerate() {
            if counts[k] < 50 {
                let g = shapes.glyph(glyphs[at].image);
                let n = norm(g.iter().copied()).max(1e-6) as f32;
                sums[k].iter_mut().zip(&g).for_each(|(s, v)| *s += v / n);
                counts[k] += 1;
            }
        }
    }
    if counts.iter().any(|&c| c < 3) {
        return None;
    }
    Some(sums.map(|s| {
        let n = norm(s.iter().copied()).max(1e-6) as f32;
        s.map(|v| v / n)
    }))
}

/// One Accuracy line's hits and shots. The "/" and the "(" are the tall glyphs that are not digits: hits before the
/// first, shots between them. Small or limited-range text can leave a digit just under the Kill Count's likeness
/// (python/hud.py then reads it as a mark, and the line as nothing or as no shot yet), so with the marks known each
/// glyph is the most alike of the digits, the "/" and the "(" instead.
fn accuracy(
    store: &Store,
    l: u32,
    band: usize,
    shapes: &mut Shapes,
    digits: &[Option<u8>],
    marks: Option<&[[f32; GLYPH]; 2]>,
) -> Option<(i64, i64)> {
    let read = match marks {
        None => by_likeness(store, l, band, shapes, digits),
        Some(marks) => {
            let protos: Vec<([f32; GLYPH], Option<u8>)> = (0..digits.len())
                .filter_map(|k| digits[k].map(|d| (shapes.unit(k), Some(d))))
                .chain(marks.iter().map(|&m| (m, None)))
                .collect();
            store.lines[l as usize]
                .iter()
                .filter(|g| tall(g, band))
                .map(|g| {
                    let img = shapes.glyph(g.image);
                    let n = norm(img.iter().copied()).max(1e-6) as f32;
                    let u = img.map(|x| x / n);
                    protos
                        .iter()
                        .map(|(p, d)| (dot(&u, p), *d))
                        .fold((f64::MIN, None), |b, e| if e.0 > b.0 { e } else { b })
                        .1
                })
                .collect()
        }
    };
    let (mut parts, mut cur) = (Vec::new(), Vec::new());
    for d in &read {
        match d {
            Some(d) => cur.push(*d),
            None => parts.push(std::mem::take(&mut cur)),
        }
    }
    if parts.len() < 2 {
        None
    } else if parts[0].is_empty() && parts[1].is_empty() {
        // "--/-- ( %)": no shot yet (its tall glyphs are the marks and "%)"; a line of unread digits has more)
        (read.len() <= 4).then_some((0, 0))
    } else {
        match (value(&parts[0]), value(&parts[1])) {
            (Some(h), Some(s)) if h <= s => Some((h, s)),
            _ => None,
        }
    }
}

/// Aim Lab's HUD (python/hud.py: read_aimlab): every hit counted as a kill (one-hit targets). A hit adds points and a
/// miss takes some off, so the POINTS number gives every hit and miss. The digits are learned from the TIME box,
/// which counts down one second at a time (read backwards it counts up, as the Kill Count does).
fn aimlab(store: &Store) -> Option<HudReading> {
    let time_lines = store.per_frame(TIME);
    let mut found = None;
    for same in [SAME, 0.96, 0.95] {
        let mut shapes = Shapes::new(store, same);
        let readings: Vec<Option<Vec<i32>>> = time_lines
            .iter()
            .map(|&l| {
                let line = &store.lines[l as usize];
                if line.len() != 4 {
                    return None;
                }
                let ids: Vec<i32> = line.iter().filter(|g| tall(g, AH)).map(|g| shapes.id(g.image, true)).collect();
                (!ids.is_empty()).then_some(ids)
            })
            .collect();
        let runs = runs(&readings);
        let back: Vec<&[i32]> = runs.iter().rev().map(|r| r.0.as_slice()).collect();
        let Some(digits) = learn_digits(&back, shapes.shapes.len()) else {
            continue;
        };
        let secs: Vec<i64> =
            runs.iter().filter_map(|r| number(&r.0, &digits)).map(|v| 60 * (v / 100) + v % 100).collect();
        let steps: Vec<i64> = secs.windows(2).map(|w| w[0] - w[1]).collect();
        if steps.len() >= 10 && steps.iter().filter(|&&x| x == 1).count() as f64 >= 0.95 * steps.len() as f64 {
            found = Some((shapes, digits));
            break;
        }
    }
    let (shapes, digits) = found?;
    // POINTS: each glyph is the most alike digit shape (a minus sign is short and wide)
    let protos: Vec<([f32; GLYPH], u8)> =
        digits.iter().enumerate().filter_map(|(k, d)| d.map(|d| (shapes.unit(k), d))).collect();
    let mut read: HashMap<u32, Option<i64>> = HashMap::new();
    let vals: Vec<Option<i64>> = store
        .per_frame(POINTS)
        .into_iter()
        .map(|l| {
            *read.entry(l).or_insert_with(|| {
                let (mut v, mut sign) = (Vec::new(), 1);
                for g in &store.lines[l as usize] {
                    if !tall(g, AH) {
                        if v.is_empty() && g.w as f64 / g.h as f64 > 1.2 {
                            sign = -1;
                        }
                        continue;
                    }
                    let img = shapes.glyph(g.image);
                    let n = norm(img.iter().copied()).max(1e-6) as f32;
                    let u = img.map(|x| x / n);
                    let (sim, d) = protos
                        .iter()
                        .map(|(p, d)| (dot(&u, p), *d))
                        .fold((f64::MIN, 0), |b, e| if e >= b { e } else { b });
                    if sim < 0.9 {
                        v.clear();
                        break;
                    }
                    v.push(d);
                }
                value(&v).map(|n| sign * n)
            })
        })
        .collect();
    let runs = runs(&vals);
    let changes: Vec<(i64, usize)> = runs.windows(2).map(|w| (w[1].0 - w[0].0, w[1].1)).collect();
    let (mut ups, mut downs) = (Counter(Vec::new()), Counter(Vec::new()));
    for &(d, _) in &changes {
        if d > 0 {
            ups.add(d);
        } else if d < 0 {
            downs.add(d);
        }
    }
    let hit = ups.top()?;
    let miss = downs.top();
    let (mut hits, mut misses, mut ok) = (Vec::new(), Vec::new(), 0);
    for &(d, f) in &changes {
        // a jump of two hits, or a hit and a miss, in one step
        let best = (0..4i64)
            .flat_map(|a| (0..if miss.is_some() { 4i64 } else { 1 }).map(move |b| (a, b)))
            .filter(|&(a, b)| a + b > 0)
            .map(|(a, b)| ((d - a * hit - b * miss.unwrap_or(0)).abs(), a, b))
            .min();
        if let Some((err, a, b)) = best
            && err as f64 <= 1f64.max(0.15 * hit as f64)
        {
            hits.extend(std::iter::repeat_n(f as i64, a as usize));
            misses.extend(std::iter::repeat_n(f as i64, b as usize));
            ok += 1;
        }
    }
    let checked = ok as f64 / changes.len().max(1) as f64;
    if hits.len() < 10 || checked < 0.85 {
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
        checked: round3(checked),
        points: runs.last().map(|r| r.0 as f64),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A glyph for pattern `p` (0 to 11): two rows of its own lit, so no two patterns are alike.
    fn glyph(p: usize, h: u16) -> Cut {
        let mut image = [0u8; GLYPH];
        image[2 * p * GW..(2 * p + 2) * GW].fill(255);
        Cut { image, h, w: 10 }
    }

    fn line(value: i64, h: u16) -> Vec<Cut> {
        value.to_string().bytes().map(|b| glyph((b - b'0') as usize, h)).collect()
    }

    #[test]
    fn a_wide_glyph_is_split_and_the_label_left_out() {
        let (w, h) = (200, 30);
        let mut band = vec![60u8; w * h];
        let mut fill = |x0: usize, x1: usize, y0: usize, y1: usize| {
            for y in y0..y1 {
                band[y * w + x0..y * w + x1].fill(230);
            }
        };
        fill(10, 40, 8, 22); // the label
        fill(150, 162, 5, 25); // two digits run together by a one-pixel bridge
        fill(164, 176, 5, 25);
        fill(162, 164, 15, 16);
        let glyphs = value_glyphs(&band, w, None);
        assert_eq!(glyphs.iter().map(|g| (g.w, g.h)).collect::<Vec<_>>(), vec![(12, 20), (14, 20)]);
        assert!(glyphs[0].image.iter().all(|&v| v == 255));
    }

    #[test]
    fn digits_are_learned_from_a_count() {
        // shape ids for the digits 0 to 9, in an order of their own
        let shape = [7, 3, 9, 0, 5, 1, 8, 2, 6, 4];
        let readings: Vec<Vec<i32>> =
            (0..=25).map(|v: u32| v.to_string().bytes().map(|b| shape[(b - b'0') as usize]).collect()).collect();
        let runs: Vec<&[i32]> = readings.iter().map(|r| r.as_slice()).collect();
        let digits = learn_digits(&runs, 10).unwrap();
        for (d, &s) in shape.iter().enumerate() {
            assert_eq!(digits[s as usize], Some(d as u8));
        }
        assert_eq!(learn_digits(&runs[..8], 10), None); // no tens place changed: no 0
    }

    /// A synthetic HUD (`hud`) with a miss before every fifth kill.
    fn counting(frames: Range<usize>) -> (Store, Layout) {
        hud(frames, |f, kills| {
            let misses = (1..=kills).filter(|k| k % 5 == 0).count() as i64 + i64::from(kills % 5 == 4 && f % 10 >= 5);
            (kills, kills + misses)
        })
    }

    /// A store fed with a synthetic HUD: the Kill Count counting to 40, ten frames a value, and Accuracy as
    /// `acc(frame, kills)`, hits/shots (percent).
    fn hud(frames: Range<usize>, acc: impl Fn(usize, i64) -> (i64, i64)) -> (Store, Layout) {
        let layout = Layout {
            x0: 0,
            x1: 100,
            kills: Row { y0: 0, y1: 20, start: None },
            accuracy: Row { y0: 30, y1: 50, start: None },
        };
        let mut store = Store::default();
        let mut recent: [Vec<Glyph>; ROWS] = Default::default();
        for f in frames {
            let kills = (f / 10).min(40) as i64;
            let (hits, shots) = acc(f, kills);
            let mut acc = line(hits, 18);
            acc.push(glyph(10, 18)); // "/"
            acc.extend(line(shots, 18));
            acc.push(glyph(11, 18)); // "("
            acc.extend(line(90, 18));
            for (row, cuts) in [(KILLS, line(kills, 18)), (ACCURACY, acc), (POINTS, Vec::new()), (TIME, Vec::new())] {
                store.push(row, cuts, &mut recent[row]);
            }
        }
        (store, layout)
    }

    #[test]
    fn a_counting_hud_reads_its_kills_and_shots() {
        let (store, layout) = counting(0..430);
        let r = kovaak(&store, &layout).unwrap();
        assert_eq!(r.kills, (1..=40).map(|k| 10 * k).collect::<Vec<i64>>());
        assert_eq!(r.totals, HudFinal { kills: 40, hits: Some(40), shots: Some(48) });
        assert_eq!(r.hits.len(), 40);
        assert_eq!(r.shots.iter().filter(|&&f| f % 10 == 5).count(), 8); // the misses, half way between kills
        assert_eq!(r.checked, 1.0);
        assert!(aimlab(&store).is_none());
    }

    #[test]
    fn a_restart_and_an_early_end_still_count_every_hit() {
        // the Accuracy line is redrawn every 7 frames: first it shows the run before's 8/9, and the run ends before it
        // shows the last kill
        let (store, layout) = hud(0..430, |f, _| {
            let tick = (f / 7 * 7).min(399);
            let k = (tick / 10) as i64;
            if f < 15 { (8, 9) } else { (k, k) }
        });
        let r = kovaak(&store, &layout).unwrap();
        assert_eq!(r.totals, HudFinal { kills: 40, hits: Some(40), shots: Some(40) });
        assert_eq!((r.hits.len(), r.shots.len()), (40, 40));
        assert_eq!((r.hits[0], r.hits[39]), (15, 400)); // the run's first reading, and the last kill
    }

    #[test]
    fn a_glyph_after_the_number_teaches_no_digit() {
        let shape = [7, 3, 9, 0, 5, 1, 8, 2, 6, 4];
        let mut readings: Vec<Vec<i32>> = Vec::new();
        for v in 0..=25u32 {
            readings.push(v.to_string().bytes().map(|b| shape[(b - b'0') as usize]).collect());
            if v % 4 == 1 {
                let mut junk = readings[readings.len() - 1].clone();
                junk.push(10); // an edge or a panel beside the number, for a moment
                readings.push(junk);
                readings.push(readings[readings.len() - 2].clone());
            }
        }
        let runs: Vec<&[i32]> = readings.iter().map(|r| r.as_slice()).collect();
        let digits = learn_digits(&runs, 11).unwrap();
        assert_eq!(digits[10], None);
        assert!(shape.iter().enumerate().all(|(d, &s)| digits[s as usize] == Some(d as u8)));
    }

    #[test]
    fn parts_join_as_one_watch() {
        let (whole, layout) = counting(0..430);
        let part = |frames: Range<usize>| HudPart {
            frames: frames.len(),
            layout: Some(layout),
            session: None,
            store: counting(frames).0,
        };
        // each run but the last also reads the next run's first frame
        let parts = [part(0..201), part(200..301), part(300..430)];
        let mut watch = HudWatch::new(0, 0, true);
        for p in parts {
            let text = serde_json::to_string(&p).unwrap();
            let back: HudPart = serde_json::from_str(&text).unwrap();
            assert_eq!(back, p);
            watch.join(back);
        }
        assert_eq!(watch.frames(), 430);
        let glyphs = |s: &Store, row: usize| -> Vec<Vec<Vec<u8>>> {
            s.per_frame(row)
                .into_iter()
                .map(|l| s.lines[l as usize].iter().map(|g| s.image(g.image).to_vec()).collect())
                .collect()
        };
        for row in 0..ROWS {
            assert_eq!(glyphs(&watch.store, row), glyphs(&whole, row));
        }
        assert_eq!(watch.finish(), kovaak(&whole, &layout));
    }

    #[test]
    fn a_part_that_does_not_add_up_is_refused() {
        let (store, layout) = counting(0..50);
        let mut text =
            serde_json::to_value(HudPart { frames: 50, layout: Some(layout), session: None, store }).unwrap();
        text["frames"] = 51.into();
        assert!(serde_json::from_value::<HudPart>(text).is_err());
    }

    #[test]
    fn skipped_frames_read_nothing() {
        let mut watch = HudWatch::new(64, 36, true);
        watch.skip(5);
        watch.add(&[0u8; 64 * 36]);
        assert_eq!(watch.frames(), 6);
        assert_eq!(watch.store.per_frame(KILLS), vec![0; 6]);
        assert_eq!(watch.finish(), None);
    }
}
