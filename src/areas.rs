//! A recording's overlay areas (HUD boxes, clocks, a webcam) for the "Exclude areas" editor, and what each one is
//! (python/areas.py, all of it).
//!
//! Finding: anything that stays put on screen while the view moves stands out from the wall behind it in nearly every
//! frame (`fixed::contrast`), as the crosshair and the HUD do in the fixed map. The run's key frames are used (or 90
//! frames spread over it, when it has fewer than 24: `sample_frames`). Pixels that stand out in 80% of them or more are
//! fixed, and fixed pixels a few pixels apart are joined into one area. A webcam is found by its border: its content
//! changes, but its edge against the game stays. The crosshair, the fixed spot at the center, is never an area.
//!
//! Naming, in two steps: rules (`rule_kind`: KovaaK's session box where the HUD watch finds it, Aim Lab's POINTS, TIME
//! and ACCURACY boxes, a webcam, the timer, a clock, the scenario name, the settings box, the version, else Other), and
//! learning from the user's saved areas (`learn`, `predict`: the kind most of an area's 5 nearest examples have).
//!
//! The arithmetic is NumPy's where a last bit can change an answer: the means are summed in NumPy's order, in float32
//! where Python's maps are float32, and the crosshair zoom test scales with Pillow's 8-bit bilinear filter. The frames
//! are the review's 1280 x 720 YUV 4:2:0 (`convert::Converter::yuv420p`), the same bytes ffmpeg gives Python.
//!
//! The pure functions take and give the JSON python/areas.py and python/server.py use: found areas as areas.json holds
//! them, saved areas as exclude.json holds them, and examples as the lines of area_examples.jsonl.

use std::collections::BTreeMap;
use std::ops::{Add, Range};

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Value, json};

use crate::fixed::{DIFF, contrast};
use crate::geometry::{CX, CY, H, W};
use crate::hud::{AIM_BAND, AIM_POINTS, AIM_TIME, AW, BH, BOX, BW, SessionRows, bilinear, taps};
use crate::python::round;

/// Frames sampled over the run when it has fewer than MIN_KEYS key frames.
pub const N_SAMPLES: usize = 90;
/// A run with this many key frames or more is read from its key frames.
pub const MIN_KEYS: usize = 24;
/// A pixel standing out in this share of the frames is fixed.
pub const FIXED: f64 = 0.8;
/// Fixed pixels this close (pixels at 1280 x 720) join one area.
pub const GAP: u16 = 5;
/// The kind of an area the user removed: not an area.
pub const NONE: &str = "none";
/// The nearest examples that vote on an area's kind.
pub const K: usize = 5;
/// One frame: YUV 4:2:0 at 1280 x 720.
pub const FRAME: usize = W * H * 3 / 2;

const SESSION: &str = "Session stats";
const ZOOMED: &str = "Zoomed crosshair";

// ---- which frames ---------------------------------------------------------------------------------------------------

/// Which frames the area finder reads (python/areas.py: sample): None when the run has MIN_KEYS key frames or more
/// (then it reads every key frame, in order); else the indexes of the frames to give it, in order, a frame given again
/// where an index repeats. `times`: every frame's time in seconds (frame 0 is the first from time 0 on), `duration`:
/// the recording's duration as ffprobe gives it (`format=duration`). The indexes are those ffmpeg's `fps` filter picks
/// at N_SAMPLES / (duration - 1) frames a second (python/areas.py's command): output frame k is the last frame whose
/// time, in output frames, rounds to k or less, until the end of the last frame.
pub fn sample_frames(keys: usize, times: &[f64], duration: f64) -> Option<Vec<usize>> {
    (keys < MIN_KEYS).then(|| fps_frames(times, duration, N_SAMPLES))
}

fn fps_frames(times: &[f64], duration: f64, n: usize) -> Vec<usize> {
    let Some(&last) = times.last() else {
        return Vec::new();
    };
    // the rate as python/areas.py writes it into the filter (5 decimals)
    let rate: f64 = format!("{:.5}", n as f64 / (duration - 1.0).max(1.0)).parse().unwrap_or(1.0);
    let near = |t: f64| (t * rate).round() as i64;
    // the end of the stream: the last frame's time and its length (the one before it)
    let step = if times.len() > 1 { last - times[times.len() - 2] } else { 0.0 };
    let end = near(last + step);
    let (mut out, mut i, mut k) = (Vec::new(), 0, near(times[0]));
    while k < end {
        while i + 1 < times.len() && near(times[i + 1]) <= k {
            i += 1;
        }
        out.push(i);
        k += 1;
    }
    out
}

// ---- the data -------------------------------------------------------------------------------------------------------

/// A found area, as areas.json holds it: its box (shares of the frame, x0, y0, x1, y1), its features (`features`) and
/// the kind the rules give it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Area {
    #[serde(rename = "box")]
    pub bounds: [f64; 4],
    pub feat: Vec<f64>,
    pub rule: String,
}

/// An area the user saved (exclude.json): [x0, y0, x1, y1] as shares, and its kind (a type id, or a name in older
/// files), None when the entry has only four numbers.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedBox {
    pub bounds: [f64; 4],
    pub kind: Option<String>,
}

impl Serialize for SavedBox {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut v: Vec<Value> = self.bounds.iter().map(|&x| json!(x)).collect();
        if let Some(k) = &self.kind {
            v.push(json!(k));
        }
        v.serialize(s)
    }
}

impl<'de> Deserialize<'de> for SavedBox {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<SavedBox, D::Error> {
        let v = Vec::<Value>::deserialize(d)?;
        let num = |i: usize| v.get(i).and_then(Value::as_f64).ok_or_else(|| D::Error::custom("a box: 4 numbers"));
        Ok(SavedBox {
            bounds: [num(0)?, num(1)?, num(2)?, num(3)?],
            kind: v.get(4).and_then(|k| k.as_str()).map(str::to_string),
        })
    }
}

/// One example for the learner (a line of area_examples.jsonl): the recording, an area's features, and its kind.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Example {
    pub rec: String,
    pub feat: Vec<f64>,
    pub kind: String,
}

/// A found area with the kind given to it, and by what ("learned" or "rule").
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Named {
    #[serde(flatten)]
    pub area: Area,
    pub kind: String,
    pub by: String,
}

/// What the area finder found in a recording: the areas and the maps they came from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Found {
    /// The frames read.
    pub frames: usize,
    pub areas: Vec<Area>,
    pub maps: Maps,
}

// ---- the maps -------------------------------------------------------------------------------------------------------

/// The stand-out map (the share of the frames each pixel stood out in, times 255, rounded) and the change map (each
/// pixel's mean brightness change from one frame to the next, cut to 0..255), 1280 x 720 each, as
/// python/areas.py keeps them (areas_maps.npz). As JSON each map is packed (`pack`) and in base64.
#[derive(Clone, Debug, PartialEq)]
pub struct Maps {
    stand: Vec<u8>,
    change: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct MapsText {
    w: usize,
    h: usize,
    stand: String,
    change: String,
}

impl Serialize for Maps {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        MapsText { w: W, h: H, stand: base64(&pack(&self.stand, W, H)), change: base64(&pack(&self.change, W, H)) }
            .serialize(s)
    }
}

impl<'de> Deserialize<'de> for Maps {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Maps, D::Error> {
        let t = MapsText::deserialize(d)?;
        if (t.w, t.h) != (W, H) {
            return Err(D::Error::custom("maps: not 1280 x 720"));
        }
        let plane =
            |s: &str| unbase64(s).and_then(|b| unpack(&b, W, H)).ok_or_else(|| D::Error::custom("maps: unreadable"));
        Ok(Maps { stand: plane(&t.stand)?, change: plane(&t.change)? })
    }
}

impl Maps {
    /// From the two maps' bytes (W x H each, row by row); None when a size is wrong.
    pub fn new(stand: Vec<u8>, change: Vec<u8>) -> Option<Maps> {
        (stand.len() == W * H && change.len() == W * H).then_some(Maps { stand, change })
    }

    pub fn stand(&self) -> &[u8] {
        &self.stand
    }

    pub fn change(&self) -> &[u8] {
        &self.change
    }

    /// An area's features from the maps as kept (python/areas.py's learn, with maps() reading the npz back: the
    /// stand-out map as float32 shares of 255, the change map as float32).
    pub fn features(&self, b: &[f64; 4]) -> Vec<f64> {
        features(
            b,
            |i| self.stand[i] as f32 / 255.0 >= FIXED as f32,
            |r| {
                let n = r.size() as f64;
                let mean = (numpy_sum(r, |i| self.change[i] as f32) as f64 / n) as f32;
                (mean / 40.0) as f64
            },
        )
    }
}

// ---- finding --------------------------------------------------------------------------------------------------------

/// Finds a recording's areas from its frames (python/areas.py: analyse). Give it each frame `sample_frames` picks
/// (`add`), then `finish`. It keeps each frame's brightness (0.9 MB a frame) for the change map and the crosshair zoom
/// test.
#[derive(Clone, Debug)]
pub struct AreaFinder {
    /// Per pixel, the frames it stood out in.
    counts: Vec<u16>,
    /// Each frame's Y plane.
    ys: Vec<Vec<u8>>,
}

impl Default for AreaFinder {
    fn default() -> AreaFinder {
        AreaFinder { counts: vec![0; W * H], ys: Vec::new() }
    }
}

impl AreaFinder {
    pub fn new() -> AreaFinder {
        AreaFinder::default()
    }

    /// One frame, YUV 4:2:0 at 1280 x 720 (FRAME bytes); a shorter buffer is left out.
    pub fn add(&mut self, yuv: &[u8]) {
        if yuv.len() < FRAME {
            return;
        }
        for (n, c) in self.counts.iter_mut().zip(contrast(yuv)) {
            *n += (c > DIFF) as u16;
        }
        self.ys.push(yuv[..W * H].to_vec());
    }

    /// The frames added.
    pub fn frames(&self) -> usize {
        self.ys.len()
    }

    /// Whether an area (shares of the frame) is a magnified copy of the screen around the crosshair, over the frames
    /// added so far (`zoomed`).
    pub fn zoomed(&self, b: &[f64; 4]) -> bool {
        zoomed(b, &self.ys)
    }

    /// The found areas and the maps. `session`: KovaaK's session box as the HUD watch finds it in the key frames
    /// (`hud::HudWatch::session_box`), None without one.
    pub fn finish(self, session: Option<SessionRows>) -> Found {
        let n = self.ys.len();
        let stand: Vec<f64> = self.counts.iter().map(|&c| if n > 0 { c as f64 / n as f64 } else { 0.0 }).collect();
        let mut sums = vec![0u32; W * H];
        for pair in self.ys.windows(2) {
            for ((s, &a), &b) in sums.iter_mut().zip(&pair[0]).zip(&pair[1]) {
                *s += a.abs_diff(b) as u32;
            }
        }
        let pairs = n.saturating_sub(1);
        let change: Vec<f64> = sums.iter().map(|&s| if pairs > 0 { s as f64 / pairs as f64 } else { 0.0 }).collect();
        let maps = Maps {
            stand: stand.iter().map(|&s| (s * 255.0).round_ties_even() as u8).collect(),
            change: change.iter().map(|&c| c.clamp(0.0, 255.0) as u8).collect(),
        };
        let fixed: Vec<bool> = stand.iter().map(|&s| s >= FIXED).collect();
        // the view hardly moved (a probe run): the room itself stays put, and overlays cannot be told from it
        let still = fixed.iter().filter(|&&f| f).count() as f64 / (W * H) as f64 > 0.15;
        let areas = if n == 0 || still { Vec::new() } else { find_areas(&stand, &fixed, &change, &self.ys, session) };
        Found { frames: n, areas, maps }
    }
}

/// KovaaK's session box as shares of the frame, with its border around the rows the HUD watch reads.
pub fn session_share(s: SessionRows) -> [f64; 4] {
    let (sx, sy) = (BOX[2] / BW as f64, BOX[3] / BH as f64);
    let pad = 8.0 / W as f64;
    let pos = |v: f64| if v > 0.0 { v } else { 0.0 };
    [
        pos(s.x0 as f64 * sx - pad),
        pos((s.y0 as f64 - 8.0) * sy - pad),
        s.x1 as f64 * sx + pad,
        (s.y1 + 8) as f64 * sy + pad,
    ]
}

fn find_areas(
    stand: &[f64],
    fixed: &[bool],
    change: &[f64],
    ys: &[Vec<u8>],
    session: Option<SessionRows>,
) -> Vec<Area> {
    let session_box = session.map(session_share);
    let mut grown = grow(fixed);
    if let Some(sb) = session_box {
        // cut out, so a clock beside it is an area of its own
        let r = px(&sb);
        let rows = (r[1] - 4).max(0) as usize..((r[3] + 4).max(0) as usize).min(H);
        let cols = (r[0] - 4).max(0) as usize..((r[2] + 4).max(0) as usize).min(W);
        for y in rows {
            grown[y * W..(y + 1) * W][cols.clone()].fill(false);
        }
    }
    let aim = aim_boxes(stand, fixed);
    let mut boxes: Vec<[f64; 4]> = Vec::new();
    for [x0, y0, x1, y1] in components(&grown) {
        let (w, h) = (x1 - x0, y1 - y0);
        let any_fixed = (y0..y1).any(|y| fixed[y * W + x0..y * W + x1].iter().any(|&f| f));
        if w * h < 80 || w.min(h) < 7 || !any_fixed {
            continue; // too small, or a sliver of a box's border
        }
        if x0 as f64 <= CX && CX <= x1 as f64 && y0 as f64 <= CY && CY <= y1 as f64 && w < 120 {
            continue; // the crosshair: never an area
        }
        boxes.push([x0 as f64 / W as f64, y0 as f64 / H as f64, x1 as f64 / W as f64, y1 as f64 / H as f64]);
    }
    let mut session_at = None;
    if let Some(sb) = session_box {
        // the session box as the HUD watch finds it, whole
        boxes.retain(|b| inside(b, &sb) < 0.6);
        boxes.push(sb);
        session_at = Some(boxes.len() - 1);
    }
    // an area mostly inside a bigger one (text inside a box, a webcam's details) is part of it
    let keep: Vec<bool> = (0..boxes.len())
        .map(|i| {
            let b = &boxes[i];
            session_at == Some(i)
                || !boxes.iter().enumerate().any(|(j, o)| j != i && area(o) > area(b) && inside(b, o) > 0.8)
        })
        .collect();
    boxes
        .iter()
        .zip(keep)
        .filter(|&(_, k)| k)
        .map(|(b, _)| {
            let feat = features(b, |i| fixed[i], |r| numpy_sum(r, |i| change[i]) / r.size() as f64 / 40.0);
            let rule = if zoomed(b, ys) {
                ZOOMED.to_string()
            } else {
                rule_kind(b, &feat, session_box.as_ref(), aim.as_deref()).to_string()
            };
            Area { bounds: b.map(|v| round(v, 4)), feat, rule }
        })
        .collect()
}

/// Aim Lab's POINTS and TIME boxes where their values stand out, and the ACCURACY box after them (as wide as POINTS);
/// None unless both are there.
fn aim_boxes(stand: &[f64], fixed: &[bool]) -> Option<Vec<([f64; 4], &'static str)>> {
    let band = AIM_BAND;
    let (yb0, yb1) = ((band[1] * H as f64) as usize, (band[3] * H as f64) as usize);
    let sxa = (band[2] - band[0]) / AW as f64;
    if !fixed[yb0 * W..yb1 * W].iter().any(|&f| f) {
        return None;
    }
    let mut aim = Vec::new();
    for ((c0, c1), kind) in [(AIM_POINTS, SESSION), (AIM_TIME, "Timer")] {
        let b = [band[0] + c0 as f64 * sxa, band[1], band[0] + c1 as f64 * sxa, band[3]];
        let (xa, xb) = ((b[0] * W as f64) as usize, (b[2] * W as f64) as usize);
        if (yb0..yb1).any(|y| stand[y * W + xa..y * W + xb].iter().any(|&s| s >= FIXED)) {
            aim.push((b, kind));
        }
    }
    if aim.len() != 2 {
        return None;
    }
    let w = aim[0].0[2] - aim[0].0[0];
    let x = aim[1].0[2] + 0.005;
    aim.push(([x, band[1], x + w, band[3]], SESSION));
    Some(aim)
}

/// The fixed pixels grown into areas: closed over GAP pixels, then grown by GAP / 2 (SciPy's binary_closing and
/// binary_dilation with the 4-neighbour cross; the closing's erosion counts the pixels past the frame's edge as
/// empty).
fn grow(fixed: &[bool]) -> Vec<bool> {
    let dilated: Vec<bool> = city_block(fixed, false).into_iter().map(|d| d <= GAP).collect();
    let holes: Vec<bool> = dilated.iter().map(|&v| !v).collect();
    let closed: Vec<bool> = city_block(&holes, true).into_iter().map(|d| d > GAP).collect();
    city_block(&closed, false).into_iter().map(|d| d <= GAP / 2).collect()
}

/// Each pixel's city-block distance to the nearest `on` pixel (W x H); `edge`: the pixels just past the frame's edge
/// count as on.
fn city_block(on: &[bool], edge: bool) -> Vec<u16> {
    let far = u16::MAX - 1;
    let mut d: Vec<u16> = on.iter().map(|&v| if v { 0 } else { far }).collect();
    let past = if edge { 1 } else { far };
    for y in 0..H {
        for x in 0..W {
            let i = y * W + x;
            let left = if x > 0 { d[i - 1].saturating_add(1) } else { past };
            let up = if y > 0 { d[i - W].saturating_add(1) } else { past };
            d[i] = d[i].min(left).min(up);
        }
    }
    for y in (0..H).rev() {
        for x in (0..W).rev() {
            let i = y * W + x;
            let right = if x + 1 < W { d[i + 1].saturating_add(1) } else { past };
            let down = if y + 1 < H { d[i + W].saturating_add(1) } else { past };
            d[i] = d[i].min(right).min(down);
        }
    }
    d
}

/// The 4-connected areas of `on` (W x H), in the order SciPy's label numbers them (raster order of their first pixel),
/// as [x0, y0, x1, y1] pixel bounds (ends excluded).
fn components(on: &[bool]) -> Vec<[usize; 4]> {
    let mut seen = vec![false; W * H];
    let (mut out, mut stack) = (Vec::new(), Vec::new());
    for p in 0..W * H {
        if !on[p] || seen[p] {
            continue;
        }
        let mut bb = [W, H, 0, 0];
        seen[p] = true;
        stack.push(p);
        while let Some(q) = stack.pop() {
            let (x, y) = (q % W, q / W);
            bb = [bb[0].min(x), bb[1].min(y), bb[2].max(x + 1), bb[3].max(y + 1)];
            let mut visit = |r: usize| {
                if on[r] && !seen[r] {
                    seen[r] = true;
                    stack.push(r);
                }
            };
            if x > 0 {
                visit(q - 1);
            }
            if x + 1 < W {
                visit(q + 1);
            }
            if y > 0 {
                visit(q - W);
            }
            if y + 1 < H {
                visit(q + W);
            }
        }
        out.push(bb);
    }
    out
}

// ---- features -------------------------------------------------------------------------------------------------------

/// A box's pixels: columns x0..x1 and rows y0..y1 of the 1280 x 720 frame.
#[derive(Clone, Copy, Debug)]
struct Rect {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
}

impl Rect {
    fn size(self) -> usize {
        (self.x1 - self.x0) * (self.y1 - self.y0)
    }
}

/// A box's pixel bounds as Python rounds them: int(round(v * size)).
fn px(b: &[f64; 4]) -> [i64; 4] {
    [b[0] * W as f64, b[1] * H as f64, b[2] * W as f64, b[3] * H as f64].map(|v| v.round_ties_even() as i64)
}

/// A box's pixels as NumPy slices them: cut to the frame, empty when an end comes before its start.
fn rect(b: &[f64; 4]) -> Rect {
    let [x0, y0, x1, y1] = px(b);
    let cut = |v: i64, s: usize| v.clamp(0, s as i64) as usize;
    let (x0, y0) = (cut(x0, W), cut(y0, H));
    Rect { x0, y0, x1: cut(x1, W).max(x0), y1: cut(y1, H).max(y0) }
}

/// What an area looks like, for the learner (python/areas.py: features): its center and size (shares of the frame),
/// the share of it that is fixed, how much it changes over the run (the change map's mean / 40), and its text rows
/// (up to 12, / 12), each rounded to 4 decimals. `fixed`: whether a pixel is fixed; `change`: the change map's mean
/// over a non-empty box, / 40.
fn features(b: &[f64; 4], fixed: impl Fn(usize) -> bool, change: impl Fn(Rect) -> f64) -> Vec<f64> {
    let r = rect(b);
    let (w, size) = (r.x1 - r.x0, r.size());
    let count: usize = (r.y0..r.y1).map(|y| (r.x0..r.x1).filter(|&x| fixed(y * W + x)).count()).sum();
    let cols = if w > 16 { r.x0 + 6..r.x1 - 6 } else { r.x0..r.x1 }; // inside a box's border
    let on: Vec<bool> = (r.y0..r.y1).map(|y| cols.clone().filter(|&x| fixed(y * W + x)).count() > 1).collect();
    let rows = review_rows(&on).len();
    let (share, ch) = if size > 0 { (count as f64 / size as f64, change(r)) } else { (0.0, 0.0) };
    [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0, b[2] - b[0], b[3] - b[1], share, ch, rows.min(12) as f64 / 12.0]
        .map(|v| round(v, 4))
        .to_vec()
}

/// Text rows in an area: runs of rows with more than one fixed pixel, at least 3 rows tall (python/areas.py:
/// review_rows, given each row's "more than one" already).
pub fn review_rows(on: &[bool]) -> Vec<Range<usize>> {
    let (mut out, mut cur) = (Vec::new(), None);
    for (i, &v) in on.iter().enumerate() {
        match (v, cur) {
            (true, None) => cur = Some(i),
            (false, Some(c)) => {
                if i - c >= 3 {
                    out.push(c..i);
                }
                cur = None;
            }
            _ => {}
        }
    }
    if let Some(c) = cur
        && on.len() - c >= 3
    {
        out.push(c..on.len());
    }
    out
}

/// A float NumPy sums (float32 or float64).
trait Float: Copy + Default + Add<Output = Self> {}
impl Float for f32 {}
impl Float for f64 {}

/// NumPy's pairwise summation (pairwise_sum in loops_utils.h.src): 8 running sums in blocks of up to 128 values.
fn pairwise<T: Float>(a: &[T]) -> T {
    let n = a.len();
    if n < 8 {
        a.iter().fold(T::default(), |r, &v| r + v)
    } else if n <= 128 {
        let mut r = [a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7]];
        let mut i = 8;
        while i < n - n % 8 {
            for (j, rj) in r.iter_mut().enumerate() {
                *rj = *rj + a[i + j];
            }
            i += 8;
        }
        let mut res = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
        for &v in &a[i..] {
            res = res + v;
        }
        res
    } else {
        let n2 = n / 2;
        let n2 = n2 - n2 % 8;
        pairwise(&a[..n2]) + pairwise(&a[n2..])
    }
}

/// NumPy's sum of a box of a 1280 x 720 map (`map[y0:y1, x0:x1].sum()`), in its order: a box of whole rows (or one
/// row) is one pairwise sum; any other is summed 8192 / width rows at a time (the reduction's buffer), each block
/// pairwise, the blocks one after another. Checked against NumPy 2.5 on random boxes, float32 and float64.
fn numpy_sum<T: Float>(r: Rect, value: impl Fn(usize) -> T) -> T {
    let (w, h) = (r.x1 - r.x0, r.y1 - r.y0);
    let block = |rows: Range<usize>| -> Vec<T> {
        rows.flat_map(|y| (r.x0..r.x1).map(move |x| y * W + x)).map(&value).collect()
    };
    if h <= 1 || w == W {
        return pairwise(&block(r.y0..r.y1));
    }
    let k = (8192 / w).max(1);
    (r.y0..r.y1).step_by(k).fold(T::default(), |acc, y| acc + pairwise(&block(y..(y + k).min(r.y1))))
}

// ---- the crosshair zoom ---------------------------------------------------------------------------------------------

/// Pillow's 8-bit bilinear resize of a w x h image to 16 x 16 (Image.resize with BILINEAR on an "L" image): the
/// weights in 22-bit fixed point, across then down, each pass rounded to bytes.
fn resize16(src: &[u8], w: usize, h: usize) -> Vec<u8> {
    const BITS: u32 = 22;
    let fixed = |len: usize| -> Vec<(usize, Vec<i64>)> {
        taps(len, 16, 1.0, bilinear)
            .into_iter()
            .map(|(lo, k)| {
                let k = k.iter().map(|&v| {
                    if v < 0.0 { (v * (1 << BITS) as f64 - 0.5) as i64 } else { (v * (1 << BITS) as f64 + 0.5) as i64 }
                });
                (lo, k.collect())
            })
            .collect()
    };
    let clip = |v: i64| (v >> BITS).clamp(0, 255) as u8;
    let half = 1i64 << (BITS - 1);
    let tmp: Vec<u8> = if w == 16 {
        src.to_vec()
    } else {
        let across = fixed(w);
        (0..h)
            .flat_map(|y| {
                across.iter().map(move |(lo, k)| {
                    clip(k.iter().enumerate().fold(half, |s, (x, &kx)| s + src[y * w + lo + x] as i64 * kx))
                })
            })
            .collect()
    };
    if h == 16 {
        return tmp;
    }
    let down = fixed(h);
    down.iter()
        .flat_map(|(lo, k)| {
            let tmp = &tmp;
            (0..16).map(move |x| {
                clip(k.iter().enumerate().fold(half, |s, (y, &ky)| s + tmp[(lo + y) * 16 + x] as i64 * ky))
            })
        })
        .collect()
}

/// NumPy's float32 mean of a whole array: a pairwise sum, divided in float64.
fn mean32(a: &[f32]) -> f32 {
    (pairwise(a) as f64 / a.len() as f64) as f32
}

/// (a - a.mean()) / (a.std() + 1e-6), in float32 as NumPy does it.
fn standardized(a: Vec<f32>) -> Vec<f32> {
    let mean = mean32(&a);
    let dev: Vec<f32> = a.iter().map(|&v| (v - mean) * (v - mean)).collect();
    let std = mean32(&dev).sqrt();
    let den = std + 1e-6f32;
    a.into_iter().map(|v| (v - mean) / den).collect()
}

/// A box of every frame's Y plane, each scaled to 16 x 16, one after another, standardized.
fn thumbnails(ys: &[Vec<u8>], cols: Range<usize>, rows: Range<usize>) -> Vec<f32> {
    let (w, h) = (cols.len(), rows.len());
    let all: Vec<f32> = ys
        .iter()
        .flat_map(|y| {
            let crop: Vec<u8> =
                rows.clone().flat_map(|r| y[r * W + cols.start..r * W + cols.end].iter().copied()).collect();
            resize16(&crop, w, h)
        })
        .map(|v| v as f32)
        .collect();
    standardized(all)
}

/// A magnified copy of the screen around the crosshair (a crosshair zoom; python/areas.py: zoomed): over the frames,
/// the area's picture follows the center's, scaled down by some zoom. True when one zoom (1.5 to 8) correlates 0.8 or
/// more. Areas smaller than 24 pixels either way, and runs of fewer than 10 frames, are never one.
pub fn zoomed(b: &[f64; 4], ys: &[Vec<u8>]) -> bool {
    let r = rect(b);
    let (w, h) = (r.x1 - r.x0, r.y1 - r.y0);
    if w < 24 || h < 24 || ys.len() < 10 {
        return false;
    }
    let area = thumbnails(ys, r.x0..r.x1, r.y0..r.y1);
    let (cx, cy) = (CX as usize, CY as usize);
    for zoom in [1.5, 2.0, 3.0, 4.0, 6.0, 8.0] {
        let hw = ((w as f64 / zoom / 2.0) as usize).max(4);
        let hh = ((h as f64 / zoom / 2.0) as usize).max(4);
        if hw > cx || hh > cy {
            continue;
        }
        let mid = thumbnails(ys, cx - hw..(cx + hw).min(W), cy - hh..(cy + hh).min(H));
        let both: Vec<f32> = area.iter().zip(&mid).map(|(a, m)| a * m).collect();
        if mean32(&both) as f64 >= 0.8 {
            return true;
        }
    }
    false
}

// ---- naming ---------------------------------------------------------------------------------------------------------

fn area(b: &[f64; 4]) -> f64 {
    (b[2] - b[0]) * (b[3] - b[1])
}

fn overlap(a: &[f64; 4], b: &[f64; 4]) -> f64 {
    let pos = |v: f64| if v > 0.0 { v } else { 0.0 };
    pos(a[2].min(b[2]) - a[0].max(b[0])) * pos(a[3].min(b[3]) - a[1].max(b[1]))
}

/// The share of box b inside box o.
pub fn inside(b: &[f64; 4], o: &[f64; 4]) -> f64 {
    overlap(b, o) / area(b).max(1e-9)
}

/// Intersection over union of two boxes.
pub fn iou(a: &[f64; 4], b: &[f64; 4]) -> f64 {
    let inter = overlap(a, b);
    inter / (area(a) + area(b) - inter).max(1e-9)
}

/// Step 1: the kind from where the area is and what it does (python/areas.py: rule_kind). `feat`: its features;
/// `session`: KovaaK's session box; `aim`: Aim Lab's boxes and their kinds.
pub fn rule_kind(
    b: &[f64; 4],
    feat: &[f64],
    session: Option<&[f64; 4]>,
    aim: Option<&[([f64; 4], &'static str)]>,
) -> &'static str {
    let [cx, cy, w, h, fixed, _, rows] = [0, 1, 2, 3, 4, 5, 6].map(|i| feat.get(i).copied().unwrap_or(0.0));
    if session.is_some_and(|s| inside(b, s) > 0.5) {
        return SESSION;
    }
    for (a, kind) in aim.unwrap_or_default() {
        if inside(b, a) > 0.3 {
            return kind;
        }
    }
    if w * h > 0.015 && fixed < 0.2 && rows * 12.0 <= 2.0 {
        // big, its content does not stay put (a hand, moving or still: only its border is fixed), and not rows of text
        return "Webcam";
    }
    if cy < 0.15 && 0.35 < cx && cx < 0.65 {
        return "Timer";
    }
    if cy < 0.18 && !(0.2..=0.8).contains(&cx) {
        return "Clock";
    }
    if cy > 0.85 && 0.3 < cx && cx < 0.7 {
        return "Scenario name";
    }
    if cy > 0.75 && !(0.35..=0.65).contains(&cx) && rows * 12.0 >= 3.0 {
        return "Settings";
    }
    if cy > 0.9 && w * h < 0.002 && !(0.1..=0.9).contains(&cx) {
        return "Version";
    }
    "Other"
}

// ---- step 2: learning from the user's saved areas -------------------------------------------------------------------

/// The examples the user's saved areas give (python/areas.py: learn, without writing the file: `merge` does that).
/// With the recording's maps, every saved area is an example of its kind, drawn by hand or not, and each found area
/// that lies in no saved one an example of "none" (the user removed it). Without them, each found area takes the kind
/// of the saved area that fits it best.
pub fn learn(rec: &str, found: &[Area], saved: &[SavedBox], maps: Option<&Maps>) -> Vec<Example> {
    let example = |feat: Vec<f64>, kind: &str| Example { rec: rec.to_string(), feat, kind: kind.to_string() };
    if let Some(m) = maps {
        let mut ex: Vec<Example> =
            saved.iter().map(|b| example(m.features(&b.bounds), b.kind.as_deref().unwrap_or("other"))).collect();
        for a in found {
            if !saved.iter().any(|b| inside(&a.bounds, &b.bounds) > 0.5) {
                ex.push(example(a.feat.clone(), NONE));
            }
        }
        return ex;
    }
    found
        .iter()
        .map(|a| {
            // areas can overlap: the one that fits it best, not the biggest that holds it
            let mut best: Option<(f64, usize)> = None;
            for (j, s) in saved.iter().enumerate() {
                if inside(&a.bounds, &s.bounds) > 0.5 && best.is_none_or(|(v, _)| iou(&a.bounds, &s.bounds) > v) {
                    best = Some((iou(&a.bounds, &s.bounds), j));
                }
            }
            let kind = match best {
                Some((_, j)) => saved[j].kind.as_deref().unwrap_or("Other"),
                None => NONE,
            };
            example(a.feat.clone(), kind)
        })
        .collect()
}

/// The examples file after learning from a recording (python/areas.py: learn's write): the lines of other recordings
/// kept as they are (the recording's own, and its "kovobs:" ones, are replaced: the user's labels replace the
/// layout's), then the new examples, one JSON object a line as Python writes them.
pub fn merge(lines: &str, rec: &str, new: &[Example]) -> String {
    let layout = format!("kovobs:{rec}");
    let mut out = String::new();
    for line in lines.lines().filter(|l| !l.trim().is_empty()) {
        let theirs = serde_json::from_str::<Value>(line)
            .ok()
            .and_then(|v| v.get("rec").and_then(Value::as_str).map(str::to_string));
        if theirs.as_deref().is_some_and(|r| r == rec || r == layout) {
            continue;
        }
        out.push_str(line.trim_end_matches('\r'));
        out.push('\n');
    }
    for e in new {
        out.push_str(&example_line(e));
        out.push('\n');
    }
    out
}

/// An example as Python's json.dumps writes it.
pub fn example_line(e: &Example) -> String {
    let feat: Vec<String> = e.feat.iter().map(|&v| py_float(v)).collect();
    format!("{{\"rec\": {}, \"feat\": [{}], \"kind\": {}}}", py_str(&e.rec), feat.join(", "), py_str(&e.kind))
}

/// Python's repr of a float: the shortest digits that read back the same, in plain notation from 1e-4 up to 1e16.
fn py_float(v: f64) -> String {
    if !v.is_finite() {
        return if v.is_nan() {
            "NaN".into()
        } else if v > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    let sci = format!("{v:e}");
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let (sign, digits) = mantissa.strip_prefix('-').map_or(("", mantissa), |m| ("-", m));
    let digits: String = digits.chars().filter(|c| *c != '.').collect();
    if v == 0.0 {
        return format!("{sign}0.0");
    }
    if (-4..16).contains(&exp) {
        let point = exp + 1;
        if point <= 0 {
            format!("{sign}0.{}{digits}", "0".repeat((-point) as usize))
        } else if point as usize >= digits.len() {
            format!("{sign}{digits}{}.0", "0".repeat(point as usize - digits.len()))
        } else {
            format!("{sign}{}.{}", &digits[..point as usize], &digits[point as usize..])
        }
    } else {
        let m = if digits.len() > 1 { format!("{}.{}", &digits[..1], &digits[1..]) } else { digits };
        format!("{sign}{m}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
    }
}

/// A string as Python's json.dumps writes it (ensure_ascii: every character past ASCII as \uXXXX).
fn py_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{u:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Step 2 for one area: its kind from its k nearest examples, when 3 or more agree and they are near (their median
/// distance under 0.08); None: the rule decides. Distances in float32 as NumPy's; examples equally far keep their
/// order (NumPy's argsort may order such ties otherwise).
fn vote(feat: &[f64], examples: &[Example], k: usize) -> Option<String> {
    let v: Vec<f32> = feat.iter().map(|&x| x as f32).collect();
    let mut d: Vec<(f32, usize)> = examples
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let sq: Vec<f32> = e.feat.iter().zip(&v).map(|(&x, &y)| (x as f32 - y) * (x as f32 - y)).collect();
            (pairwise(&sq).sqrt(), i)
        })
        .collect();
    d.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let near = &d[..k.min(d.len())];
    let mut votes: Vec<(&str, usize)> = Vec::new();
    for &(_, i) in near {
        match votes.iter_mut().find(|(kind, _)| *kind == examples[i].kind) {
            Some(v) => v.1 += 1,
            None => votes.push((&examples[i].kind, 1)),
        }
    }
    let (kind, n) = votes.iter().fold(("", 0), |best, &(kind, n)| if n > best.1 { (kind, n) } else { best });
    let mut ds: Vec<f32> = near.iter().map(|x| x.0).collect();
    ds.sort_by(f32::total_cmp);
    let m = ds.len();
    let median = if m == 0 {
        f32::NAN
    } else if m % 2 == 1 {
        ds[m / 2]
    } else {
        ((ds[m / 2 - 1] + ds[m / 2]) as f64 / 2.0) as f32
    };
    (n >= 3 && (median as f64) < 0.08).then(|| kind.to_string())
}

/// Step 2: each found area's kind from its k nearest examples (other recordings' saved areas), when 3 or more agree
/// and they are near; else the rule's kind (python/areas.py: predict). "none": the user removes such areas, so they
/// are left out.
pub fn predict(found: &[Area], examples: &[Example], k: usize) -> Vec<Named> {
    let named = |a: &Area, kind: String, by: &str| Named { area: a.clone(), kind, by: by.into() };
    if examples.len() < k {
        return found.iter().map(|a| named(a, a.rule.clone(), "rule")).collect();
    }
    found
        .iter()
        .map(|a| match vote(&a.feat, examples, k) {
            Some(kind) => named(a, kind, "learned"),
            None => named(a, a.rule.clone(), "rule"),
        })
        .filter(|a| a.kind != NONE)
        .collect()
}

/// How alike two recordings' overlays are: each found area's best overlap with the other's, averaged both ways (1:
/// the same areas in the same places).
pub fn same_layout(found: &[Area], other: &[Area]) -> f64 {
    if found.is_empty() || other.is_empty() {
        return 0.0;
    }
    let one = |xs: &[Area], ys: &[Area]| {
        xs.iter()
            .map(|x| ys.iter().map(|y| iou(&x.bounds, &y.bounds)).fold(f64::NEG_INFINITY, f64::max))
            .fold(0.0, |s, v| s + v)
            / xs.len() as f64
    };
    one(found, other).min(one(other, found))
}

/// A recording the user saved areas for: its found areas and its saved areas.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Labelled {
    pub rec: String,
    pub found: Vec<Area>,
    pub saved: Vec<SavedBox>,
}

impl<'de> Deserialize<'de> for Labelled {
    /// {rec, found, saved}, or [rec, found, saved] as python/server.py's labelled() gives them.
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Labelled, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Either {
            Named { rec: String, found: Vec<Area>, saved: Vec<SavedBox> },
            Tuple(String, Vec<Area>, Vec<SavedBox>),
        }
        Ok(match Either::deserialize(d)? {
            Either::Named { rec, found, saved } | Either::Tuple(rec, found, saved) => Labelled { rec, found, saved },
        })
    }
}

/// The areas to propose for a recording.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Proposal {
    /// [x0, y0, x1, y1, kind] each.
    pub boxes: Vec<SavedBox>,
    /// The recording whose saved areas were copied, None when the found areas were named.
    pub copied: Option<String>,
    /// How many were named by the learner ("learned") and by the rules ("rule"); empty when copied.
    pub by: BTreeMap<String, usize>,
}

/// The areas to propose (python/areas.py: find, after the found areas are known). When the found areas sit where
/// those of a recording the user already labelled do (same_layout 0.5 or more), that recording's saved areas, as the
/// user drew and named them (the first such recording with the best match); else the found areas, named by the
/// learner or the rules.
pub fn find(found: &[Area], examples: &[Example], labelled: &[Labelled]) -> Proposal {
    let mut best: Option<(f64, &Labelled)> = None;
    for l in labelled {
        let s = same_layout(found, &l.found);
        if best.is_none_or(|(b, _)| s > b) {
            best = Some((s, l));
        }
    }
    if let Some((s, l)) = best
        && s >= 0.5
    {
        let boxes = l
            .saved
            .iter()
            .map(|b| SavedBox { bounds: b.bounds, kind: Some(b.kind.clone().unwrap_or_else(|| "Other".into())) })
            .collect();
        return Proposal { boxes, copied: Some(l.rec.clone()), by: BTreeMap::new() };
    }
    let named = predict(found, examples, K);
    let by = ["learned", "rule"].map(|k| (k.to_string(), named.iter().filter(|a| a.by == k).count())).into();
    let boxes = named.into_iter().map(|a| SavedBox { bounds: a.area.bounds, kind: Some(a.kind) }).collect();
    Proposal { boxes, copied: None, by }
}

/// Leave one recording out: each example's kind predicted from the other recordings' examples (python/areas.py:
/// check).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Check {
    /// The share the learner names (the rest: not sure, the rules decide).
    pub sure: f64,
    /// The share of those it names right.
    pub right: f64,
    /// The examples.
    pub count: usize,
    /// [truth, guess, count] for each wrong pair, in order.
    pub wrong: Vec<(String, String, usize)>,
}

pub fn check(examples: &[Example]) -> Check {
    let mut recs: Vec<&str> = examples.iter().map(|e| e.rec.as_str()).collect();
    recs.sort_unstable();
    recs.dedup();
    let (mut right, mut sure, mut wrong) = (0usize, 0usize, BTreeMap::<(String, String), usize>::new());
    for rec in recs {
        let rest: Vec<Example> = examples.iter().filter(|e| e.rec != rec).cloned().collect();
        for e in examples.iter().filter(|e| e.rec == rec) {
            let guess = if rest.len() < K { None } else { vote(&e.feat, &rest, K) };
            let Some(k) = guess else {
                continue; // not sure: the rules decide
            };
            sure += 1;
            if k == e.kind {
                right += 1;
            } else {
                *wrong.entry((e.kind.clone(), k)).or_default() += 1;
            }
        }
    }
    Check {
        sure: sure as f64 / examples.len().max(1) as f64,
        right: right as f64 / sure.max(1) as f64,
        count: examples.len(),
        wrong: wrong.into_iter().map(|((t, g), n)| (t, g, n)).collect(),
    }
}

/// An area type (area_kinds.json): its id and name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Kind {
    pub id: String,
    pub name: String,
}

/// A type's id from its id or its name (saved areas from before ids held names); unknown: "other"
/// (python/server.py: kind_id).
pub fn kind_id(v: &str, kinds: &[Kind]) -> String {
    if kinds.iter().any(|k| k.id == v) {
        return v.to_string();
    }
    let lower = v.to_lowercase();
    kinds.iter().find(|k| k.name.to_lowercase() == lower).map_or_else(|| "other".into(), |k| k.id.clone())
}

/// Boxes with their kinds as type ids (python/server.py: with_ids).
pub fn with_ids(boxes: &[SavedBox], kinds: &[Kind]) -> Vec<SavedBox> {
    boxes
        .iter()
        .map(|b| SavedBox { bounds: b.bounds, kind: Some(kind_id(b.kind.as_deref().unwrap_or("other"), kinds)) })
        .collect()
}

// ---- JSON in and out (the WebAssembly exports and the desktop app) ---------------------------------------------------

/// Examples as the text of area_examples.jsonl, or as a list.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum Examples {
    Lines(String),
    List(Vec<Example>),
}

impl Default for Examples {
    fn default() -> Examples {
        Examples::List(Vec::new())
    }
}

impl Examples {
    /// The examples; lines that are not an example are left out (as an empty line is).
    pub fn list(&self) -> Vec<Example> {
        match self {
            Examples::Lines(text) => text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect(),
            Examples::List(v) => v.clone(),
        }
    }

    /// The examples as JSON lines.
    pub fn lines(&self) -> String {
        match self {
            Examples::Lines(text) => text.clone(),
            Examples::List(v) => v.iter().map(|e| example_line(e) + "\n").collect(),
        }
    }
}

fn parse<'a, T: Deserialize<'a>>(text: &'a str) -> Result<T, String> {
    serde_json::from_str(text).map_err(|e| e.to_string())
}

fn text(v: &impl Serialize) -> Result<String, String> {
    serde_json::to_string(v).map_err(|e| e.to_string())
}

/// {keys, times, duration} -> null (read the key frames) or [frame index, ...] (`sample_frames`).
pub fn sample_json(input: &str) -> Result<String, String> {
    #[derive(Deserialize)]
    struct In {
        keys: usize,
        times: Vec<f64>,
        duration: f64,
    }
    let i: In = parse(input)?;
    text(&sample_frames(i.keys, &i.times, i.duration))
}

/// {found, examples, labelled, kinds?} -> {boxes, copied, by, examples, recordings} (python/server.py: find_areas):
/// `labelled` as {rec, found, saved} or [rec, found, saved], `kinds` (area_kinds.json) turns the boxes' kinds into
/// type ids; `examples` and `recordings` count the examples and the recordings (not "kovobs:" ones) they are from.
pub fn find_json(input: &str) -> Result<String, String> {
    #[derive(Deserialize)]
    struct In {
        found: Vec<Area>,
        #[serde(default)]
        examples: Examples,
        #[serde(default)]
        labelled: Vec<Labelled>,
        #[serde(default)]
        kinds: Option<Vec<Kind>>,
    }
    let i: In = parse(input)?;
    let ex = i.examples.list();
    let mut p = find(&i.found, &ex, &i.labelled);
    if let Some(kinds) = &i.kinds {
        p.boxes = with_ids(&p.boxes, kinds);
    }
    let mut recs: Vec<&str> = ex.iter().map(|e| e.rec.as_str()).filter(|r| !r.starts_with("kovobs:")).collect();
    recs.sort_unstable();
    recs.dedup();
    text(&json!({"boxes": p.boxes, "copied": p.copied, "by": p.by, "examples": ex.len(), "recordings": recs.len()}))
}

/// {rec, found, saved, maps?, examples?, kinds?} -> {examples: the new area_examples.jsonl text, added}
/// (python/areas.py: learn; python/server.py's set_exclude gives the saved boxes type ids first, as `kinds` does).
pub fn learn_json(input: &str) -> Result<String, String> {
    #[derive(Deserialize)]
    struct In {
        rec: String,
        #[serde(default)]
        found: Vec<Area>,
        saved: Vec<SavedBox>,
        #[serde(default)]
        maps: Option<Maps>,
        #[serde(default)]
        examples: Examples,
        #[serde(default)]
        kinds: Option<Vec<Kind>>,
    }
    let i: In = parse(input)?;
    let saved = match &i.kinds {
        Some(kinds) => with_ids(&i.saved, kinds),
        None => i.saved,
    };
    let new = learn(&i.rec, &i.found, &saved, i.maps.as_ref());
    text(&json!({"examples": merge(&i.examples.lines(), &i.rec, &new), "added": new.len()}))
}

/// {found, examples, k?} -> [{box, feat, rule, kind, by}, ...] (`predict`).
pub fn predict_json(input: &str) -> Result<String, String> {
    #[derive(Deserialize)]
    struct In {
        found: Vec<Area>,
        #[serde(default)]
        examples: Examples,
        #[serde(default)]
        k: Option<usize>,
    }
    let i: In = parse(input)?;
    text(&predict(&i.found, &i.examples.list(), i.k.unwrap_or(K)))
}

/// {found, other} -> a number (`same_layout`).
pub fn same_layout_json(input: &str) -> Result<String, String> {
    #[derive(Deserialize)]
    struct In {
        found: Vec<Area>,
        other: Vec<Area>,
    }
    let i: In = parse(input)?;
    text(&same_layout(&i.found, &i.other))
}

/// {examples} -> {sure, right, count, wrong: [[truth, guess, n], ...]} (`check`).
pub fn check_json(input: &str) -> Result<String, String> {
    #[derive(Deserialize)]
    struct In {
        examples: Examples,
    }
    let i: In = parse(input)?;
    text(&check(&i.examples.list()))
}

// ---- packing the maps -----------------------------------------------------------------------------------------------
//
// Each map is packed without loss (a LOCO-I style coder). Its values are first replaced by their rank among the values
// it has (the stand-out map has only one value for each count of frames). Each pixel is then predicted from its
// neighbors (the median edge detector), and the difference is written as a Rice code whose size adapts to how busy
// the neighborhood is. Where the neighbors are all equal, a run of equal pixels is written as its length.

/// Unary prefixes this long or longer are followed by the value's 8 bits instead.
const LIMIT: u32 = 20;
const CONTEXTS: usize = 8;

struct BitWriter {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl BitWriter {
    fn put(&mut self, v: u64, len: u32) {
        self.acc |= v << self.n;
        self.n += len;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    fn ones(&mut self, q: u32, stop: bool) {
        self.put((1u64 << q) - 1, q);
        if stop {
            self.put(0, 1);
        }
    }
}

struct BitReader<'a> {
    data: &'a [u8],
    i: usize,
    acc: u64,
    n: u32,
}

impl BitReader<'_> {
    fn get(&mut self, len: u32) -> u64 {
        while self.n < len {
            self.acc |= (*self.data.get(self.i).unwrap_or(&0) as u64) << self.n;
            self.i += 1;
            self.n += 8;
        }
        let v = self.acc & ((1u64 << len) - 1);
        self.acc >>= len;
        self.n -= len;
        v
    }

    fn ones(&mut self, limit: u32) -> u32 {
        let mut q = 0;
        while q < limit && self.get(1) == 1 {
            q += 1;
        }
        q
    }
}

/// Each context's running sum of codes and count, for its Rice parameter.
struct Stats {
    a: [u32; CONTEXTS],
    n: [u32; CONTEXTS],
}

impl Stats {
    fn new() -> Stats {
        Stats { a: [4; CONTEXTS], n: [1; CONTEXTS] }
    }

    fn k(&self, c: usize) -> u32 {
        let mut k = 0;
        while k < 7 && (self.n[c] << k) < self.a[c] {
            k += 1;
        }
        k
    }

    fn update(&mut self, c: usize, u: u32) {
        self.a[c] += u;
        self.n[c] += 1;
        if self.n[c] >= 64 {
            self.a[c] = (self.a[c] + 1) >> 1;
            self.n[c] >>= 1;
        }
    }
}

/// A pixel's neighbors (left, up, up-left, up-right), the row above taken as zeros on the first row and the edge
/// pixels standing in past the sides.
fn neighbors(img: &[u8], w: usize, x: usize, y: usize) -> (u8, u8, u8, u8) {
    let up = |x: usize| if y > 0 { img[(y - 1) * w + x] } else { 0 };
    let b = up(x);
    let a = if x > 0 { img[y * w + x - 1] } else { b };
    let c = if x > 0 { up(x - 1) } else { b };
    let d = if x + 1 < w { up(x + 1) } else { b };
    (a, b, c, d)
}

fn predicted(a: u8, b: u8, c: u8) -> u8 {
    if c >= a.max(b) {
        a.min(b)
    } else if c <= a.min(b) {
        a.max(b)
    } else {
        (a as i16 + b as i16 - c as i16) as u8
    }
}

fn context(a: u8, b: u8, c: u8, d: u8) -> usize {
    let g = d.abs_diff(b) as u32 + b.abs_diff(c) as u32 + c.abs_diff(a) as u32;
    ((32 - g.leading_zeros()) as usize).min(CONTEXTS - 1)
}

/// A map (w x h bytes) packed: the number of values it has less one, the values, then the ranks' code.
fn pack(img: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut present = [false; 256];
    img.iter().for_each(|&v| present[v as usize] = true);
    let values: Vec<u8> = (0..=255).filter(|&v| present[v as usize]).collect();
    let mut rank = [0u8; 256];
    for (i, &v) in values.iter().enumerate() {
        rank[v as usize] = i as u8;
    }
    let ranks: Vec<u8> = img.iter().map(|&v| rank[v as usize]).collect();
    let mut out = vec![values.len().saturating_sub(1) as u8];
    out.extend(&values);
    out.extend(code(&ranks, w, h));
    out
}

/// A packed map back to its w x h bytes; None when it is not one.
fn unpack(data: &[u8], w: usize, h: usize) -> Option<Vec<u8>> {
    let n = *data.first()? as usize + 1;
    let values = data.get(1..1 + n)?;
    decode(&data[1 + n..], w, h)?.into_iter().map(|r| values.get(r as usize).copied()).collect()
}

/// An image's code (w x h bytes).
fn code(img: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut bits = BitWriter { out: Vec::new(), acc: 0, n: 0 };
    let mut stats = Stats::new();
    for y in 0..h {
        let mut x = 0;
        let mut force = false;
        while x < w {
            let (a, b, c, d) = neighbors(img, w, x, y);
            let ctx = context(a, b, c, d);
            if ctx == 0 && !force {
                let run = img[y * w + x..(y + 1) * w].iter().take_while(|&&v| v == a).count();
                let m = run as u64 + 1;
                let len = 63 - m.leading_zeros();
                bits.ones(len, true);
                bits.put(m & ((1 << len) - 1), len);
                x += run;
                force = true;
                continue;
            }
            force = false;
            let e = (img[y * w + x] as i32 - predicted(a, b, c) as i32 + 384) % 256 - 128;
            let u = if e >= 0 { 2 * e } else { -2 * e - 1 } as u32;
            let k = stats.k(ctx);
            let q = u >> k;
            if q < LIMIT {
                bits.ones(q, true);
                bits.put((u & ((1 << k) - 1)) as u64, k);
            } else {
                bits.ones(LIMIT, false);
                bits.put(u as u64, 8);
            }
            stats.update(ctx, u);
            x += 1;
        }
    }
    let mut out = bits.out;
    if bits.n > 0 {
        out.push(bits.acc as u8);
    }
    out
}

/// A code back to its image (w x h bytes); None when the data runs out.
fn decode(data: &[u8], w: usize, h: usize) -> Option<Vec<u8>> {
    let mut r = BitReader { data, i: 0, acc: 0, n: 0 };
    let mut img = vec![0u8; w * h];
    let mut stats = Stats::new();
    for y in 0..h {
        let mut x = 0;
        let mut force = false;
        while x < w {
            let (a, b, c, d) = neighbors(&img, w, x, y);
            let ctx = context(a, b, c, d);
            if ctx == 0 && !force {
                let len = r.ones(32);
                if len >= 32 {
                    return None;
                }
                let run = (((1u64 << len) | r.get(len)) - 1) as usize;
                if run > w - x {
                    return None;
                }
                img[y * w + x..y * w + x + run].fill(a);
                x += run;
                force = true;
                continue;
            }
            force = false;
            let k = stats.k(ctx);
            let q = r.ones(LIMIT);
            let u = if q < LIMIT { (q << k) | r.get(k) as u32 } else { r.get(8) as u32 };
            if u > 255 {
                return None;
            }
            stats.update(ctx, u);
            let e = if u % 2 == 0 { (u / 2) as i32 } else { -((u as i32 + 1) / 2) };
            img[y * w + x] = (predicted(a, b, c) as i32 + e).rem_euclid(256) as u8;
            x += 1;
        }
    }
    (r.i <= data.len()).then_some(img)
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let v =
            (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[(v >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn unbase64(s: &str) -> Option<Vec<u8>> {
    let s = s.trim_end_matches('=').as_bytes();
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut n) = (0u32, 0);
    for &c in s {
        let v = B64.iter().position(|&b| b == c)? as u32;
        acc = acc << 6 | v;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((acc >> n) as u8);
            acc &= (1 << n) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(b: [f64; 4], rule: &str) -> Area {
        Area { bounds: b, feat: vec![0.5; 7], rule: rule.into() }
    }

    #[test]
    fn maps_pack_and_unpack_to_the_same_bytes() {
        let mut seed = 7u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 24) as u8
        };
        let (w, h) = (97, 31);
        let flat = vec![0u8; w * h];
        let ramp: Vec<u8> = (0..w * h).map(|i| ((i % w) * 3 + i / w) as u8).collect();
        let random: Vec<u8> = (0..w * h).map(|_| noise()).collect();
        let mixed: Vec<u8> = (0..w * h).map(|i| if (i / 7) % 5 == 0 { noise() } else { 200 }).collect();
        for img in [flat, ramp, random, mixed] {
            let packed = pack(&img, w, h);
            assert_eq!(unpack(&packed, w, h).as_deref(), Some(&img[..]));
            assert_eq!(unbase64(&base64(&packed)), Some(packed));
        }
        // an empty map: a run a row
        assert!(pack(&vec![0u8; W * H], W, H).len() < 2000);
        let maps = Maps::new(vec![3; W * H], (0..W * H).map(|i| (i % 251) as u8).collect()).unwrap();
        let text = serde_json::to_string(&maps).unwrap();
        assert_eq!(serde_json::from_str::<Maps>(&text).unwrap(), maps);
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(unbase64("TWE="), Some(b"Ma".to_vec()));
    }

    #[test]
    fn floats_and_strings_are_written_as_python_writes_them() {
        for (v, s) in
            [(0.1328, "0.1328"), (0.0, "0.0"), (1.0, "1.0"), (0.0001, "0.0001"), (1e-5, "1e-05"), (12.5, "12.5")]
        {
            assert_eq!(py_float(v), s);
        }
        assert_eq!(py_float(1e16), "1e+16");
        assert_eq!(py_float(123456789012345.0), "123456789012345.0");
        assert_eq!(py_str("uploads/1902 \u{ff5c} #2.mp4"), "\"uploads/1902 \\uff5c #2.mp4\"");
        assert_eq!(py_str("a\"b\\\n\u{1f600}"), "\"a\\\"b\\\\\\n\\ud83d\\ude00\"");
        let e = Example { rec: "r".into(), feat: vec![0.5, 0.0833], kind: "clock".into() };
        assert_eq!(example_line(&e), r#"{"rec": "r", "feat": [0.5, 0.0833], "kind": "clock"}"#);
    }

    #[test]
    fn text_rows_are_runs_of_three_or_more() {
        let on = [true, true, true, false, true, true, false, true, true, true, true];
        assert_eq!(review_rows(&on), vec![0..3, 7..11]);
    }

    #[test]
    fn numpy_sums_follow_its_order() {
        // the order itself is checked against NumPy by examples/areas.rs; here, that every pixel is summed once
        let v = |i: usize| (i % 7) as f64;
        for r in [
            Rect { x0: 0, y0: 3, x1: W, y1: 40 },
            Rect { x0: 5, y0: 0, x1: 13, y1: H },
            Rect { x0: 9, y0: 2, x1: 10, y1: 3 },
        ] {
            let direct: f64 = (r.y0..r.y1).flat_map(|y| (r.x0..r.x1).map(move |x| v(y * W + x))).sum();
            assert_eq!(numpy_sum(r, v), direct);
        }
        // 8 values or more: eight running sums, then the rest
        assert_eq!(pairwise(&[1e8f32, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 4.0]), ((1e8f32 + 1.0) + 2.0) + 4.0 + 4.0);
    }

    #[test]
    fn pillow_bilinear_matches_its_bytes() {
        // Pillow 12.3's Image.resize((16, 16), BILINEAR) of the same patterns
        let pattern = |h: usize, w: usize| -> Vec<u8> {
            (0..h).flat_map(|y| (0..w).map(move |x| ((x * 7 + y * 13 + (x * y) % 11) % 256) as u8)).collect()
        };
        let out = resize16(&pattern(30, 47), 47, 30);
        assert_eq!(out.iter().map(|&v| v as u32).sum::<u32>(), 31983);
        assert_eq!(&out[..16], &[18, 38, 60, 80, 98, 121, 142, 162, 181, 203, 219, 200, 88, 29, 51, 66]);
        let col: Vec<u8> = (0..16).map(|y| out[y * 16 + 5]).collect();
        assert_eq!(col, [121, 145, 169, 193, 219, 221, 91, 37, 60, 84, 108, 131, 156, 182, 206, 228]);
        let up = resize16(&pattern(8, 8), 8, 8);
        assert_eq!(up.iter().map(|&v| v as u32).sum::<u32>(), 18997);
        assert_eq!(&up[..16], &[0, 2, 5, 9, 12, 16, 19, 23, 26, 30, 33, 37, 40, 44, 47, 49]);
        let tall = resize16(&pattern(100, 37), 37, 100);
        assert_eq!(tall.iter().map(|&v| v as u32).sum::<u32>(), 32688);
        let col: Vec<u8> = (0..16).map(|y| tall[y * 16 + 5]).collect();
        assert_eq!(col, [131, 190, 88, 112, 184, 92, 100, 176, 106, 87, 168, 121, 75, 156, 152, 63]);
    }

    #[test]
    fn growing_closes_gaps_and_keeps_off_the_edge() {
        // SciPy's binary_dilation(binary_closing(f, iterations=5), iterations=2), labelled, of the same blocks: two
        // blocks 3 pixels apart join, a dot at the frame's edge is closed away, a block at the top loses its edge rows
        let mut fixed = vec![false; W * H];
        let mut set = |xs: Range<usize>, ys: Range<usize>| {
            for y in ys {
                fixed[y * W + xs.start..y * W + xs.end].fill(true);
            }
        };
        set(100..110, 100..110);
        set(113..123, 100..110);
        set(0..1, 300..301);
        set(300..301, 300..301);
        set(600..640, 0..20);
        assert_eq!(components(&grow(&fixed)), vec![[598, 3, 642, 22], [98, 98, 125, 112], [298, 298, 303, 303]]);
    }

    #[test]
    fn rules_name_areas_by_place() {
        let feat = |cx: f64, cy: f64, w: f64, h: f64, fixed: f64, rows: f64| {
            vec![cx, cy, w, h, fixed, 0.1, round(rows / 12.0, 4)]
        };
        let b = [0.0, 0.0, 0.1, 0.1];
        assert_eq!(rule_kind(&b, &feat(0.5, 0.05, 0.05, 0.04, 0.4, 1.0), None, None), "Timer");
        assert_eq!(rule_kind(&b, &feat(0.95, 0.05, 0.05, 0.04, 0.4, 1.0), None, None), "Clock");
        assert_eq!(rule_kind(&b, &feat(0.9, 0.85, 0.17, 0.25, 0.1, 1.0), None, None), "Webcam");
        assert_eq!(rule_kind(&b, &feat(0.5, 0.93, 0.1, 0.1, 0.5, 1.0), None, None), "Scenario name");
        assert_eq!(rule_kind(&b, &feat(0.2, 0.9, 0.3, 0.1, 0.1, 3.0), None, None), "Settings");
        assert_eq!(rule_kind(&b, &feat(0.03, 0.98, 0.04, 0.03, 0.4, 1.0), None, None), "Version");
        assert_eq!(rule_kind(&b, &feat(0.5, 0.5, 0.04, 0.03, 0.4, 1.0), None, None), "Other");
        assert_eq!(rule_kind(&b, &feat(0.05, 0.05, 0.1, 0.1, 0.4, 5.0), Some(&[0.0, 0.0, 0.12, 0.12]), None), SESSION);
        // two rows round to 0.1667, and 0.1667 * 12 is more than 2: not a webcam (as in Python)
        assert_eq!(rule_kind(&b, &feat(0.5, 0.5, 0.2, 0.2, 0.1, 2.0), None, None), "Other");
    }

    #[test]
    fn the_learner_votes_and_leaves_removed_areas_out() {
        let ex = |rec: &str, x: f64, kind: &str| Example {
            rec: rec.into(),
            feat: vec![x, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1],
            kind: kind.into(),
        };
        let examples = vec![
            ex("a", 0.5, "clock"),
            ex("b", 0.51, "clock"),
            ex("c", 0.52, "clock"),
            ex("d", 0.9, "timer"),
            ex("e", 0.49, "none"),
        ];
        let mut a = found([0.0, 0.0, 0.1, 0.1], "Other");
        a.feat = vec![0.5, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1];
        let named = predict(std::slice::from_ref(&a), &examples, K);
        assert_eq!((named[0].kind.as_str(), named[0].by.as_str()), ("clock", "learned"));
        // too few examples: the rules
        assert_eq!(predict(std::slice::from_ref(&a), &examples[..4], K)[0].by, "rule");
        let removed =
            vec![ex("a", 0.5, NONE), ex("b", 0.5, NONE), ex("c", 0.5, NONE), ex("d", 0.5, "x"), ex("e", 0.5, "y")];
        assert!(predict(&[a], &removed, K).is_empty());
        let c = check(&examples);
        assert_eq!(c.count, 5);
    }

    #[test]
    fn learning_replaces_the_recordings_examples() {
        let saved = vec![
            SavedBox { bounds: [0.0, 0.0, 0.2, 0.2], kind: Some("clock".into()) },
            SavedBox { bounds: [0.5, 0.5, 0.6, 0.6], kind: None },
        ];
        let f = vec![found([0.01, 0.01, 0.19, 0.19], "Clock"), found([0.8, 0.8, 0.9, 0.9], "Other")];
        let ex = learn("r", &f, &saved, None);
        assert_eq!(ex.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(), ["clock", NONE]);
        let maps = Maps::new(vec![255; W * H], vec![40; W * H]).unwrap();
        let ex = learn("r", &f, &saved, Some(&maps));
        assert_eq!(ex.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(), ["clock", "other", NONE]);
        assert_eq!(ex[0].feat, vec![0.1, 0.1, 0.2, 0.2, 1.0, 1.0, 0.0833]);
        let old = "{\"rec\": \"q\", \"feat\": [1.0], \"kind\": \"x\"}\n{\"rec\": \"kovobs:r\", \"feat\": [1.0], \"kind\": \"x\"}\n";
        let lines = merge(old, "r", &ex);
        assert_eq!(lines.lines().count(), 4);
        assert!(lines.starts_with("{\"rec\": \"q\""));
    }

    #[test]
    fn find_copies_a_labelled_layout_or_names_the_found_areas() {
        let f = vec![found([0.0, 0.0, 0.1, 0.1], "Clock")];
        let saved = vec![SavedBox { bounds: [0.0, 0.0, 0.11, 0.1], kind: None }];
        let l = Labelled { rec: "other".into(), found: f.clone(), saved };
        let p = find(&f, &[], std::slice::from_ref(&l));
        assert_eq!(p.copied.as_deref(), Some("other"));
        assert_eq!(p.boxes[0].kind.as_deref(), Some("Other"));
        let p = find(&f, &[], &[]);
        assert_eq!((p.copied, p.by["rule"]), (None, 1));
        assert_eq!(same_layout(&f, &f), 1.0);
        let kinds = vec![Kind { id: "clock".into(), name: "Clock".into() }];
        assert_eq!(with_ids(&p.boxes, &kinds)[0].kind.as_deref(), Some("clock"));
        assert_eq!(kind_id("FPS", &kinds), "other");
    }

    #[test]
    fn few_key_frames_take_ninety_frames_over_the_run() {
        assert_eq!(sample_frames(24, &[0.0], 10.0), None);
        // 60 fps for 10 s: 10 frames a second, the first frame of each tenth
        let times: Vec<f64> = (0..600).map(|i| i as f64 / 60.0).collect();
        let picked = sample_frames(5, &times, 10.0).unwrap();
        assert_eq!(picked.len(), 100);
        // output frame k: the last frame whose time rounds to k tenths or less
        assert_eq!(&picked[..4], &[2, 8, 14, 20]);
        assert!(picked.windows(2).all(|p| p[0] < p[1]));
    }

    #[test]
    fn json_calls_read_and_write_pythons_shapes() {
        let found =
            r#"[{"box": [0.0, 0.0, 0.1, 0.1], "feat": [0.05, 0.05, 0.1, 0.1, 0.4, 0.2, 0.0833], "rule": "Clock"}]"#;
        let out = find_json(&format!(
            r#"{{"found": {found}, "examples": "", "labelled": [["x", {found}, [[0, 0, 0.1, 0.1, "Clock"]]]], "kinds": [{{"id": "clock", "name": "Clock"}}]}}"#
        ))
        .unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["copied"], "x");
        assert_eq!(v["boxes"][0][4], "clock");
        let out = learn_json(&format!(r#"{{"rec": "y", "found": {found}, "saved": [], "examples": ""}}"#)).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["added"], 1);
        assert!(v["examples"].as_str().unwrap().contains("\"kind\": \"none\""));
        assert_eq!(same_layout_json(&format!(r#"{{"found": {found}, "other": {found}}}"#)).unwrap(), "1.0");
        assert!(sample_json(r#"{"keys": 30, "times": [], "duration": 1}"#).unwrap() == "null");
    }
}
