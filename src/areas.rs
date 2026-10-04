//! The area finder: a recording's overlay areas (HUD boxes, clocks, a webcam) for the run page's Excluded areas
//! editor, and what each one is (python/areas.py, all of it).
//!
//! In: the review's frames, 1280 x 720 YUV 4:2:0 (`convert::Converter::yuv420p`, the same bytes ffmpeg gives Python):
//! the run's key frames, or the frames `sample_frames` picks when it has few (service/src/review.rs and finder.rs feed
//! them natively, the page's area finder worker through src/wasm.rs); KovaaK's session box as the HUD watch finds it
//! (src/hud.rs); and, for naming, the areas the user saved (exclude.json) and the examples they gave
//! (area_examples.jsonl). Out: the found areas and the maps they came from (`Found`, kept as areas.json), the areas
//! proposed for a recording (`find`), and the examples the user's saved areas give (`learn`, `merge`), to
//! service/src/areas.rs and the WebAssembly exports.
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
//! where Python's maps are float32, and the crosshair zoom test scales with Pillow's 8-bit bilinear filter.
//!
//! The pure functions take and give the JSON python/areas.py and python/server.py use: found areas as areas.json holds
//! them, saved areas as exclude.json holds them, and examples as the lines of area_examples.jsonl.

use std::collections::BTreeMap;
use std::ops::{Add, Range, RangeInclusive};

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
pub const NEAREST_EXAMPLES: usize = 5;
/// One frame: YUV 4:2:0 at 1280 x 720.
pub const FRAME: usize = W * H * 3 / 2;
/// The features of an area (`features`).
pub const FEATURES: usize = 7;

/// The kinds the rules give, by name.
const SESSION: &str = "Session stats";
const ZOOMED: &str = "Zoomed crosshair";
const TIMER: &str = "Timer";
const OTHER: &str = "Other";
/// The type id of an area saved without a kind, or of an unknown kind.
const OTHER_ID: &str = "other";
/// Who named a proposed area (`Named::by`).
const BY_LEARNER: &str = "learned";
const BY_RULE: &str = "rule";
/// The examples learned from the KovOBS layout are from recordings named with this prefix.
const LAYOUT_PREFIX: &str = "kovobs:";

/// A map holds bytes: a share of the frames times MAP_MAX, or a change cut to 0..MAP_MAX.
const MAP_MAX: f64 = 255.0;
/// More than this share of the frame fixed: the view hardly moved (a probe run), and the room itself stays put.
const STILL_SHARE: f64 = 0.15;
/// Found boxes and features are rounded to this many decimals.
const DECIMALS: usize = 4;

// ---- which frames ---------------------------------------------------------------------------------------------------

/// Which frames the area finder reads (python/areas.py: sample): None when the run has MIN_KEYS key frames or more
/// (then it reads every key frame, in order); else the indexes of the frames to give it, in order, a frame given again
/// where an index repeats. `times_s`: every frame's time in seconds (frame 0 is the first from time 0 on),
/// `duration_s`: the recording's duration as ffprobe gives it (`format=duration`). The indexes are those ffmpeg's `fps`
/// filter picks at N_SAMPLES / (duration_s - 1) frames a second (python/areas.py's command): output frame k is the last
/// frame whose time, in output frames, rounds to k or less, until the end of the last frame.
pub fn sample_frames(keys: usize, times_s: &[f64], duration_s: f64) -> Option<Vec<usize>> {
    (keys < MIN_KEYS).then(|| fps_frames(times_s, duration_s, N_SAMPLES))
}

/// python/areas.py spreads the frames over the recording's duration less TRIMMED_END_S, and over at least MIN_SPAN_S.
const TRIMMED_END_S: f64 = 1.0;
const MIN_SPAN_S: f64 = 1.0;
/// It writes the rate into the fps filter with this many decimals.
const RATE_DECIMALS: usize = 5;

/// The frames ffmpeg's fps filter picks for `count` frames over the recording (`sample_frames`).
fn fps_frames(times_s: &[f64], duration_s: f64, count: usize) -> Vec<usize> {
    let Some(&last_s) = times_s.last() else {
        return Vec::new();
    };
    let span_s = (duration_s - TRIMMED_END_S).max(MIN_SPAN_S);
    let rate: f64 = format!("{:.*}", RATE_DECIMALS, count as f64 / span_s).parse().unwrap_or(1.0);
    let output_frame = |time_s: f64| (time_s * rate).round() as i64;
    // the end of the stream: the last frame's time and its length (the one before it)
    let last_length_s = if times_s.len() > 1 { last_s - times_s[times_s.len() - 2] } else { 0.0 };
    let end = output_frame(last_s + last_length_s);
    let (mut frame, mut output) = (0, output_frame(times_s[0]));
    let mut picks = Vec::with_capacity((end - output).max(0) as usize);
    while output < end {
        while frame + 1 < times_s.len() && output_frame(times_s[frame + 1]) <= output {
            frame += 1;
        }
        picks.push(frame);
        output += 1;
    }
    picks
}

// ---- the data -------------------------------------------------------------------------------------------------------

/// A found area, as areas.json holds it: its box (shares of the frame, x0, y0, x1, y1), its features (`features`) and
/// the kind the rules give it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Area {
    #[serde(rename = "box")]
    pub bounds: [f64; 4],
    pub feat: [f64; FEATURES],
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
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut values: Vec<Value> = self.bounds.iter().map(|&share| json!(share)).collect();
        if let Some(kind) = &self.kind {
            values.push(json!(kind));
        }
        values.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SavedBox {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<SavedBox, D::Error> {
        let values = Vec::<Value>::deserialize(deserializer)?;
        let share =
            |i: usize| values.get(i).and_then(Value::as_f64).ok_or_else(|| D::Error::custom("a box: 4 numbers"));
        Ok(SavedBox {
            bounds: [share(0)?, share(1)?, share(2)?, share(3)?],
            kind: values.get(4).and_then(|kind| kind.as_str()).map(str::to_string),
        })
    }
}

/// One example for the learner (a line of area_examples.jsonl): the recording, an area's features, and its kind.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Example {
    pub rec: String,
    pub feat: [f64; FEATURES],
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
    stand: Box<[u8; W * H]>,
    change: Box<[u8; W * H]>,
}

/// The maps as JSON: their width and height, and each map packed and in base64.
#[derive(Serialize, Deserialize)]
#[expect(clippy::min_ident_chars, reason = "the JSON's field names, which areas.json and the UI read")]
struct MapsText {
    w: usize,
    h: usize,
    stand: String,
    change: String,
}

impl Serialize for Maps {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let text = |map: &[u8]| base64(&pack(map, W, H));
        MapsText { w: W, h: H, stand: text(&self.stand[..]), change: text(&self.change[..]) }.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Maps {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Maps, D::Error> {
        let text = MapsText::deserialize(deserializer)?;
        if (text.w, text.h) != (W, H) {
            return Err(D::Error::custom("maps: not 1280 x 720"));
        }
        let plane = |packed: &str| {
            unbase64(packed).and_then(|bytes| unpack(&bytes, W, H)).ok_or_else(|| D::Error::custom("maps: unreadable"))
        };
        Maps::new(plane(&text.stand)?, plane(&text.change)?).ok_or_else(|| D::Error::custom("maps: not 1280 x 720"))
    }
}

impl Maps {
    /// From the two maps' bytes (W x H each, row by row); None when a size is wrong.
    pub fn new(stand: Vec<u8>, change: Vec<u8>) -> Option<Maps> {
        Some(Maps { stand: stand.try_into().ok()?, change: change.try_into().ok()? })
    }

    pub fn stand(&self) -> &[u8] {
        &self.stand[..]
    }

    pub fn change(&self) -> &[u8] {
        &self.change[..]
    }

    /// An area's features from the maps as kept (python/areas.py's learn, with maps() reading the npz back: the
    /// stand-out map as float32 shares of 255, the change map as float32).
    pub fn features(&self, bounds: &[f64; 4]) -> [f64; FEATURES] {
        features(
            bounds,
            |i| self.stand[i] as f32 / MAP_MAX as f32 >= FIXED as f32,
            |rect| {
                let pixels = rect.size() as f64;
                let mean = (numpy_sum(rect, |i| self.change[i] as f32) as f64 / pixels) as f32;
                (mean / CHANGE_SCALE as f32) as f64
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
    stood_out_frames: Box<[u16; W * H]>,
    /// Each frame's Y plane.
    y_planes: Vec<Box<[u8]>>,
}

impl Default for AreaFinder {
    fn default() -> AreaFinder {
        AreaFinder { stood_out_frames: vec![0; W * H].try_into().unwrap(), y_planes: Vec::new() }
    }
}

impl AreaFinder {
    pub fn new() -> AreaFinder {
        AreaFinder::default()
    }

    /// One frame, YUV 4:2:0 at 1280 x 720 (FRAME bytes); a shorter buffer is left out.
    pub fn add(&mut self, yuv: &[u8]) {
        if yuv.len() >= FRAME {
            self.add_contrast(yuv, &contrast(yuv));
        }
    }

    /// The same, with the frame's `contrast` already worked out (the fixed map's, on the same key frame).
    pub fn add_contrast(&mut self, yuv: &[u8], frame_contrast: &[f32]) {
        if yuv.len() < FRAME {
            return;
        }
        for (frames, &pixel_contrast) in self.stood_out_frames.iter_mut().zip(frame_contrast) {
            *frames += (pixel_contrast > DIFF) as u16;
        }
        self.y_planes.push(yuv[..W * H].into());
    }

    /// The frames added.
    pub fn frames(&self) -> usize {
        self.y_planes.len()
    }

    /// Whether an area (shares of the frame) is a magnified copy of the screen around the crosshair, over the frames
    /// added so far (`zoomed`).
    pub fn zoomed(&self, bounds: &[f64; 4]) -> bool {
        zoomed(bounds, &self.y_planes)
    }

    /// The found areas and the maps. `session`: KovaaK's session box as the HUD watch finds it in the key frames
    /// (`hud::HudWatch::session_box`), None without one.
    pub fn finish(self, session: Option<SessionRows>) -> Found {
        let frames = self.y_planes.len();
        let stand = stand_out_shares(&self.stood_out_frames[..], frames);
        let change = mean_changes(&self.y_planes);
        let maps = Maps::new(
            stand.iter().map(|&share| (share * MAP_MAX).round_ties_even() as u8).collect(),
            change.iter().map(|&mean| mean.clamp(0.0, MAP_MAX) as u8).collect(),
        )
        .expect("W x H maps");
        let fixed: Vec<bool> = stand.iter().map(|&share| share >= FIXED).collect();
        // when the view hardly moved, overlays cannot be told from the room
        let areas = if frames == 0 || hardly_moved(&fixed) {
            Vec::new()
        } else {
            find_areas(&PixelMaps { stand: &stand, fixed: &fixed, change: &change }, &self.y_planes, session)
        };
        Found { frames, areas, maps }
    }
}

/// Per pixel, the share of the frames it stood out in.
fn stand_out_shares(stood_out_frames: &[u16], frames: usize) -> Vec<f64> {
    stood_out_frames.iter().map(|&count| if frames > 0 { count as f64 / frames as f64 } else { 0.0 }).collect()
}

/// Per pixel, its mean brightness change from one frame to the next (0 with fewer than two frames).
fn mean_changes(y_planes: &[Box<[u8]>]) -> Vec<f64> {
    let mut sums = vec![0u32; W * H];
    for pair in y_planes.windows(2) {
        for ((sum, &a), &b) in sums.iter_mut().zip(&pair[0]).zip(&pair[1]) {
            *sum += a.abs_diff(b) as u32;
        }
    }
    let pairs = y_planes.len().saturating_sub(1);
    sums.iter().map(|&sum| if pairs > 0 { sum as f64 / pairs as f64 } else { 0.0 }).collect()
}

/// Whether more than STILL_SHARE of the frame is fixed.
fn hardly_moved(fixed: &[bool]) -> bool {
    fixed.iter().filter(|&&is_fixed| is_fixed).count() as f64 / (W * H) as f64 > STILL_SHARE
}

/// KovaaK's box reaches this many pixels (of hud::BW x BH) above its first value row and below its last.
const SESSION_ROW_MARGIN: usize = 8;
/// Its own border, in pixels at 1280 x 720, is added around that.
const SESSION_BORDER_PX: f64 = 8.0;

/// KovaaK's session box as shares of the frame, with its border around the rows the HUD watch reads.
pub fn session_share(session: SessionRows) -> [f64; 4] {
    let (x_scale, y_scale) = (BOX[2] / BW as f64, BOX[3] / BH as f64);
    let border = SESSION_BORDER_PX / W as f64;
    [
        non_negative(session.x0 as f64 * x_scale - border),
        non_negative((session.y0 as f64 - SESSION_ROW_MARGIN as f64) * y_scale - border),
        session.x1 as f64 * x_scale + border,
        (session.y1 + SESSION_ROW_MARGIN) as f64 * y_scale + border,
    ]
}

/// The maps the areas are found in, per pixel at 1280 x 720: the share of the frames it stood out in, whether that
/// makes it fixed, and its mean change from frame to frame.
struct PixelMaps<'a> {
    stand: &'a [f64],
    fixed: &'a [bool],
    change: &'a [f64],
}

/// The fixed pixels around KovaaK's session box are cut away this many pixels past it.
const SESSION_CUT_PX: i64 = 4;
/// Found areas more than this share inside KovaaK's session box give way to it.
const SESSION_PART_SHARE: f64 = 0.6;
/// An area more than this share inside a bigger one (text inside a box, a webcam's details) is part of it.
const PART_SHARE: f64 = 0.8;

/// The areas in the fixed pixels, named by the rules, or as a crosshair zoom.
fn find_areas(maps: &PixelMaps, y_planes: &[Box<[u8]>], session: Option<SessionRows>) -> Vec<Area> {
    let session_box = session.map(session_share);
    let mut grown = grow(maps.fixed);
    if let Some(session_box) = &session_box {
        // so a clock beside it is an area of its own
        cut_out(&mut grown, session_box);
    }
    let aim = aim_boxes(maps.stand, maps.fixed);
    let mut boxes = component_boxes(&grown, maps.fixed);
    let mut session_at = None;
    if let Some(session_box) = session_box {
        // the session box as the HUD watch finds it, whole
        boxes.retain(|bounds| inside(bounds, &session_box) < SESSION_PART_SHARE);
        boxes.push(session_box);
        session_at = Some(boxes.len() - 1);
    }
    let outermost = outermost(&boxes, session_at);
    boxes
        .iter()
        .zip(outermost)
        .filter(|&(_, is_outermost)| is_outermost)
        .map(|(bounds, _)| {
            let feat = features(
                bounds,
                |i| maps.fixed[i],
                |rect| numpy_sum(rect, |i| maps.change[i]) / rect.size() as f64 / CHANGE_SCALE,
            );
            let rule = if zoomed(bounds, y_planes) {
                ZOOMED.to_string()
            } else {
                rule_kind(bounds, &feat, session_box.as_ref(), aim.as_ref()).to_string()
            };
            Area { bounds: bounds.map(|share| round(share, DECIMALS)), feat, rule }
        })
        .collect()
}

/// The grown pixels around a box (shares of the frame) cut away, SESSION_CUT_PX past it.
fn cut_out(grown: &mut [bool], bounds: &[f64; 4]) {
    let [x0, y0, x1, y1] = pixel_bounds(bounds);
    let rows = (y0 - SESSION_CUT_PX).max(0) as usize..((y1 + SESSION_CUT_PX).max(0) as usize).min(H);
    let columns = (x0 - SESSION_CUT_PX).max(0) as usize..((x1 + SESSION_CUT_PX).max(0) as usize).min(W);
    for y in rows {
        grown[y * W..(y + 1) * W][columns.clone()].fill(false);
    }
}

/// An area is at least MIN_AREA_PX pixels and MIN_SIDE_PX either way, and holds a fixed pixel; else it is too small,
/// or a sliver of a box's border.
const MIN_AREA_PX: usize = 80;
const MIN_SIDE_PX: usize = 7;
/// An area over the screen's center narrower than this is the crosshair, which is never an area.
const CROSSHAIR_MAX_WIDTH_PX: usize = 120;

/// The grown areas' boxes as shares of the frame, in SciPy's order, without those too small and the crosshair.
fn component_boxes(grown: &[bool], fixed: &[bool]) -> Vec<[f64; 4]> {
    let mut boxes = Vec::new();
    for [x0, y0, x1, y1] in components(grown) {
        let (width, height) = (x1 - x0, y1 - y0);
        let any_fixed = (y0..y1).any(|y| fixed[y * W + x0..y * W + x1].iter().any(|&is_fixed| is_fixed));
        if width * height < MIN_AREA_PX || width.min(height) < MIN_SIDE_PX || !any_fixed {
            continue;
        }
        let holds_center = x0 as f64 <= CX && CX <= x1 as f64 && y0 as f64 <= CY && CY <= y1 as f64;
        if holds_center && width < CROSSHAIR_MAX_WIDTH_PX {
            continue;
        }
        boxes.push([x0 as f64 / W as f64, y0 as f64 / H as f64, x1 as f64 / W as f64, y1 as f64 / H as f64]);
    }
    boxes
}

/// For each box, whether it is kept: it is the session box (at `session_at`), or it is part of no bigger box.
fn outermost(boxes: &[[f64; 4]], session_at: Option<usize>) -> Vec<bool> {
    (0..boxes.len())
        .map(|i| {
            let bounds = &boxes[i];
            let part_of = |(j, other): (usize, &[f64; 4])| {
                j != i && box_area(other) > box_area(bounds) && inside(bounds, other) > PART_SHARE
            };
            session_at == Some(i) || !boxes.iter().enumerate().any(part_of)
        })
        .collect()
}

/// Aim Lab's ACCURACY box starts this share of the frame after the TIME box.
const AIM_BOX_GAP: f64 = 0.005;

/// Aim Lab's POINTS and TIME boxes where their values stand out, and the ACCURACY box after them (as wide as POINTS);
/// None unless both are there.
fn aim_boxes(stand: &[f64], fixed: &[bool]) -> Option<AimBoxes> {
    let band = AIM_BAND;
    let (band_top, band_bottom) = ((band[1] * H as f64) as usize, (band[3] * H as f64) as usize);
    let x_scale = (band[2] - band[0]) / AW as f64;
    if !fixed[band_top * W..band_bottom * W].iter().any(|&is_fixed| is_fixed) {
        return None;
    }
    let [Some(points), Some(time)] = [(AIM_POINTS, SESSION), (AIM_TIME, TIMER)].map(|((start, end), kind)| {
        let bounds = [band[0] + start as f64 * x_scale, band[1], band[0] + end as f64 * x_scale, band[3]];
        let (left, right) = ((bounds[0] * W as f64) as usize, (bounds[2] * W as f64) as usize);
        let stands_out = |y: usize| stand[y * W + left..y * W + right].iter().any(|&share| share >= FIXED);
        (band_top..band_bottom).any(stands_out).then_some((bounds, kind))
    }) else {
        return None;
    };
    let points_width = points.0[2] - points.0[0];
    let accuracy_left = time.0[2] + AIM_BOX_GAP;
    Some([points, time, ([accuracy_left, band[1], accuracy_left + points_width, band[3]], SESSION)])
}

/// Aim Lab's POINTS, TIME and ACCURACY boxes and their kinds.
pub type AimBoxes = [([f64; 4], &'static str); 3];

/// The fixed pixels are grown this far after the closing.
const GROW_PX: u16 = GAP / 2;

/// The fixed pixels grown into areas: closed over GAP pixels, then grown by GROW_PX (SciPy's binary_closing and
/// binary_dilation with the 4-neighbour cross; the closing's erosion counts the pixels past the frame's edge as
/// empty).
fn grow(fixed: &[bool]) -> Vec<bool> {
    let dilated: Vec<bool> = city_block(fixed, false).into_iter().map(|distance| distance <= GAP).collect();
    let holes: Vec<bool> = dilated.iter().map(|&is_on| !is_on).collect();
    let closed: Vec<bool> = city_block(&holes, true).into_iter().map(|distance| distance > GAP).collect();
    city_block(&closed, false).into_iter().map(|distance| distance <= GROW_PX).collect()
}

/// Farther than any pixel: one less than u16's largest, which saturating_add keeps from wrapping.
const FAR: u16 = u16::MAX - 1;

/// Each pixel's city-block distance to the nearest `on` pixel (W x H); `edge`: the pixels just past the frame's edge
/// count as on.
fn city_block(on: &[bool], edge: bool) -> Vec<u16> {
    let mut distances: Vec<u16> = on.iter().map(|&is_on| if is_on { 0 } else { FAR }).collect();
    let past_edge = if edge { 1 } else { FAR };
    for y in 0..H {
        for x in 0..W {
            let i = y * W + x;
            let left = if x > 0 { distances[i - 1].saturating_add(1) } else { past_edge };
            let up = if y > 0 { distances[i - W].saturating_add(1) } else { past_edge };
            distances[i] = distances[i].min(left).min(up);
        }
    }
    for y in (0..H).rev() {
        for x in (0..W).rev() {
            let i = y * W + x;
            let right = if x + 1 < W { distances[i + 1].saturating_add(1) } else { past_edge };
            let down = if y + 1 < H { distances[i + W].saturating_add(1) } else { past_edge };
            distances[i] = distances[i].min(right).min(down);
        }
    }
    distances
}

/// The 4-connected areas of `on` (W x H), in the order SciPy's label numbers them (raster order of their first pixel),
/// as [x0, y0, x1, y1] pixel bounds (ends excluded).
fn components(on: &[bool]) -> Vec<[usize; 4]> {
    let mut seen = vec![false; W * H];
    let (mut areas, mut stack) = (Vec::new(), Vec::new());
    for first in 0..W * H {
        if !on[first] || seen[first] {
            continue;
        }
        let mut bounds = [W, H, 0, 0];
        seen[first] = true;
        stack.push(first);
        while let Some(pixel) = stack.pop() {
            let (x, y) = (pixel % W, pixel / W);
            bounds = [bounds[0].min(x), bounds[1].min(y), bounds[2].max(x + 1), bounds[3].max(y + 1)];
            let mut visit = |neighbor: usize| {
                if on[neighbor] && !seen[neighbor] {
                    seen[neighbor] = true;
                    stack.push(neighbor);
                }
            };
            if x > 0 {
                visit(pixel - 1);
            }
            if x + 1 < W {
                visit(pixel + 1);
            }
            if y > 0 {
                visit(pixel - W);
            }
            if y + 1 < H {
                visit(pixel + W);
            }
        }
        areas.push(bounds);
    }
    areas
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
    /// A box's pixels as NumPy slices them: cut to the frame, empty when an end comes before its start.
    fn from_shares(bounds: &[f64; 4]) -> Rect {
        let [x0, y0, x1, y1] = pixel_bounds(bounds);
        let cut = |value: i64, size: usize| value.clamp(0, size as i64) as usize;
        let (x0, y0) = (cut(x0, W), cut(y0, H));
        Rect { x0, y0, x1: cut(x1, W).max(x0), y1: cut(y1, H).max(y0) }
    }

    fn size(self) -> usize {
        (self.x1 - self.x0) * (self.y1 - self.y0)
    }
}

/// A box's pixel bounds as Python rounds them: int(round(share * size)).
fn pixel_bounds(bounds: &[f64; 4]) -> [i64; 4] {
    [bounds[0] * W as f64, bounds[1] * H as f64, bounds[2] * W as f64, bounds[3] * H as f64]
        .map(|pixels| pixels.round_ties_even() as i64)
}

/// The change feature is the change map's mean divided by this.
const CHANGE_SCALE: f64 = 40.0;
/// The text rows feature counts up to this many rows, as a share of it.
const MAX_TEXT_ROWS: usize = 12;
/// In an area wider than BORDERED_MIN_WIDTH_PX, text rows are read BORDER_PX inside its left and right edges (off a
/// box's border).
const BORDERED_MIN_WIDTH_PX: usize = 16;
const BORDER_PX: usize = 6;
/// A row of pixels is text when more than TEXT_FIXED_PX of them are fixed, and a text row is at least
/// MIN_TEXT_ROW_PX such rows tall.
const TEXT_FIXED_PX: usize = 1;
const MIN_TEXT_ROW_PX: usize = 3;

/// What an area looks like, for the learner (python/areas.py: features): its center and size (shares of the frame),
/// the share of it that is fixed, how much it changes over the run (the change map's mean / CHANGE_SCALE), and its
/// text rows (up to MAX_TEXT_ROWS, as a share of it), each rounded to DECIMALS. `fixed`: whether a pixel is fixed;
/// `change`: the change map's mean over a non-empty box, / CHANGE_SCALE.
fn features(bounds: &[f64; 4], fixed: impl Fn(usize) -> bool, change: impl Fn(Rect) -> f64) -> [f64; FEATURES] {
    let rect = Rect::from_shares(bounds);
    let (width, size) = (rect.x1 - rect.x0, rect.size());
    let fixed_in_row = |y: usize, columns: Range<usize>| columns.filter(|&x| fixed(y * W + x)).count();
    let fixed_pixels: usize = (rect.y0..rect.y1).map(|y| fixed_in_row(y, rect.x0..rect.x1)).sum();
    let columns =
        if width > BORDERED_MIN_WIDTH_PX { rect.x0 + BORDER_PX..rect.x1 - BORDER_PX } else { rect.x0..rect.x1 };
    let is_text: Vec<bool> = (rect.y0..rect.y1).map(|y| fixed_in_row(y, columns.clone()) > TEXT_FIXED_PX).collect();
    let text_rows = review_rows(&is_text).len();
    let (fixed_share, change_share) =
        if size > 0 { (fixed_pixels as f64 / size as f64, change(rect)) } else { (0.0, 0.0) };
    [
        (bounds[0] + bounds[2]) / 2.0,
        (bounds[1] + bounds[3]) / 2.0,
        bounds[2] - bounds[0],
        bounds[3] - bounds[1],
        fixed_share,
        change_share,
        text_rows.min(MAX_TEXT_ROWS) as f64 / MAX_TEXT_ROWS as f64,
    ]
    .map(|value| round(value, DECIMALS))
}

/// Text rows in an area: runs of rows of pixels with more than TEXT_FIXED_PX fixed, at least MIN_TEXT_ROW_PX tall
/// (python/areas.py: review_rows, given whether each row has more than that already).
pub fn review_rows(is_text: &[bool]) -> Vec<Range<usize>> {
    let (mut rows, mut start) = (Vec::new(), None);
    for (i, &text) in is_text.iter().enumerate() {
        match (text, start) {
            (true, None) => start = Some(i),
            (false, Some(first)) => {
                if i - first >= MIN_TEXT_ROW_PX {
                    rows.push(first..i);
                }
                start = None;
            }
            _ => {}
        }
    }
    if let Some(first) = start
        && is_text.len() - first >= MIN_TEXT_ROW_PX
    {
        rows.push(first..is_text.len());
    }
    rows
}

/// A float NumPy sums (float32 or float64).
trait Float: Copy + Default + Add<Output = Self> {}
impl Float for f32 {}
impl Float for f64 {}

/// NumPy's pairwise summation keeps this many running sums, over blocks of up to PAIRWISE_BLOCK values.
const PAIRWISE_LANES: usize = 8;
const PAIRWISE_BLOCK: usize = 128;

/// NumPy's pairwise summation (pairwise_sum in loops_utils.h.src): PAIRWISE_LANES running sums in blocks of up to
/// PAIRWISE_BLOCK values, a longer list halved (at a multiple of PAIRWISE_LANES).
fn pairwise<T: Float>(values: &[T]) -> T {
    let len = values.len();
    if len < PAIRWISE_LANES {
        values.iter().fold(T::default(), |sum, &value| sum + value)
    } else if len <= PAIRWISE_BLOCK {
        pairwise_block(values)
    } else {
        let half = len / 2;
        let half = half - half % PAIRWISE_LANES;
        pairwise(&values[..half]) + pairwise(&values[half..])
    }
}

/// One block of PAIRWISE_LANES to PAIRWISE_BLOCK values: the running sums, added in pairs, then the rest one by one.
fn pairwise_block<T: Float>(values: &[T]) -> T {
    let len = values.len();
    let mut sums: [T; PAIRWISE_LANES] = std::array::from_fn(|i| values[i]);
    let mut i = PAIRWISE_LANES;
    while i < len - len % PAIRWISE_LANES {
        for (j, sum) in sums.iter_mut().enumerate() {
            *sum = *sum + values[i + j];
        }
        i += PAIRWISE_LANES;
    }
    let mut total = ((sums[0] + sums[1]) + (sums[2] + sums[3])) + ((sums[4] + sums[5]) + (sums[6] + sums[7]));
    for &value in &values[i..] {
        total = total + value;
    }
    total
}

/// NumPy's reduction buffer holds this many values: a box that is not whole rows is summed this many values' rows at
/// a time.
const NUMPY_BUFFER: usize = 8192;

/// NumPy's sum of a box of a 1280 x 720 map (`map[y0:y1, x0:x1].sum()`), in its order: a box of whole rows (or one
/// row) is one pairwise sum; any other is summed NUMPY_BUFFER / width rows at a time, each block pairwise, the blocks
/// one after another. Checked against NumPy 2.5 on random boxes, float32 and float64.
fn numpy_sum<T: Float>(rect: Rect, value: impl Fn(usize) -> T) -> T {
    let (width, height) = (rect.x1 - rect.x0, rect.y1 - rect.y0);
    let values = |rows: Range<usize>| -> Vec<T> {
        rows.flat_map(|y| (rect.x0..rect.x1).map(move |x| y * W + x)).map(&value).collect()
    };
    if height <= 1 || width == W {
        return pairwise(&values(rect.y0..rect.y1));
    }
    let block_rows = (NUMPY_BUFFER / width).max(1);
    (rect.y0..rect.y1)
        .step_by(block_rows)
        .fold(T::default(), |total, y| total + pairwise(&values(y..(y + block_rows).min(rect.y1))))
}

// ---- the crosshair zoom ---------------------------------------------------------------------------------------------

/// Pictures are compared at THUMBNAIL_PX x THUMBNAIL_PX.
const THUMBNAIL_PX: usize = 16;
/// Pillow's bilinear filter reaches this many source pixels either side.
const BILINEAR_SUPPORT: f64 = 1.0;
/// Pillow's 8-bit resize keeps its weights in fixed point with this many fractional bits, and adds ROUNDING before it
/// shifts a sum back to a byte.
const WEIGHT_BITS: u32 = 22;
const ROUNDING: i64 = 1 << (WEIGHT_BITS - 1);

/// Pillow's 8-bit bilinear resize of a width x height image to THUMBNAIL_PX square (Image.resize with BILINEAR on an
/// "L" image): across then down, each pass rounded to bytes, a pass left out when the size is already right.
fn thumbnail(source: &[u8], width: usize, height: usize) -> Vec<u8> {
    let across = if width == THUMBNAIL_PX { source.to_vec() } else { resize_across(source, width, height) };
    if height == THUMBNAIL_PX {
        return across;
    }
    resize_down(&across, height)
}

/// Each output pixel's first source pixel and its weights, for `len` source pixels to THUMBNAIL_PX, the weights in
/// fixed point (rounded half away from zero).
fn fixed_point_taps(len: usize) -> Vec<(usize, Vec<i64>)> {
    taps(len, THUMBNAIL_PX, BILINEAR_SUPPORT, bilinear)
        .into_iter()
        .map(|(first, weights)| {
            let fixed = weights.iter().map(|&weight| {
                if weight < 0.0 {
                    (weight * (1 << WEIGHT_BITS) as f64 - 0.5) as i64
                } else {
                    (weight * (1 << WEIGHT_BITS) as f64 + 0.5) as i64
                }
            });
            (first, fixed.collect())
        })
        .collect()
}

/// A weighted sum in fixed point back to a byte.
fn to_byte(sum: i64) -> u8 {
    (sum >> WEIGHT_BITS).clamp(0, u8::MAX as i64) as u8
}

/// Each row of a width x height image resized across to THUMBNAIL_PX.
fn resize_across(source: &[u8], width: usize, height: usize) -> Vec<u8> {
    let across = fixed_point_taps(width);
    (0..height)
        .flat_map(|y| {
            across.iter().map(move |(first, weights)| {
                let pixel = |x: usize| source[y * width + first + x] as i64;
                let weighted = |sum: i64, (x, &weight): (usize, &i64)| sum + pixel(x) * weight;
                to_byte(weights.iter().enumerate().fold(ROUNDING, weighted))
            })
        })
        .collect()
}

/// Each column of a THUMBNAIL_PX x height image resized down to THUMBNAIL_PX.
fn resize_down(across: &[u8], height: usize) -> Vec<u8> {
    let down = fixed_point_taps(height);
    down.iter()
        .flat_map(|(first, weights)| {
            (0..THUMBNAIL_PX).map(move |x| {
                let pixel = |y: usize| across[(first + y) * THUMBNAIL_PX + x] as i64;
                let weighted = |sum: i64, (y, &weight): (usize, &i64)| sum + pixel(y) * weight;
                to_byte(weights.iter().enumerate().fold(ROUNDING, weighted))
            })
        })
        .collect()
}

/// NumPy's float32 mean of a whole array: a pairwise sum, divided in float64.
fn mean32(values: &[f32]) -> f32 {
    (pairwise(values) as f64 / values.len() as f64) as f32
}

/// Added to the standard deviation before dividing by it, as python/areas.py does.
const STD_EPSILON: f32 = 1e-6;

/// (values - values.mean()) / (values.std() + STD_EPSILON), in float32 as NumPy does it.
fn standardized(values: Vec<f32>) -> Vec<f32> {
    let mean = mean32(&values);
    let squared_deviations: Vec<f32> = values.iter().map(|&value| (value - mean) * (value - mean)).collect();
    let deviation = mean32(&squared_deviations).sqrt();
    let denominator = deviation + STD_EPSILON;
    values.into_iter().map(|value| (value - mean) / denominator).collect()
}

/// A box of every frame's Y plane, each scaled to THUMBNAIL_PX square, one after another, standardized.
fn thumbnails(y_planes: &[Box<[u8]>], columns: Range<usize>, rows: Range<usize>) -> Vec<f32> {
    let (width, height) = (columns.len(), rows.len());
    let all: Vec<f32> = y_planes
        .iter()
        .flat_map(|plane| {
            let row_pixels = |row: usize| plane[row * W + columns.start..row * W + columns.end].iter().copied();
            let crop: Vec<u8> = rows.clone().flat_map(row_pixels).collect();
            thumbnail(&crop, width, height)
        })
        .map(f32::from)
        .collect();
    standardized(all)
}

/// A crosshair zoom is at least MIN_ZOOM_SIDE_PX pixels either way, and is looked for over MIN_ZOOM_FRAMES frames or
/// more.
const MIN_ZOOM_SIDE_PX: usize = 24;
const MIN_ZOOM_FRAMES: usize = 10;
/// The zooms tried; one that correlates MIN_ZOOM_CORRELATION or more makes the area a crosshair zoom.
const ZOOMS: [f64; 6] = [1.5, 2.0, 3.0, 4.0, 6.0, 8.0];
const MIN_ZOOM_CORRELATION: f64 = 0.8;
/// The screen compared reaches at least this many pixels either side of the center.
const MIN_HALF_SIDE_PX: usize = 4;

/// A magnified copy of the screen around the crosshair (a crosshair zoom; python/areas.py: zoomed): over the frames,
/// the area's picture follows the center's, scaled down by one of ZOOMS. Areas smaller than MIN_ZOOM_SIDE_PX either
/// way, and runs of fewer than MIN_ZOOM_FRAMES frames, are never one.
pub fn zoomed(bounds: &[f64; 4], y_planes: &[Box<[u8]>]) -> bool {
    let rect = Rect::from_shares(bounds);
    let (width, height) = (rect.x1 - rect.x0, rect.y1 - rect.y0);
    if width < MIN_ZOOM_SIDE_PX || height < MIN_ZOOM_SIDE_PX || y_planes.len() < MIN_ZOOM_FRAMES {
        return false;
    }
    let area = thumbnails(y_planes, rect.x0..rect.x1, rect.y0..rect.y1);
    let (center_x, center_y) = (CX as usize, CY as usize);
    for zoom in ZOOMS {
        let half_width = ((width as f64 / zoom / 2.0) as usize).max(MIN_HALF_SIDE_PX);
        let half_height = ((height as f64 / zoom / 2.0) as usize).max(MIN_HALF_SIDE_PX);
        if half_width > center_x || half_height > center_y {
            continue;
        }
        let columns = center_x - half_width..(center_x + half_width).min(W);
        let center = thumbnails(y_planes, columns, center_y - half_height..(center_y + half_height).min(H));
        let products: Vec<f32> = area.iter().zip(&center).map(|(a, b)| a * b).collect();
        if mean32(&products) as f64 >= MIN_ZOOM_CORRELATION {
            return true;
        }
    }
    false
}

// ---- naming ---------------------------------------------------------------------------------------------------------

/// A box's area, as a share of the frame's.
fn box_area(bounds: &[f64; 4]) -> f64 {
    (bounds[2] - bounds[0]) * (bounds[3] - bounds[1])
}

/// The value, or 0 when it is not above 0.
fn non_negative(value: f64) -> f64 {
    if value > 0.0 { value } else { 0.0 }
}

/// The area two boxes share.
fn overlap(a: &[f64; 4], b: &[f64; 4]) -> f64 {
    non_negative(a[2].min(b[2]) - a[0].max(b[0])) * non_negative(a[3].min(b[3]) - a[1].max(b[1]))
}

/// Areas are divided by at least this, so an empty box gives 0.
const MIN_DIVISOR_AREA: f64 = 1e-9;

/// The share of a box inside an outer box.
pub fn inside(bounds: &[f64; 4], outer: &[f64; 4]) -> f64 {
    overlap(bounds, outer) / box_area(bounds).max(MIN_DIVISOR_AREA)
}

/// Intersection over union of two boxes.
pub fn iou(a: &[f64; 4], b: &[f64; 4]) -> f64 {
    let intersection = overlap(a, b);
    intersection / (box_area(a) + box_area(b) - intersection).max(MIN_DIVISOR_AREA)
}

/// An area more than SESSION_INSIDE_SHARE inside KovaaK's session box is it; more than AIM_INSIDE_SHARE inside one of
/// Aim Lab's boxes is that box.
const SESSION_INSIDE_SHARE: f64 = 0.5;
const AIM_INSIDE_SHARE: f64 = 0.3;
/// A webcam is bigger than WEBCAM_MIN_SIZE (a share of the frame), has less than WEBCAM_MAX_FIXED_SHARE of it fixed
/// (its content does not stay put, a hand moving or still: only its border is fixed), and at most
/// WEBCAM_MAX_TEXT_ROWS text rows.
const WEBCAM_MIN_SIZE: f64 = 0.015;
const WEBCAM_MAX_FIXED_SHARE: f64 = 0.2;
const WEBCAM_MAX_TEXT_ROWS: f64 = 2.0;
/// Where an area's center is, as shares of the frame: the timer above TIMER_MAX_Y, between TIMER_MIN_X and TIMER_MAX_X
/// across; a clock above CLOCK_MAX_Y, outside CLOCK_NOT_X.
const TIMER_MAX_Y: f64 = 0.15;
const TIMER_MIN_X: f64 = 0.35;
const TIMER_MAX_X: f64 = 0.65;
const CLOCK_MAX_Y: f64 = 0.18;
const CLOCK_NOT_X: RangeInclusive<f64> = 0.2..=0.8;
/// The scenario name below SCENARIO_MIN_Y, between SCENARIO_MIN_X and SCENARIO_MAX_X across.
const SCENARIO_MIN_Y: f64 = 0.85;
const SCENARIO_MIN_X: f64 = 0.3;
const SCENARIO_MAX_X: f64 = 0.7;
/// The settings box below SETTINGS_MIN_Y, outside SETTINGS_NOT_X, with SETTINGS_MIN_TEXT_ROWS text rows or more.
const SETTINGS_MIN_Y: f64 = 0.75;
const SETTINGS_NOT_X: RangeInclusive<f64> = 0.35..=0.65;
const SETTINGS_MIN_TEXT_ROWS: f64 = 3.0;
/// The version below VERSION_MIN_Y, smaller than VERSION_MAX_SIZE, outside VERSION_NOT_X.
const VERSION_MIN_Y: f64 = 0.9;
const VERSION_MAX_SIZE: f64 = 0.002;
const VERSION_NOT_X: RangeInclusive<f64> = 0.1..=0.9;

/// Whether a value is strictly between two others.
fn between(value: f64, low: f64, high: f64) -> bool {
    low < value && value < high
}

/// Step 1: the kind from where the area is and what it does (python/areas.py: rule_kind). `feat`: its features;
/// `session`: KovaaK's session box; `aim`: Aim Lab's boxes and their kinds.
pub fn rule_kind(
    bounds: &[f64; 4],
    feat: &[f64; FEATURES],
    session: Option<&[f64; 4]>,
    aim: Option<&AimBoxes>,
) -> &'static str {
    let [center_x, center_y, width, height, fixed_share, _, text_rows_share] = *feat;
    let text_rows = text_rows_share * MAX_TEXT_ROWS as f64;
    if session.is_some_and(|session_box| inside(bounds, session_box) > SESSION_INSIDE_SHARE) {
        return SESSION;
    }
    for (aim_box, kind) in aim.into_iter().flatten() {
        if inside(bounds, aim_box) > AIM_INSIDE_SHARE {
            return kind;
        }
    }
    if width * height > WEBCAM_MIN_SIZE && fixed_share < WEBCAM_MAX_FIXED_SHARE && text_rows <= WEBCAM_MAX_TEXT_ROWS {
        return "Webcam";
    }
    if center_y < TIMER_MAX_Y && between(center_x, TIMER_MIN_X, TIMER_MAX_X) {
        return TIMER;
    }
    if center_y < CLOCK_MAX_Y && !CLOCK_NOT_X.contains(&center_x) {
        return "Clock";
    }
    if center_y > SCENARIO_MIN_Y && between(center_x, SCENARIO_MIN_X, SCENARIO_MAX_X) {
        return "Scenario name";
    }
    if center_y > SETTINGS_MIN_Y && !SETTINGS_NOT_X.contains(&center_x) && text_rows >= SETTINGS_MIN_TEXT_ROWS {
        return "Settings";
    }
    if center_y > VERSION_MIN_Y && width * height < VERSION_MAX_SIZE && !VERSION_NOT_X.contains(&center_x) {
        return "Version";
    }
    OTHER
}

// ---- step 2: learning from the user's saved areas -------------------------------------------------------------------

/// The examples the user's saved areas give (python/areas.py: learn, without writing the file: `merge` does that).
/// With the recording's maps, every saved area is an example of its kind, drawn by hand or not, and each found area
/// that lies in no saved one an example of "none" (the user removed it). Without them, each found area takes the kind
/// of the saved area that fits it best.
pub fn learn(rec: &str, found: &[Area], saved: &[SavedBox], maps: Option<&Maps>) -> Vec<Example> {
    let example = |feat: [f64; FEATURES], kind: &str| Example { rec: rec.to_string(), feat, kind: kind.to_string() };
    if let Some(maps) = maps {
        let mut examples: Vec<Example> = saved
            .iter()
            .map(|saved_box| example(maps.features(&saved_box.bounds), saved_box.kind.as_deref().unwrap_or(OTHER_ID)))
            .collect();
        for area in found {
            if !saved.iter().any(|saved_box| inside(&area.bounds, &saved_box.bounds) > LIES_IN_SHARE) {
                examples.push(example(area.feat, NONE));
            }
        }
        return examples;
    }
    found
        .iter()
        .map(|area| {
            let kind = best_fit(area, saved).map_or(NONE, |saved_box| saved_box.kind.as_deref().unwrap_or(OTHER));
            example(area.feat, kind)
        })
        .collect()
}

/// A found area lies in a saved one when more than this share of it is inside.
const LIES_IN_SHARE: f64 = 0.5;

/// The saved area a found one lies in that fits it best (the highest IoU, the first of equals): areas can overlap, and
/// the biggest that holds it need not be it.
fn best_fit<'a>(area: &Area, saved: &'a [SavedBox]) -> Option<&'a SavedBox> {
    let mut best: Option<(f64, &SavedBox)> = None;
    for saved_box in saved {
        let fits_better = |(best_iou, _): (f64, &SavedBox)| iou(&area.bounds, &saved_box.bounds) > best_iou;
        if inside(&area.bounds, &saved_box.bounds) > LIES_IN_SHARE && best.is_none_or(fits_better) {
            best = Some((iou(&area.bounds, &saved_box.bounds), saved_box));
        }
    }
    best.map(|(_, saved_box)| saved_box)
}

/// The examples file after learning from a recording (python/areas.py: learn's write): the lines of other recordings
/// kept as they are (the recording's own, and its "kovobs:" ones, are replaced: the user's labels replace the
/// layout's), then the new examples, one JSON object a line as Python writes them.
pub fn merge(lines: &str, rec: &str, new: &[Example]) -> String {
    let layout_rec = format!("{LAYOUT_PREFIX}{rec}");
    let mut out = String::new();
    for line in lines.lines().filter(|line| !line.trim().is_empty()) {
        let line_rec = serde_json::from_str::<Value>(line)
            .ok()
            .and_then(|value| value.get("rec").and_then(Value::as_str).map(str::to_string));
        if line_rec.as_deref().is_some_and(|other| other == rec || other == layout_rec) {
            continue;
        }
        out.push_str(line.trim_end_matches('\r'));
        out.push('\n');
    }
    for example in new {
        out.push_str(&example_line(example));
        out.push('\n');
    }
    out
}

/// An example as Python's json.dumps writes it.
pub fn example_line(example: &Example) -> String {
    let feat: Vec<String> = example.feat.iter().map(|&value| py_float(value)).collect();
    let (rec, kind) = (py_str(&example.rec), py_str(&example.kind));
    format!("{{\"rec\": {rec}, \"feat\": [{}], \"kind\": {kind}}}", feat.join(", "))
}

/// Python writes a float whose exponent is in this range in plain notation, others in scientific.
const PLAIN_EXPONENTS: Range<i32> = -4..16;

/// Python's repr of a float: the shortest digits that read back the same, in plain notation from 1e-4 up to 1e16.
fn py_float(value: f64) -> String {
    if !value.is_finite() {
        return if value.is_nan() {
            "NaN".into()
        } else if value > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    let scientific = format!("{value:e}");
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let (sign, digits) = mantissa.strip_prefix('-').map_or(("", mantissa), |unsigned| ("-", unsigned));
    let digits: String = digits.chars().filter(|&character| character != '.').collect();
    if value == 0.0 {
        return format!("{sign}0.0");
    }
    if PLAIN_EXPONENTS.contains(&exponent) {
        plain_float(sign, &digits, exponent)
    } else {
        scientific_float(sign, &digits, exponent)
    }
}

/// A float's sign, significant digits and exponent in plain notation, as Python's repr writes it.
fn plain_float(sign: &str, digits: &str, exponent: i32) -> String {
    let point = exponent + 1;
    if point <= 0 {
        format!("{sign}0.{}{digits}", "0".repeat((-point) as usize))
    } else if point as usize >= digits.len() {
        format!("{sign}{digits}{}.0", "0".repeat(point as usize - digits.len()))
    } else {
        format!("{sign}{}.{}", &digits[..point as usize], &digits[point as usize..])
    }
}

/// The same in scientific notation (at least two exponent digits, and its sign).
fn scientific_float(sign: &str, digits: &str, exponent: i32) -> String {
    let mantissa = if digits.len() > 1 { format!("{}.{}", &digits[..1], &digits[1..]) } else { digits.to_string() };
    format!("{sign}{mantissa}e{}{:02}", if exponent < 0 { '-' } else { '+' }, exponent.abs())
}

/// Python's json.dumps writes these characters as they are (ensure_ascii); the rest as escapes.
const PRINTABLE_ASCII: RangeInclusive<u32> = 0x20..=0x7e;

/// A string as Python's json.dumps writes it (ensure_ascii: every character past ASCII as \uXXXX).
fn py_str(text: &str) -> String {
    let mut out = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            other if !PRINTABLE_ASCII.contains(&(other as u32)) => {
                let mut units = [0u16; 2];
                for unit in other.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// The learner names an area when at least MIN_AGREEING of its nearest examples agree and their median distance is
/// under MAX_MEDIAN_DISTANCE.
const MIN_AGREEING: usize = 3;
const MAX_MEDIAN_DISTANCE: f64 = 0.08;

/// Step 2 for one area: its kind from its `nearest_count` nearest examples, when MIN_AGREEING or more agree and they
/// are near; None: the rule decides.
fn vote(feat: &[f64], examples: &[Example], nearest_count: usize) -> Option<String> {
    let nearest = nearest(feat, examples, nearest_count);
    let (kind, agreeing) = most_common(nearest.iter().map(|&(_, i)| examples[i].kind.as_str()));
    let median = median_distance(&nearest);
    (agreeing >= MIN_AGREEING && (median as f64) < MAX_MEDIAN_DISTANCE).then(|| kind.to_string())
}

/// The `count` examples nearest the features, as (distance, index), nearest first. Distances in float32 as NumPy's;
/// examples equally far keep their order (NumPy's argsort may order such ties otherwise).
fn nearest(feat: &[f64], examples: &[Example], count: usize) -> Vec<(f32, usize)> {
    let wanted: Vec<f32> = feat.iter().map(|&value| value as f32).collect();
    let square = |(&value, &want): (&f64, &f32)| (value as f32 - want) * (value as f32 - want);
    let distance = |example: &Example| {
        let squares: Vec<f32> = example.feat.iter().zip(&wanted).map(square).collect();
        pairwise(&squares).sqrt()
    };
    let mut distances: Vec<(f32, usize)> =
        examples.iter().enumerate().map(|(i, example)| (distance(example), i)).collect();
    distances.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    distances.truncate(count);
    distances
}

/// The kind most often given, and how often (the first seen of equals).
fn most_common<'a>(kinds: impl Iterator<Item = &'a str>) -> (&'a str, usize) {
    let mut votes: Vec<(&str, usize)> = Vec::new();
    for kind in kinds {
        match votes.iter_mut().find(|(voted, _)| *voted == kind) {
            Some(vote) => vote.1 += 1,
            None => votes.push((kind, 1)),
        }
    }
    votes.iter().fold(("", 0), |best, &(kind, count)| if count > best.1 { (kind, count) } else { best })
}

/// The median of the nearest examples' distances, in float32 as NumPy gives it (NaN without any).
fn median_distance(nearest: &[(f32, usize)]) -> f32 {
    let mut distances: Vec<f32> = nearest.iter().map(|&(distance, _)| distance).collect();
    distances.sort_by(f32::total_cmp);
    let len = distances.len();
    if len == 0 {
        f32::NAN
    } else if len % 2 == 1 {
        distances[len / 2]
    } else {
        ((distances[len / 2 - 1] + distances[len / 2]) as f64 / 2.0) as f32
    }
}

/// Step 2: each found area's kind from its `nearest_count` nearest examples (other recordings' saved areas), when
/// MIN_AGREEING or more agree and they are near; else the rule's kind (python/areas.py: predict). "none": the user
/// removes such areas, so they are left out.
pub fn predict(found: &[Area], examples: &[Example], nearest_count: usize) -> Vec<Named> {
    let named = |area: &Area, kind: String, by: &str| Named { area: area.clone(), kind, by: by.into() };
    if examples.len() < nearest_count {
        return found.iter().map(|area| named(area, area.rule.clone(), BY_RULE)).collect();
    }
    found
        .iter()
        .map(|area| match vote(&area.feat, examples, nearest_count) {
            Some(kind) => named(area, kind, BY_LEARNER),
            None => named(area, area.rule.clone(), BY_RULE),
        })
        .filter(|area| area.kind != NONE)
        .collect()
}

/// How alike two recordings' overlays are: each found area's best overlap with the other's, averaged both ways (1:
/// the same areas in the same places).
pub fn same_layout(found: &[Area], other: &[Area]) -> f64 {
    if found.is_empty() || other.is_empty() {
        return 0.0;
    }
    mean_best_iou(found, other).min(mean_best_iou(other, found))
}

/// Each area's best IoU with any of the others, averaged.
fn mean_best_iou(areas: &[Area], others: &[Area]) -> f64 {
    let best_iou =
        |area: &Area| others.iter().map(|other| iou(&area.bounds, &other.bounds)).fold(f64::NEG_INFINITY, f64::max);
    areas.iter().map(best_iou).fold(0.0, |sum, best| sum + best) / areas.len() as f64
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
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Labelled, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Either {
            Named { rec: String, found: Vec<Area>, saved: Vec<SavedBox> },
            Tuple(String, Vec<Area>, Vec<SavedBox>),
        }
        Ok(match Either::deserialize(deserializer)? {
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

/// The found areas sit where a labelled recording's do when `same_layout` gives this or more.
const SAME_LAYOUT: f64 = 0.5;

/// The areas to propose (python/areas.py: find, after the found areas are known). When the found areas sit where
/// those of a recording the user already labelled do (SAME_LAYOUT or more), that recording's saved areas, as the user
/// drew and named them (the first such recording with the best match); else the found areas, named by the learner or
/// the rules.
pub fn find(found: &[Area], examples: &[Example], labelled: &[Labelled]) -> Proposal {
    if let Some(recording) = most_alike(found, labelled) {
        return copied(recording);
    }
    let named = predict(found, examples, NEAREST_EXAMPLES);
    let by = [BY_LEARNER, BY_RULE].map(|by| (by.to_string(), named.iter().filter(|area| area.by == by).count())).into();
    let boxes = named.into_iter().map(|area| SavedBox { bounds: area.area.bounds, kind: Some(area.kind) }).collect();
    Proposal { boxes, copied: None, by }
}

/// The first labelled recording whose found areas are the most like these, when that is SAME_LAYOUT or more.
fn most_alike<'a>(found: &[Area], labelled: &'a [Labelled]) -> Option<&'a Labelled> {
    let mut best: Option<(f64, &Labelled)> = None;
    for recording in labelled {
        let likeness = same_layout(found, &recording.found);
        if best.is_none_or(|(best_likeness, _)| likeness > best_likeness) {
            best = Some((likeness, recording));
        }
    }
    best.filter(|&(likeness, _)| likeness >= SAME_LAYOUT).map(|(_, recording)| recording)
}

/// A labelled recording's saved areas, proposed as they are (Other where they have no kind).
fn copied(recording: &Labelled) -> Proposal {
    let with_kind = |saved_box: &SavedBox| SavedBox {
        bounds: saved_box.bounds,
        kind: Some(saved_box.kind.clone().unwrap_or_else(|| OTHER.into())),
    };
    let boxes = recording.saved.iter().map(with_kind).collect();
    Proposal { boxes, copied: Some(recording.rec.clone()), by: BTreeMap::new() }
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

/// Leave one recording out over the examples (`Check`).
pub fn check(examples: &[Example]) -> Check {
    let mut recs: Vec<&str> = examples.iter().map(|example| example.rec.as_str()).collect();
    recs.sort_unstable();
    recs.dedup();
    let (mut right, mut sure, mut wrong) = (0usize, 0usize, BTreeMap::<(String, String), usize>::new());
    for rec in recs {
        let rest: Vec<Example> = examples.iter().filter(|example| example.rec != rec).cloned().collect();
        for example in examples.iter().filter(|example| example.rec == rec) {
            let guess =
                if rest.len() < NEAREST_EXAMPLES { None } else { vote(&example.feat, &rest, NEAREST_EXAMPLES) };
            let Some(guess) = guess else {
                continue; // not sure: the rules decide
            };
            sure += 1;
            if guess == example.kind {
                right += 1;
            } else {
                *wrong.entry((example.kind.clone(), guess)).or_default() += 1;
            }
        }
    }
    Check {
        sure: sure as f64 / examples.len().max(1) as f64,
        right: right as f64 / sure.max(1) as f64,
        count: examples.len(),
        wrong: wrong.into_iter().map(|((truth, guess), count)| (truth, guess, count)).collect(),
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
pub fn kind_id(kind: &str, kinds: &[Kind]) -> String {
    if kinds.iter().any(|known| known.id == kind) {
        return kind.to_string();
    }
    let lower = kind.to_lowercase();
    let by_name = kinds.iter().find(|known| known.name.to_lowercase() == lower);
    by_name.map_or_else(|| OTHER_ID.into(), |known| known.id.clone())
}

/// Boxes with their kinds as type ids (python/server.py: with_ids).
pub fn with_ids(boxes: &[SavedBox], kinds: &[Kind]) -> Vec<SavedBox> {
    let with_id = |saved_box: &SavedBox| SavedBox {
        bounds: saved_box.bounds,
        kind: Some(kind_id(saved_box.kind.as_deref().unwrap_or(OTHER_ID), kinds)),
    };
    boxes.iter().map(with_id).collect()
}

// ---- JSON in and out (the WebAssembly exports and the desktop app) --------------------------------------------------

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
            Examples::Lines(text) => text.lines().filter_map(|line| serde_json::from_str(line).ok()).collect(),
            Examples::List(list) => list.clone(),
        }
    }

    /// The examples as JSON lines.
    pub fn lines(&self) -> String {
        match self {
            Examples::Lines(text) => text.clone(),
            Examples::List(list) => list.iter().map(|example| example_line(example) + "\n").collect(),
        }
    }
}

fn parse<'a, T: Deserialize<'a>>(text: &'a str) -> Result<T, String> {
    serde_json::from_str(text).map_err(|error| error.to_string())
}

fn text(value: &impl Serialize) -> Result<String, String> {
    serde_json::to_string(value).map_err(|error| error.to_string())
}

/// {keys, times, duration} -> null (read the key frames) or [frame index, ...] (`sample_frames`).
pub fn sample_json(input: &str) -> Result<String, String> {
    #[derive(Deserialize)]
    struct In {
        keys: usize,
        times: Vec<f64>,
        duration: f64,
    }
    let request: In = parse(input)?;
    text(&sample_frames(request.keys, &request.times, request.duration))
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
    let request: In = parse(input)?;
    let examples = request.examples.list();
    let mut proposal = find(&request.found, &examples, &request.labelled);
    if let Some(kinds) = &request.kinds {
        proposal.boxes = with_ids(&proposal.boxes, kinds);
    }
    let mut recs: Vec<&str> =
        examples.iter().map(|example| example.rec.as_str()).filter(|rec| !rec.starts_with(LAYOUT_PREFIX)).collect();
    recs.sort_unstable();
    recs.dedup();
    let Proposal { boxes, copied, by } = proposal;
    text(&json!({"boxes": boxes, "copied": copied, "by": by, "examples": examples.len(), "recordings": recs.len()}))
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
    let request: In = parse(input)?;
    let saved = match &request.kinds {
        Some(kinds) => with_ids(&request.saved, kinds),
        None => request.saved,
    };
    let new = learn(&request.rec, &request.found, &saved, request.maps.as_ref());
    text(&json!({"examples": merge(&request.examples.lines(), &request.rec, &new), "added": new.len()}))
}

/// {found, examples, k?} -> [{box, feat, rule, kind, by}, ...] (`predict`; k: the nearest examples that vote).
pub fn predict_json(input: &str) -> Result<String, String> {
    #[derive(Deserialize)]
    #[expect(clippy::min_ident_chars, reason = "the JSON's field name, which the page sends")]
    struct In {
        found: Vec<Area>,
        #[serde(default)]
        examples: Examples,
        #[serde(default)]
        k: Option<usize>,
    }
    let request: In = parse(input)?;
    text(&predict(&request.found, &request.examples.list(), request.k.unwrap_or(NEAREST_EXAMPLES)))
}

/// {found, other} -> a number (`same_layout`).
pub fn same_layout_json(input: &str) -> Result<String, String> {
    #[derive(Deserialize)]
    struct In {
        found: Vec<Area>,
        other: Vec<Area>,
    }
    let request: In = parse(input)?;
    text(&same_layout(&request.found, &request.other))
}

/// {examples} -> {sure, right, count, wrong: [[truth, guess, n], ...]} (`check`).
pub fn check_json(input: &str) -> Result<String, String> {
    #[derive(Deserialize)]
    struct In {
        examples: Examples,
    }
    let request: In = parse(input)?;
    text(&check(&request.examples.list()))
}

// ---- packing the maps -----------------------------------------------------------------------------------------------
//
// Each map is packed without loss (a LOCO-I style coder). Its values are first replaced by their rank among the values
// it has (the stand-out map has only one value for each count of frames). Each pixel is then predicted from its
// neighbors (the median edge detector), and the difference is written as a Rice code whose size adapts to how busy
// the neighborhood is. Where the neighbors are all equal, a run of equal pixels is written as its length.

/// A code's unary quotient stops at LIMIT ones; the code's ESCAPE_BITS bits follow instead of its remainder.
const LIMIT: u32 = 20;
const ESCAPE_BITS: u32 = 8;
/// How busy a pixel's neighborhood is picks one of this many contexts, each with its own Rice parameter.
const CONTEXTS: usize = 8;
/// A context starts as a sum of codes of INITIAL_SUM over INITIAL_COUNT codes; both are halved when the count reaches
/// HALVING_COUNT, so the parameter follows the recent pixels.
const INITIAL_SUM: u32 = 4;
const INITIAL_COUNT: u32 = 1;
const HALVING_COUNT: u32 = 64;
/// The largest Rice parameter.
const MAX_RICE: u32 = 7;
/// A run's length has a unary prefix of fewer than this many ones.
const MAX_RUN_PREFIX: u32 = 32;
/// The values a byte can take.
const BYTE_VALUES: usize = 256;

/// Writes bits, the first in the lowest bit of each byte.
#[derive(Default)]
struct BitWriter {
    out: Vec<u8>,
    bits: u64,
    bit_count: u32,
}

impl BitWriter {
    fn put(&mut self, value: u64, len: u32) {
        self.bits |= value << self.bit_count;
        self.bit_count += len;
        while self.bit_count >= u8::BITS {
            self.out.push(self.bits as u8);
            self.bits >>= u8::BITS;
            self.bit_count -= u8::BITS;
        }
    }

    /// `count` ones, and a zero after them when `stop`.
    fn ones(&mut self, count: u32, stop: bool) {
        self.put((1u64 << count) - 1, count);
        if stop {
            self.put(0, 1);
        }
    }

    /// A run's length, plus one, in Elias gamma code: the bits under its top bit counted in unary, then those bits.
    fn put_run(&mut self, run: usize) {
        let value = run as u64 + 1;
        let len = value.ilog2();
        self.ones(len, true);
        self.put(value & ((1 << len) - 1), len);
    }

    /// A code in Rice code with parameter `rice`: its quotient in unary, then its remainder's `rice` bits; or, when the
    /// quotient reaches LIMIT, LIMIT ones and the code's ESCAPE_BITS bits.
    fn put_code(&mut self, code: u32, rice: u32) {
        let quotient = code >> rice;
        if quotient < LIMIT {
            self.ones(quotient, true);
            self.put((code & ((1 << rice) - 1)) as u64, rice);
        } else {
            self.ones(LIMIT, false);
            self.put(code as u64, ESCAPE_BITS);
        }
    }

    /// The bytes written, the last one filled up with zeros.
    fn finish(self) -> Vec<u8> {
        let mut out = self.out;
        if self.bit_count > 0 {
            out.push(self.bits as u8);
        }
        out
    }
}

/// Reads the bits a BitWriter wrote; past the data's end, zeros.
struct BitReader<'a> {
    data: &'a [u8],
    next_byte: usize,
    bits: u64,
    bit_count: u32,
}

impl BitReader<'_> {
    fn get(&mut self, len: u32) -> u64 {
        while self.bit_count < len {
            self.bits |= (*self.data.get(self.next_byte).unwrap_or(&0) as u64) << self.bit_count;
            self.next_byte += 1;
            self.bit_count += u8::BITS;
        }
        let value = self.bits & ((1u64 << len) - 1);
        self.bits >>= len;
        self.bit_count -= len;
        value
    }

    /// The ones before a zero, up to `limit` of them.
    fn ones(&mut self, limit: u32) -> u32 {
        let mut count = 0;
        while count < limit && self.get(1) == 1 {
            count += 1;
        }
        count
    }

    /// A run's length (`BitWriter::put_run`); None when its prefix is too long.
    fn get_run(&mut self) -> Option<usize> {
        let len = self.ones(MAX_RUN_PREFIX);
        if len >= MAX_RUN_PREFIX {
            return None;
        }
        Some((((1u64 << len) | self.get(len)) - 1) as usize)
    }

    /// A code in Rice code (`BitWriter::put_code`); None when it is past a byte's values.
    fn get_code(&mut self, rice: u32) -> Option<u32> {
        let quotient = self.ones(LIMIT);
        let code =
            if quotient < LIMIT { (quotient << rice) | self.get(rice) as u32 } else { self.get(ESCAPE_BITS) as u32 };
        (code < BYTE_VALUES as u32).then_some(code)
    }
}

/// Each context's running sum of codes and count, for its Rice parameter.
struct RiceStats {
    sums: [u32; CONTEXTS],
    counts: [u32; CONTEXTS],
}

impl RiceStats {
    fn new() -> RiceStats {
        RiceStats { sums: [INITIAL_SUM; CONTEXTS], counts: [INITIAL_COUNT; CONTEXTS] }
    }

    /// The smallest parameter (up to MAX_RICE) at which the context's count, shifted by it, reaches its sum.
    fn parameter(&self, context: usize) -> u32 {
        let mut rice = 0;
        while rice < MAX_RICE && (self.counts[context] << rice) < self.sums[context] {
            rice += 1;
        }
        rice
    }

    fn update(&mut self, context: usize, code: u32) {
        self.sums[context] += code;
        self.counts[context] += 1;
        if self.counts[context] >= HALVING_COUNT {
            self.sums[context] = (self.sums[context] + 1) >> 1;
            self.counts[context] >>= 1;
        }
    }
}

/// A pixel's neighbors, the row above taken as zeros on the first row and the edge pixels standing in past the sides.
#[derive(Clone, Copy)]
struct Neighbors {
    left: u8,
    up: u8,
    up_left: u8,
    up_right: u8,
}

impl Neighbors {
    fn of(image: &[u8], width: usize, x: usize, y: usize) -> Neighbors {
        let above = |x: usize| if y > 0 { image[(y - 1) * width + x] } else { 0 };
        let up = above(x);
        Neighbors {
            left: if x > 0 { image[y * width + x - 1] } else { up },
            up,
            up_left: if x > 0 { above(x - 1) } else { up },
            up_right: if x + 1 < width { above(x + 1) } else { up },
        }
    }

    /// The median edge detector's guess at the pixel.
    fn predicted(self) -> u8 {
        let Neighbors { left, up, up_left, .. } = self;
        if up_left >= left.max(up) {
            left.min(up)
        } else if up_left <= left.min(up) {
            left.max(up)
        } else {
            (left as i16 + up as i16 - up_left as i16) as u8
        }
    }

    /// How busy the neighborhood is: the bit length of its gradients' sum, up to the last context (0: all equal).
    fn context(self) -> usize {
        let gradients = self.up_right.abs_diff(self.up) as u32
            + self.up.abs_diff(self.up_left) as u32
            + self.up_left.abs_diff(self.left) as u32;
        ((u32::BITS - gradients.leading_zeros()) as usize).min(CONTEXTS - 1)
    }
}

/// A pixel's difference from its prediction as a code: wrapped to -128..127, then 0, -1, 1, -2, ... as 0, 1, 2, 3, ...
fn error_code(value: u8, predicted: u8) -> u32 {
    let error = (value as i32 - predicted as i32 + 384) % 256 - 128;
    (if error >= 0 { 2 * error } else { -2 * error - 1 }) as u32
}

/// The pixel a code gives on its prediction (`error_code` undone).
fn from_error_code(code: u32, predicted: u8) -> u8 {
    let error = if code.is_multiple_of(2) { (code / 2) as i32 } else { -((code as i32 + 1) / 2) };
    (predicted as i32 + error).rem_euclid(BYTE_VALUES as i32) as u8
}

/// A map (width x height bytes) packed: the number of values it has less one, the values, then the ranks' code.
fn pack(map: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut present = [false; BYTE_VALUES];
    map.iter().for_each(|&value| present[value as usize] = true);
    let values: Vec<u8> = (0..=u8::MAX).filter(|&value| present[value as usize]).collect();
    let mut rank = [0u8; BYTE_VALUES];
    for (i, &value) in values.iter().enumerate() {
        rank[value as usize] = i as u8;
    }
    let ranks: Vec<u8> = map.iter().map(|&value| rank[value as usize]).collect();
    let mut out = vec![values.len().saturating_sub(1) as u8];
    out.extend(&values);
    out.extend(encode(&ranks, width, height));
    out
}

/// A packed map back to its width x height bytes; None when it is not one.
fn unpack(data: &[u8], width: usize, height: usize) -> Option<Vec<u8>> {
    let value_count = *data.first()? as usize + 1;
    let values = data.get(1..1 + value_count)?;
    let ranks = decode(&data[1 + value_count..], width, height)?;
    ranks.into_iter().map(|rank| values.get(rank as usize).copied()).collect()
}

/// An image's code (width x height bytes).
fn encode(image: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut bits = BitWriter::default();
    let mut stats = RiceStats::new();
    for y in 0..height {
        let row = &image[y * width..(y + 1) * width];
        let mut x = 0;
        let mut after_run = false;
        while x < width {
            let neighbors = Neighbors::of(image, width, x, y);
            let context = neighbors.context();
            if context == 0 && !after_run {
                let run = row[x..].iter().take_while(|&&value| value == neighbors.left).count();
                bits.put_run(run);
                x += run;
                after_run = true;
                continue;
            }
            after_run = false;
            let code = error_code(row[x], neighbors.predicted());
            bits.put_code(code, stats.parameter(context));
            stats.update(context, code);
            x += 1;
        }
    }
    bits.finish()
}

/// A code back to its image (width x height bytes); None when the data runs out.
fn decode(data: &[u8], width: usize, height: usize) -> Option<Vec<u8>> {
    let mut bits = BitReader { data, next_byte: 0, bits: 0, bit_count: 0 };
    let mut image = vec![0u8; width * height];
    let mut stats = RiceStats::new();
    for y in 0..height {
        let mut x = 0;
        let mut after_run = false;
        while x < width {
            let neighbors = Neighbors::of(&image, width, x, y);
            let context = neighbors.context();
            if context == 0 && !after_run {
                let run = bits.get_run()?;
                if run > width - x {
                    return None;
                }
                image[y * width + x..y * width + x + run].fill(neighbors.left);
                x += run;
                after_run = true;
                continue;
            }
            after_run = false;
            let code = bits.get_code(stats.parameter(context))?;
            stats.update(context, code);
            image[y * width + x] = from_error_code(code, neighbors.predicted());
            x += 1;
        }
    }
    (bits.next_byte <= data.len()).then_some(image)
}

const BASE64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64, padded: each 3 bytes as 4 characters of 6 bits.
fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let byte = |i: usize| *chunk.get(i).unwrap_or(&0) as u32;
        let triple = byte(0) << 16 | byte(1) << 8 | byte(2);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(BASE64_ALPHABET[(triple >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Base64 back to bytes; None at a character outside the alphabet.
fn unbase64(text: &str) -> Option<Vec<u8>> {
    let characters = text.trim_end_matches('=').as_bytes();
    let mut out = Vec::with_capacity(characters.len() * 3 / 4);
    let (mut bits, mut bit_count) = (0u32, 0);
    for &character in characters {
        let sextet = BASE64_ALPHABET.iter().position(|&letter| letter == character)? as u32;
        bits = bits << 6 | sextet;
        bit_count += 6;
        if bit_count >= u8::BITS {
            bit_count -= u8::BITS;
            out.push((bits >> bit_count) as u8);
            bits &= (1 << bit_count) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(b: [f64; 4], rule: &str) -> Area {
        Area { bounds: b, feat: [0.5; 7], rule: rule.into() }
    }

    #[test]
    fn maps_pack_and_unpack_to_the_same_bytes() {
        let mut seed = 7u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 24) as u8
        };
        let (width, height) = (97, 31);
        let flat = vec![0u8; width * height];
        let ramp: Vec<u8> = (0..width * height).map(|i| ((i % width) * 3 + i / width) as u8).collect();
        let random: Vec<u8> = (0..width * height).map(|_| noise()).collect();
        let mixed: Vec<u8> = (0..width * height).map(|i| if (i / 7) % 5 == 0 { noise() } else { 200 }).collect();
        for image in [flat, ramp, random, mixed] {
            let packed = pack(&image, width, height);
            assert_eq!(unpack(&packed, width, height).as_deref(), Some(&image[..]));
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
        for (value, text) in
            [(0.1328, "0.1328"), (0.0, "0.0"), (1.0, "1.0"), (0.0001, "0.0001"), (1e-5, "1e-05"), (12.5, "12.5")]
        {
            assert_eq!(py_float(value), text);
        }
        assert_eq!(py_float(1e16), "1e+16");
        assert_eq!(py_float(123456789012345.0), "123456789012345.0");
        assert_eq!(py_str("uploads/1902 \u{ff5c} #2.mp4"), "\"uploads/1902 \\uff5c #2.mp4\"");
        assert_eq!(py_str("a\"b\\\n\u{1f600}"), "\"a\\\"b\\\\\\n\\ud83d\\ude00\"");
        let example = Example { rec: "r".into(), feat: [0.5, 0.0833, 0.1, 0.0, 1.0, 0.25, 0.0], kind: "clock".into() };
        assert_eq!(
            example_line(&example),
            r#"{"rec": "r", "feat": [0.5, 0.0833, 0.1, 0.0, 1.0, 0.25, 0.0], "kind": "clock"}"#
        );
    }

    #[test]
    fn text_rows_are_runs_of_three_or_more() {
        let on = [true, true, true, false, true, true, false, true, true, true, true];
        assert_eq!(review_rows(&on), vec![0..3, 7..11]);
    }

    #[test]
    fn numpy_sums_follow_its_order() {
        // the order itself is checked against NumPy by examples/areas.rs; here, that every pixel is summed once
        let value = |i: usize| (i % 7) as f64;
        for rect in [
            Rect { x0: 0, y0: 3, x1: W, y1: 40 },
            Rect { x0: 5, y0: 0, x1: 13, y1: H },
            Rect { x0: 9, y0: 2, x1: 10, y1: 3 },
        ] {
            let direct: f64 = (rect.y0..rect.y1).flat_map(|y| (rect.x0..rect.x1).map(move |x| value(y * W + x))).sum();
            assert_eq!(numpy_sum(rect, value), direct);
        }
        // 8 values or more: eight running sums, then the rest
        assert_eq!(pairwise(&[1e8f32, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 4.0]), ((1e8f32 + 1.0) + 2.0) + 4.0 + 4.0);
    }

    #[test]
    fn pillow_bilinear_matches_its_bytes() {
        // Pillow 12.3's Image.resize((16, 16), BILINEAR) of the same patterns
        let pattern = |height: usize, width: usize| -> Vec<u8> {
            (0..height).flat_map(|y| (0..width).map(move |x| ((x * 7 + y * 13 + (x * y) % 11) % 256) as u8)).collect()
        };
        let sum = |image: &[u8]| image.iter().map(|&value| value as u32).sum::<u32>();
        let out = thumbnail(&pattern(30, 47), 47, 30);
        assert_eq!(sum(&out), 31983);
        assert_eq!(&out[..16], &[18, 38, 60, 80, 98, 121, 142, 162, 181, 203, 219, 200, 88, 29, 51, 66]);
        let col: Vec<u8> = (0..16).map(|y| out[y * 16 + 5]).collect();
        assert_eq!(col, [121, 145, 169, 193, 219, 221, 91, 37, 60, 84, 108, 131, 156, 182, 206, 228]);
        let up = thumbnail(&pattern(8, 8), 8, 8);
        assert_eq!(sum(&up), 18997);
        assert_eq!(&up[..16], &[0, 2, 5, 9, 12, 16, 19, 23, 26, 30, 33, 37, 40, 44, 47, 49]);
        let tall = thumbnail(&pattern(100, 37), 37, 100);
        assert_eq!(sum(&tall), 32688);
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
        let feat = |center_x: f64, center_y: f64, width: f64, height: f64, fixed: f64, rows: f64| {
            [center_x, center_y, width, height, fixed, 0.1, round(rows / 12.0, 4)]
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
            feat: [x, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1],
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
        a.feat = [0.5, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1];
        let named = predict(std::slice::from_ref(&a), &examples, NEAREST_EXAMPLES);
        assert_eq!((named[0].kind.as_str(), named[0].by.as_str()), ("clock", "learned"));
        // too few examples: the rules
        assert_eq!(predict(std::slice::from_ref(&a), &examples[..4], NEAREST_EXAMPLES)[0].by, "rule");
        let removed =
            vec![ex("a", 0.5, NONE), ex("b", 0.5, NONE), ex("c", 0.5, NONE), ex("d", 0.5, "x"), ex("e", 0.5, "y")];
        assert!(predict(&[a], &removed, NEAREST_EXAMPLES).is_empty());
        assert_eq!(check(&examples).count, 5);
    }

    #[test]
    fn learning_replaces_the_recordings_examples() {
        let saved = vec![
            SavedBox { bounds: [0.0, 0.0, 0.2, 0.2], kind: Some("clock".into()) },
            SavedBox { bounds: [0.5, 0.5, 0.6, 0.6], kind: None },
        ];
        let found_areas = vec![found([0.01, 0.01, 0.19, 0.19], "Clock"), found([0.8, 0.8, 0.9, 0.9], "Other")];
        let kinds = |examples: &[Example]| examples.iter().map(|example| example.kind.clone()).collect::<Vec<_>>();
        let ex = learn("r", &found_areas, &saved, None);
        assert_eq!(kinds(&ex), ["clock", NONE]);
        let maps = Maps::new(vec![255; W * H], vec![40; W * H]).unwrap();
        let ex = learn("r", &found_areas, &saved, Some(&maps));
        assert_eq!(kinds(&ex), ["clock", "other", NONE]);
        assert_eq!(ex[0].feat, [0.1, 0.1, 0.2, 0.2, 1.0, 1.0, 0.0833]);
        let old = concat!(
            "{\"rec\": \"q\", \"feat\": [1.0], \"kind\": \"x\"}\n",
            "{\"rec\": \"kovobs:r\", \"feat\": [1.0], \"kind\": \"x\"}\n"
        );
        let lines = merge(old, "r", &ex);
        assert_eq!(lines.lines().count(), 4);
        assert!(lines.starts_with("{\"rec\": \"q\""));
    }

    #[test]
    fn find_copies_a_labelled_layout_or_names_the_found_areas() {
        let found_areas = vec![found([0.0, 0.0, 0.1, 0.1], "Clock")];
        let saved = vec![SavedBox { bounds: [0.0, 0.0, 0.11, 0.1], kind: None }];
        let labelled = Labelled { rec: "other".into(), found: found_areas.clone(), saved };
        let proposal = find(&found_areas, &[], std::slice::from_ref(&labelled));
        assert_eq!(proposal.copied.as_deref(), Some("other"));
        assert_eq!(proposal.boxes[0].kind.as_deref(), Some("Other"));
        let proposal = find(&found_areas, &[], &[]);
        assert_eq!((proposal.copied, proposal.by["rule"]), (None, 1));
        assert_eq!(same_layout(&found_areas, &found_areas), 1.0);
        let kinds = vec![Kind { id: "clock".into(), name: "Clock".into() }];
        assert_eq!(with_ids(&proposal.boxes, &kinds)[0].kind.as_deref(), Some("clock"));
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
        assert!(picked.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn json_calls_read_and_write_pythons_shapes() {
        let found =
            r#"[{"box": [0.0, 0.0, 0.1, 0.1], "feat": [0.05, 0.05, 0.1, 0.1, 0.4, 0.2, 0.0833], "rule": "Clock"}]"#;
        let labelled = format!(r#"[["x", {found}, [[0, 0, 0.1, 0.1, "Clock"]]]]"#);
        let kinds = r#"[{"id": "clock", "name": "Clock"}]"#;
        let out =
            find_json(&format!(r#"{{"found": {found}, "examples": "", "labelled": {labelled}, "kinds": {kinds}}}"#))
                .unwrap();
        let value: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value["copied"], "x");
        assert_eq!(value["boxes"][0][4], "clock");
        let out = learn_json(&format!(r#"{{"rec": "y", "found": {found}, "saved": [], "examples": ""}}"#)).unwrap();
        let value: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value["added"], 1);
        assert!(value["examples"].as_str().unwrap().contains("\"kind\": \"none\""));
        assert_eq!(same_layout_json(&format!(r#"{{"found": {found}, "other": {found}}}"#)).unwrap(), "1.0");
        assert!(sample_json(r#"{"keys": 30, "times": [], "duration": 1}"#).unwrap() == "null");
    }
}
