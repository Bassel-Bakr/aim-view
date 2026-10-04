//! How the camera turned, from the video alone (review.py: `camera_motion`), and KovaaK's countdown bar
//! (`countdown_showing`). Each frame is resampled onto an angular grid around the crosshair (azimuth -36 to 36 degrees,
//! elevation 18 to -18, 0.15 degrees apart), cut into 18 tiles of 12 degrees, and each tile is matched with the frame
//! before by phase correlation, in single precision as SciPy's FFT works for Python. A tile is left out while a
//! tracked target is in it, when a tenth of it is excluded area or the fixed map, or when its peak is under 0.08. The
//! frame's reading is the mean of the tiles that agree with their median within 0.1 degrees, if 3 or more do.

use std::sync::Arc;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use serde::{Deserialize, Serialize};

use crate::geometry::{CX, CY, H, K, W, degrees, radians};
use crate::track::TrackFrame;
use crate::tracking::CameraReading;

const STEP: f64 = 0.15;
const T: usize = 80;
const TILES: usize = 18;
const COLS: usize = 6 * T;
const BINS: usize = T / 2 + 1;
/// The grid's cells: 3 tiles down, 6 across.
const CELLS: usize = COLS * 3 * T;
const THR: f32 = 0.08;

/// Each tile's shift since the frame before (degrees, the room's move on screen), or None where its peak is too low.
pub type TileShifts = [Option<(f32, f32)>; TILES];

/// The grid's bilinear gather (top-left pixel and weights per grid cell), each tile's center, and the Hann window.
struct Grid {
    i00: Box<[usize; CELLS]>,
    fx: Box<[f32; CELLS]>,
    fy: Box<[f32; CELLS]>,
    taz: [f64; TILES],
    tel: [f64; TILES],
    hann: Box<[f32; T * T]>,
}

impl Grid {
    fn new() -> Grid {
        let az: [f64; COLS] = std::array::from_fn(|c| (c as f64 - (3 * T) as f64 + 0.5) * STEP);
        let el: [f64; 3 * T] = std::array::from_fn(|r| (1.5 * T as f64 - r as f64 - 0.5) * STEP);
        let mut g = Grid {
            i00: vec![0; CELLS].try_into().unwrap(),
            fx: vec![0.0; CELLS].try_into().unwrap(),
            fy: vec![0.0; CELLS].try_into().unwrap(),
            taz: [0.0; TILES],
            tel: [0.0; TILES],
            hann: vec![0.0; T * T].try_into().unwrap(),
        };
        for (r, &e) in el.iter().enumerate() {
            for (c, &a) in az.iter().enumerate() {
                let p = r * COLS + c;
                let px = CX + K * radians(a).tan();
                let py = CY - radians(e).tan() * K.hypot(px - CX);
                let (x0, y0) = (px.floor(), py.floor());
                g.fx[p] = (px - x0) as f32;
                g.fy[p] = (py - y0) as f32;
                let (x0, y0) = ((x0 as i64).clamp(0, W as i64 - 2), (y0 as i64).clamp(0, H as i64 - 2));
                g.i00[p] = y0 as usize * W + x0 as usize;
            }
        }
        for k in 0..TILES {
            let (c, r) = ((k % 6) * T, (k / 6) * T);
            g.taz[k] = az[c..c + T].iter().sum::<f64>() / T as f64;
            g.tel[k] = el[r..r + T].iter().sum::<f64>() / T as f64;
        }
        // np.hanning
        let w: [f64; T] = std::array::from_fn(|i| {
            0.5 + 0.5 * (std::f64::consts::PI * (2 * i as i64 + 1 - T as i64) as f64 / (T - 1) as f64).cos()
        });
        for (i, &a) in w.iter().enumerate() {
            for (j, &b) in w.iter().enumerate() {
                g.hann[i * T + j] = (a * b) as f32;
            }
        }
        g
    }

    /// The image resampled onto the grid, as 18 tiles of T x T (row by row), tile k at row k / 6, column k % 6.
    fn tiles(&self, img: &[u8]) -> Vec<f32> {
        let mut out = vec![0.0f32; TILES * T * T];
        let f = |j: usize| img[j] as f32;
        for r in 0..3 * T {
            let row = (r / T) * 6 * T * T + (r % T) * T;
            for c in 0..COLS {
                let p = r * COLS + c;
                let (i, fx, fy) = (self.i00[p], self.fx[p], self.fy[p]);
                let v = (f(i) * (1.0 - fx) + f(i + 1) * fx) * (1.0 - fy) + (f(i + W) * (1.0 - fx) + f(i + W + 1) * fx) * fy;
                out[row + (c / T) * T * T + c % T] = v;
            }
        }
        out
    }
}

/// NumPy's float32 sum (pairwise: 8 running sums for up to 128 values, then halves).
fn sum_f32(v: &[f32]) -> f32 {
    if v.len() < 8 {
        return v.iter().fold(0.0f32, |a, &b| a + b);
    }
    if v.len() <= 128 {
        let mut r = [0.0f32; 8];
        r.copy_from_slice(&v[..8]);
        let whole = v.len() - v.len() % 8;
        for chunk in v[8..whole].chunks_exact(8) {
            for (a, &b) in r.iter_mut().zip(chunk) {
                *a += b;
            }
        }
        let mut s = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
        for &b in &v[whole..] {
            s += b;
        }
        return s;
    }
    let half = v.len() / 2 / 8 * 8;
    sum_f32(&v[..half]) + sum_f32(&v[half..])
}

fn median_f32(v: &mut [f32]) -> f32 {
    v.sort_by(f32::total_cmp);
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

/// The parabola's vertex offset from three samples around a peak m (review.py: `sub`).
fn vertex(a: f32, b: f32, m: f32) -> f32 {
    let d = a - 2.0 * m + b;
    if d >= 0.0 { 0.0 } else { 0.5 * (a - b) / d }
}

/// A frame's camera reading from its tiles' shifts: the tiles allowed (`ok`) that agree with their median.
fn agreed(shifts: &TileShifts, ok: &[bool; TILES]) -> CameraReading {
    let sh: Vec<(f32, f32)> = (0..TILES).filter(|&k| ok[k]).filter_map(|k| shifts[k]).collect();
    if sh.len() < 3 {
        return None;
    }
    let m = (median_f32(&mut sh.iter().map(|s| s.0).collect::<Vec<_>>()), median_f32(&mut sh.iter().map(|s| s.1).collect::<Vec<_>>()));
    let agree: Vec<&(f32, f32)> = sh.iter().filter(|s| (s.0 - m.0).hypot(s.1 - m.1) < 0.1).collect();
    if agree.len() < 3 {
        return None;
    }
    let n = agree.len() as f32;
    let mx = sum_f32(&agree.iter().map(|s| s.0).collect::<Vec<_>>()) / n;
    let my = sum_f32(&agree.iter().map(|s| s.1).collect::<Vec<_>>()) / n;
    Some((mx as f64, my as f64, agree.len()))
}

/// The pixels a tile must not be read from: the excluded areas (`keep` false) and the fixed map grown by 3 pixels
/// (SciPy's `binary_dilation`, 3 iterations of the cross).
pub fn excluded(keep: &[bool], fixed: &[u8]) -> Vec<bool> {
    let mut grown: Vec<bool> = fixed.iter().map(|&v| v != 0).collect();
    for _ in 0..3 {
        let g = grown.clone();
        for y in 0..H {
            for x in 0..W {
                let i = y * W + x;
                grown[i] = g[i]
                    || (x > 0 && g[i - 1])
                    || (x + 1 < W && g[i + 1])
                    || (y > 0 && g[i - W])
                    || (y + 1 < H && g[i + W]);
            }
        }
    }
    grown.iter().zip(keep).map(|(&g, &k)| g || !k).collect()
}

/// The camera watch over a recording: fed each frame's luma (1280 x 720), it keeps each frame's tile shifts and its
/// countdown bar's showing; the readings come once the tracks are known (a tile with a target in it is left out).
pub struct CameraWatch {
    grid: Grid,
    static_ok: [bool; TILES],
    row: Arc<dyn Fft<f32>>,
    row_inv: Arc<dyn Fft<f32>>,
    /// The FFTs' working space, kept from frame to frame.
    scratch: Box<[Complex32]>,
    prev: Option<Vec<Complex32>>,
    pub shifts: Vec<TileShifts>,
    pub countdown: Vec<bool>,
}

impl CameraWatch {
    /// `bad`: the pixels no tile may be read from (`excluded`).
    pub fn new(bad: &[bool]) -> CameraWatch {
        let grid = Grid::new();
        let share = grid.tiles(&bad.iter().map(|&b| b as u8).collect::<Vec<u8>>());
        let mut static_ok = [false; TILES];
        for (k, ok) in static_ok.iter_mut().enumerate() {
            let t = &share[k * T * T..(k + 1) * T * T];
            *ok = (t.iter().map(|&v| v as f64).sum::<f64>() / (T * T) as f64) < 0.1;
        }
        let mut planner = FftPlanner::new();
        let (row, row_inv) = (planner.plan_fft_forward(T), planner.plan_fft_inverse(T));
        let scratch = vec![Complex32::default(); row.get_inplace_scratch_len().max(row_inv.get_inplace_scratch_len())]
            .into_boxed_slice();
        CameraWatch {
            grid,
            static_ok,
            row,
            row_inv,
            scratch,
            prev: None,
            shifts: Vec::new(),
            countdown: Vec::new(),
        }
    }

    /// Each tile's spectrum (T rows of BINS, rfft2 of the tile less its mean, times the Hann window). The FFTs run a
    /// tile at a time: its T rows in one call, then its BINS columns in one call.
    fn spectra(&mut self, gray: &[u8]) -> Vec<Complex32> {
        let tiles = self.grid.tiles(gray);
        let mut out = vec![Complex32::default(); TILES * T * BINS];
        let mut rows = vec![Complex32::default(); T * T];
        let mut cols = vec![Complex32::default(); BINS * T];
        for k in 0..TILES {
            let t = &tiles[k * T * T..(k + 1) * T * T];
            let mean = (t.iter().map(|&v| v as f64).sum::<f64>() / (T * T) as f64) as f32;
            for (i, v) in rows.iter_mut().enumerate() {
                *v = Complex32::new((t[i] - mean) * self.grid.hann[i], 0.0);
            }
            self.row.process_with_scratch(&mut rows, &mut self.scratch);
            for c in 0..BINS {
                for y in 0..T {
                    cols[c * T + y] = rows[y * T + c];
                }
            }
            self.row.process_with_scratch(&mut cols, &mut self.scratch);
            let f = &mut out[k * T * BINS..(k + 1) * T * BINS];
            for c in 0..BINS {
                for y in 0..T {
                    f[y * BINS + c] = cols[c * T + y];
                }
            }
        }
        out
    }

    /// The watch for a recording, its tiles kept clear of the KovOBS overlay and of the fixed map (1280 x 720, 1 fixed).
    pub fn for_recording(fixed: &[u8]) -> CameraWatch {
        let overlay = crate::track::Mask::without(&crate::geometry::overlay_shares());
        CameraWatch::new(&excluded(overlay.kept(), fixed))
    }

    /// The readings, the tracks known: each frame's camera turn, and whether the countdown bar shows.
    pub fn finish(self, frames: &[TrackFrame]) -> VideoReadings {
        VideoReadings { camera: self.readings(frames), countdown: self.countdown }
    }

    /// Frames not reviewed before the first one (a review from part way in): no reading, no countdown. Before any
    /// frame is added.
    pub fn skip(&mut self, frames: usize) {
        self.shifts.extend(std::iter::repeat_n([None; TILES], frames));
        self.countdown.extend(std::iter::repeat_n(false, frames));
    }

    /// What this run of the recording read, to join with the other runs' (a recording split into runs, reviewed in
    /// workers at once, has a watch for each).
    pub fn part(self) -> CameraPart {
        CameraPart { shifts: self.shifts, countdown: self.countdown }
    }

    /// The next run's part. Each run but the last also reads the next run's first frame, for the camera's turn into
    /// it; the next run read that frame without the one before it, so its first entry is left out.
    pub fn join(&mut self, next: CameraPart) {
        let skip = usize::from(!self.countdown.is_empty());
        self.shifts.extend(next.shifts.into_iter().skip(skip));
        self.countdown.extend(next.countdown.into_iter().skip(skip));
    }

    /// One frame: its luma (1280 x 720) and its RGB24 (for the countdown bar).
    pub fn add(&mut self, gray: &[u8], rgb: &[u8]) {
        self.countdown.push(countdown_showing(rgb));
        let f = self.spectra(gray);
        let Some(prev) = self.prev.replace(f) else {
            self.shifts.push([None; TILES]);
            return;
        };
        let f = self.prev.as_ref().unwrap();
        let mut shifts = [None; TILES];
        let mut cols = vec![Complex32::default(); BINS * T];
        let mut rows = vec![Complex32::default(); T * T];
        let mut c = vec![0.0f32; T * T];
        for (k, shift) in shifts.iter_mut().enumerate() {
            let at = k * T * BINS;
            // the normalized cross-power spectrum, column by column
            for col in 0..BINS {
                for y in 0..T {
                    let v = f[at + y * BINS + col] * prev[at + y * BINS + col].conj();
                    cols[col * T + y] = v / v.norm().max(1e-6);
                }
            }
            // irfft2: the inverse along y for each column (one call), then the real inverse along x for each row
            self.row_inv.process_with_scratch(&mut cols, &mut self.scratch);
            for y in 0..T {
                for x in 0..BINS {
                    rows[y * T + x] = cols[x * T + y];
                }
                for x in BINS..T {
                    rows[y * T + x] = cols[(T - x) * T + y].conj();
                }
            }
            self.row_inv.process_with_scratch(&mut rows, &mut self.scratch);
            for (ci, v) in c.iter_mut().zip(&rows) {
                *ci = v.re / (T * T) as f32;
            }
            let peak = (0..T * T).fold(0, |b, i| if c[i] > c[b] { i } else { b });
            let (py, px) = (peak / T, peak % T);
            let pk = c[peak];
            if pk < THR {
                continue;
            }
            let dx = px as f32 + vertex(c[py * T + (px + T - 1) % T], c[py * T + (px + 1) % T], pk);
            let dy = py as f32 + vertex(c[((py + T - 1) % T) * T + px], c[((py + 1) % T) * T + px], pk);
            let wrap = |v: f32| if v > (T / 2) as f32 { v - T as f32 } else { v };
            *shift = Some((wrap(dx) * STEP as f32, -wrap(dy) * STEP as f32));
        }
        self.shifts.push(shifts);
    }

    /// A frame's reading from its tiles' shifts, leaving out the tiles a target is in, in it or the frame before.
    pub fn reading(&self, shifts: &TileShifts, near: &[&TrackFrame]) -> CameraReading {
        let mut ok = self.static_ok;
        for f in near {
            for (&(_, x, y), &a) in f.t.iter().zip(&f.a) {
                let r = degrees(((a as f64 / std::f64::consts::PI).sqrt() / K).atan()) + 1.5;
                for ((ok, &az), &el) in ok.iter_mut().zip(&self.grid.taz).zip(&self.grid.tel) {
                    if (az - x).abs() < 6.0 + r && (el - y).abs() < 6.0 + r {
                        *ok = false;
                    }
                }
            }
        }
        agreed(shifts, &ok)
    }

    /// Every frame's reading, the tracks known.
    pub fn readings(&self, frames: &[TrackFrame]) -> Vec<CameraReading> {
        (0..self.shifts.len())
            .map(|i| {
                if i == 0 {
                    return None;
                }
                let near: Vec<&TrackFrame> = if i < frames.len() { vec![&frames[i - 1], &frames[i]] } else { vec![] };
                self.reading(&self.shifts[i], &near)
            })
            .collect()
    }
}

/// What a tracking run reads from the video besides the tracks (CameraWatch::finish).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct VideoReadings {
    pub camera: Vec<CameraReading>,
    pub countdown: Vec<bool>,
}

/// A run's part of the camera watch (CameraWatch::part): each frame's tile shifts and whether the countdown bar shows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraPart {
    pub shifts: Vec<TileShifts>,
    pub countdown: Vec<bool>,
}

/// The rows of a frame the countdown test reads (y from, to): the camera watch needs only these rows of a frame's RGB.
pub const COUNTDOWN_ROWS: (usize, usize) = (214, 247);

/// Whether a frame (RGB24, 1280 x 720) shows KovaaK's countdown bar ("Challenge begins in"): its box (x 520 to 760,
/// y 214 to 246) holds the bar's dark gray track and one fill color, the fill on the left, the track at the right
/// end, and white digits. The fill takes the HUD's color, so the color is read from the bar's left end, which the
/// fill covers to the last frame.
pub fn countdown_showing(rgb: &[u8]) -> bool {
    const TRACK: [i32; 3] = [64, 60, 68];
    let (x0, x1, (y0, y1)) = (520, 761, COUNTDOWN_ROWS);
    let px = |x: usize, y: usize| {
        let i = (y * W + x) * 3;
        [rgb[i] as i32, rgb[i + 1] as i32, rgb[i + 2] as i32]
    };
    let near = |c: [i32; 3], to: [f64; 3], by: f64| (0..3).all(|k| (c[k] as f64 - to[k]).abs() <= by);
    let track_f = TRACK.map(|v| v as f64);
    let mut fill_c = [0.0; 3];
    for (k, f) in fill_c.iter_mut().enumerate() {
        let mut v: [i32; 21 * 4] = std::array::from_fn(|j| px(x0 + 2 + j % 4, y0 + 6 + j / 4)[k]);
        v.sort();
        *f = (v[v.len() / 2 - 1] + v[v.len() / 2]) as f64 / 2.0;
    }
    if (0..3).all(|k| (fill_c[k] - track_f[k]).abs() <= 40.0) {
        return false;
    }
    let (mut fill, mut track, mut known, mut fill_x, mut track_x) = (0usize, 0usize, 0usize, 0usize, 0usize);
    for y in y0..y1 {
        for x in x0..x1 {
            let c = px(x, y);
            let (is_fill, is_track) = (near(c, fill_c, 24.0), near(c, track_f, 12.0));
            let white = c.iter().all(|&v| v >= 170);
            fill += is_fill as usize;
            track += is_track as usize;
            known += (is_fill || is_track || white) as usize;
            fill_x += if is_fill { x - x0 } else { 0 };
            track_x += if is_track { x - x0 } else { 0 };
        }
    }
    let area = (x1 - x0) * (y1 - y0);
    if fill < 40 || (track as f64) < 0.05 * area as f64 || (known as f64) < 0.9 * area as f64 {
        return false;
    }
    let right = (y0 + 6..y0 + 27).flat_map(|y| (x1 - 12..x1 - 4).map(move |x| (x, y))).filter(|&(x, y)| near(px(x, y), track_f, 12.0)).count();
    (fill_x as f64 / fill as f64) < (track_x as f64 / track as f64) && right as f64 > 0.8 * (21 * 8) as f64
}
