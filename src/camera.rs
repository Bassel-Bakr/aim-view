//! How the camera turned, from the video alone (python/retired/review.py: `camera_motion`), and whether KovaaK's
//! countdown bar shows (`countdown_showing`). The review session (src/session.rs) feeds the watch each frame's luma
//! and the countdown bar's rows of its RGB: in the camera worker in the browser, on a thread of its own natively. A
//! recording split into run parts has a watch for each, joined in order (`CameraPart`). Once the tracks are known,
//! `finish` gives the readings (`VideoReadings`): the camera's turn, which src/tracking.rs measures a tracking run
//! with, and the countdown, which src/review.rs places the run's start with when no kill does.
//!
//! Each frame is resampled onto an angular grid around the crosshair (azimuth -36 to 36 degrees, elevation 18 to -18,
//! 0.15 degrees apart), cut into 18 tiles of 12 degrees, and each tile is matched with the frame before by phase
//! correlation, in single precision as SciPy's FFT works for Python: every operation keeps Python's order and type. A
//! tile is left out while a tracked target is in it, when a tenth of it is excluded area or the fixed map, or when its
//! peak is under 0.08. The frame's reading is the mean of the tiles that agree with their median within 0.1 degrees,
//! if 3 or more do.

use std::f64::consts::PI;
use std::sync::Arc;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use serde::{Deserialize, Serialize};

use crate::capped::Capped;
use crate::geometry::{CX, CY, H, K, W, radians};
use crate::track::TrackFrame;
use crate::tracking::{CameraReading, disc_radius_deg};

/// The grid's spacing (degrees), across and down.
const GRID_STEP_DEG: f64 = 0.15;
/// A tile's side, in grid cells (12 degrees).
const TILE_SIDE: usize = 80;
/// A tile's cells (6,400).
const TILE_CELLS: usize = TILE_SIDE * TILE_SIDE;
/// The grid's tiles across: tile k is at row k / 6, column k % 6.
const TILES_ACROSS: usize = 6;
/// The grid's tiles down.
const TILES_DOWN: usize = 3;
/// The grid's tiles (18), each matched on its own.
const TILES: usize = TILES_ACROSS * TILES_DOWN;
/// The grid's columns (480 cells: 72 degrees of azimuth).
const GRID_COLUMNS: usize = TILES_ACROSS * TILE_SIDE;
/// The grid's rows (240 cells: 36 degrees of elevation).
const GRID_ROWS: usize = TILES_DOWN * TILE_SIDE;
/// The grid's cells, all tiles together.
const GRID_CELLS: usize = GRID_COLUMNS * GRID_ROWS;
/// The columns of a tile's spectrum: a real FFT along x keeps the frequencies 0 to half the side.
const SPECTRUM_COLUMNS: usize = TILE_SIDE / 2 + 1;
/// The values in a tile's half spectrum: TILE_SIDE rows of SPECTRUM_COLUMNS.
const SPECTRUM_CELLS: usize = TILE_SIDE * SPECTRUM_COLUMNS;
/// A tile whose phase correlation peaks under this matched nothing.
const MIN_PEAK: f32 = 0.08;
/// A tile is left out when this share of it or more is excluded area or the fixed map.
const MAX_EXCLUDED_SHARE: f64 = 0.1;
/// The tiles that must agree for a reading.
const MIN_AGREEING_TILES: usize = 3;
/// A tile agrees when its shift is under this from the tiles' median (degrees).
const AGREE_DEG: f32 = 0.1;
/// The least magnitude a cross-power value is divided by, so a frequency with no energy does not divide by 0.
const MIN_MAGNITUDE: f32 = 1e-6;
/// The fixed map is grown by this many pixels (the cross's iterations): the edges of a fixed thing count too.
const FIXED_GROWTH_PX: usize = 3;
/// Half a tile's side (degrees): a target is in a tile when it lies under this plus its reach from the tile's center,
/// along each axis.
const TILE_HALF_DEG: f64 = 6.0;
/// A target's reach past its disc's radius (degrees).
const TARGET_MARGIN_DEG: f64 = 1.5;
/// NumPy's float32 sum adds in this many running sums, over blocks of up to `PAIRWISE_BLOCK` values.
const PAIRWISE_LANES: usize = 8;
/// The most values NumPy's float32 sum adds in running sums; a longer run is split in halves and each summed alone.
const PAIRWISE_BLOCK: usize = 128;

/// Each tile's shift since the frame before (degrees, the room's move on screen), or None where its peak is too low.
pub type TileShifts = [Option<(f32, f32)>; TILES];

/// The grid's bilinear gather (per grid cell, the top-left pixel of the four it is read from, and the weights of the
/// pixels right and below), each tile's center (degrees), and the Hann window.
struct Grid {
    /// Per grid cell (row by row), the index in the 1280 x 720 frame of the top-left pixel of the four it is read from.
    top_left: Box<[usize; GRID_CELLS]>,
    /// Per grid cell, the weight of the right-hand pixels (0 to 1); the left-hand ones get 1 less this.
    weight_x: Box<[f32; GRID_CELLS]>,
    /// Per grid cell, the weight of the lower pixels (0 to 1); the upper ones get 1 less this.
    weight_y: Box<[f32; GRID_CELLS]>,
    /// Each tile's center azimuth (degrees, right positive), to tell which tiles a target covers.
    tile_azimuth_deg: [f64; TILES],
    /// Each tile's center elevation (degrees, up positive).
    tile_elevation_deg: [f64; TILES],
    /// The Hann window a tile is multiplied by before its FFT: it fades the tile to 0 at its edges, since the FFT
    /// treats the tile as repeating and the jump between opposite edges would swamp the match.
    hann: Box<[f32; TILE_CELLS]>,
}

impl Grid {
    /// The grid for a 1280 x 720 frame: each cell's place on the frame, each tile's center and the window. A watch
    /// makes it once.
    fn new() -> Grid {
        let azimuth_deg: [f64; GRID_COLUMNS] =
            std::array::from_fn(|column| (column as f64 - (GRID_COLUMNS / 2) as f64 + 0.5) * GRID_STEP_DEG);
        let elevation_deg: [f64; GRID_ROWS] =
            std::array::from_fn(|row| ((GRID_ROWS / 2) as f64 - row as f64 - 0.5) * GRID_STEP_DEG);
        let mut grid = Grid {
            top_left: vec![0; GRID_CELLS].try_into().unwrap(),
            weight_x: vec![0.0; GRID_CELLS].try_into().unwrap(),
            weight_y: vec![0.0; GRID_CELLS].try_into().unwrap(),
            tile_azimuth_deg: tile_centers(&azimuth_deg, |tile| tile % TILES_ACROSS),
            tile_elevation_deg: tile_centers(&elevation_deg, |tile| tile / TILES_ACROSS),
            hann: hann_window(),
        };
        grid.set_gather(&azimuth_deg, &elevation_deg);
        grid
    }

    /// Per grid cell, where its azimuth and elevation fall on the frame (pixels): the top-left pixel of the four it is
    /// read from, and the weights of the others.
    fn set_gather(&mut self, azimuth_deg: &[f64; GRID_COLUMNS], elevation_deg: &[f64; GRID_ROWS]) {
        for (row, &elevation) in elevation_deg.iter().enumerate() {
            for (column, &azimuth) in azimuth_deg.iter().enumerate() {
                let cell = row * GRID_COLUMNS + column;
                let x = CX + K * radians(azimuth).tan();
                let y = CY - radians(elevation).tan() * K.hypot(x - CX);
                let (left, top) = (x.floor(), y.floor());
                self.weight_x[cell] = (x - left) as f32;
                self.weight_y[cell] = (y - top) as f32;
                let (left, top) = ((left as i64).clamp(0, W as i64 - 2), (top as i64).clamp(0, H as i64 - 2));
                self.top_left[cell] = top as usize * W + left as usize;
            }
        }
    }

    /// The image resampled onto the grid, as 18 tiles of TILE_SIDE x TILE_SIDE (row by row), tile k at row k / 6,
    /// column k % 6.
    fn tiles(&self, image: &[u8]) -> Vec<f32> {
        let mut out = vec![0.0f32; TILES * TILE_CELLS];
        self.tiles_into(image, &mut out);
        out
    }

    /// `tiles` into `out` (TILES x TILE_CELLS), every value written.
    fn tiles_into(&self, image: &[u8], out: &mut [f32]) {
        for row in 0..GRID_ROWS {
            let row_start = (row / TILE_SIDE) * TILES_ACROSS * TILE_CELLS + (row % TILE_SIDE) * TILE_SIDE;
            for column in 0..GRID_COLUMNS {
                let value = self.bilinear(image, row * GRID_COLUMNS + column);
                out[row_start + (column / TILE_SIDE) * TILE_CELLS + column % TILE_SIDE] = value;
            }
        }
    }

    /// A grid cell's value: the image's four pixels around its place, weighted.
    fn bilinear(&self, image: &[u8], cell: usize) -> f32 {
        let pixel = |index: usize| f32::from(image[index]);
        let (i, weight_x, weight_y) = (self.top_left[cell], self.weight_x[cell], self.weight_y[cell]);
        (pixel(i) * (1.0 - weight_x) + pixel(i + 1) * weight_x) * (1.0 - weight_y)
            + (pixel(i + W) * (1.0 - weight_x) + pixel(i + W + 1) * weight_x) * weight_y
    }
}

/// Each tile's center along one axis (degrees): the mean of its cells' places, `cells_deg`, from the tile's position
/// along that axis.
fn tile_centers(cells_deg: &[f64], position: fn(usize) -> usize) -> [f64; TILES] {
    std::array::from_fn(|tile| {
        let start = position(tile) * TILE_SIDE;
        cells_deg[start..start + TILE_SIDE].iter().sum::<f64>() / TILE_SIDE as f64
    })
}

/// The Hann window over a tile: np.hanning's along each side, multiplied.
fn hann_window() -> Box<[f32; TILE_CELLS]> {
    let side: [f64; TILE_SIDE] = std::array::from_fn(|i| {
        0.5 + 0.5 * (PI * (2 * i as i64 + 1 - TILE_SIDE as i64) as f64 / (TILE_SIDE - 1) as f64).cos()
    });
    let mut window: Box<[f32; TILE_CELLS]> = vec![0.0; TILE_CELLS].try_into().unwrap();
    for (i, &a) in side.iter().enumerate() {
        for (j, &b) in side.iter().enumerate() {
            window[i * TILE_SIDE + j] = (a * b) as f32;
        }
    }
    window
}

/// The mean of a tile's cells, summed in double precision.
fn tile_mean(cells: &[f32]) -> f64 {
    cells.iter().map(|&value| f64::from(value)).sum::<f64>() / TILE_CELLS as f64
}

/// NumPy's float32 sum (pairwise: 8 running sums for up to 128 values, then halves).
fn sum_f32(values: &[f32]) -> f32 {
    if values.len() < PAIRWISE_LANES {
        return values.iter().fold(0.0f32, |a, &b| a + b);
    }
    if values.len() <= PAIRWISE_BLOCK {
        let mut lanes = [0.0f32; PAIRWISE_LANES];
        lanes.copy_from_slice(&values[..PAIRWISE_LANES]);
        let whole = values.len() - values.len() % PAIRWISE_LANES;
        for chunk in values[PAIRWISE_LANES..whole].chunks_exact(PAIRWISE_LANES) {
            for (a, &b) in lanes.iter_mut().zip(chunk) {
                *a += b;
            }
        }
        let mut total =
            ((lanes[0] + lanes[1]) + (lanes[2] + lanes[3])) + ((lanes[4] + lanes[5]) + (lanes[6] + lanes[7]));
        for &b in &values[whole..] {
            total += b;
        }
        return total;
    }
    let half = values.len() / 2 / PAIRWISE_LANES * PAIRWISE_LANES;
    sum_f32(&values[..half]) + sum_f32(&values[half..])
}

/// NumPy's median: the middle value, or the mean of the middle two.
fn median_f32(values: &mut [f32]) -> f32 {
    values.sort_by(f32::total_cmp);
    let count = values.len();
    if count % 2 == 1 { values[count / 2] } else { (values[count / 2 - 1] + values[count / 2]) / 2.0 }
}

/// The offset of a parabola's vertex from the peak it is fitted through, with the samples before and after it
/// (review.py: `sub`); 0 where they do not curve down.
fn vertex(before: f32, after: f32, peak: f32) -> f32 {
    let curvature = before - 2.0 * peak + after;
    if curvature >= 0.0 { 0.0 } else { 0.5 * (before - after) / curvature }
}

/// One axis of the shifts (degrees), as `pick` takes it from each: x or y.
fn axis(shifts: &[(f32, f32)], pick: fn(&(f32, f32)) -> f32) -> Capped<f32, TILES> {
    shifts.iter().map(pick).collect()
}

/// A frame's camera reading from its tiles' shifts: the mean of the tiles allowed (`clear`) that agree with their
/// median, and how many agree. None when fewer than 3 allowed tiles have a shift, or fewer than 3 agree.
fn agreed(shifts: &TileShifts, clear: &[bool; TILES]) -> CameraReading {
    let allowed: Capped<(f32, f32), TILES> =
        (0..TILES).filter(|&tile| clear[tile]).filter_map(|tile| shifts[tile]).collect();
    if allowed.len() < MIN_AGREEING_TILES {
        return None;
    }
    let median = (median_f32(&mut axis(&allowed, |shift| shift.0)), median_f32(&mut axis(&allowed, |shift| shift.1)));
    let agreeing: Capped<(f32, f32), TILES> =
        allowed.iter().filter(|shift| (shift.0 - median.0).hypot(shift.1 - median.1) < AGREE_DEG).copied().collect();
    if agreeing.len() < MIN_AGREEING_TILES {
        return None;
    }
    let count = agreeing.len() as f32;
    let mean_x = sum_f32(&axis(&agreeing, |shift| shift.0)) / count;
    let mean_y = sum_f32(&axis(&agreeing, |shift| shift.1)) / count;
    Some((f64::from(mean_x), f64::from(mean_y), agreeing.len()))
}

/// The pixels a tile must not be read from: the excluded areas (`keep` false) and the fixed map grown by 3 pixels
/// (SciPy's `binary_dilation`, 3 iterations of the cross).
pub fn excluded(keep: &[bool], fixed: &[u8]) -> Vec<bool> {
    let mut grown: Vec<bool> = fixed.iter().map(|&count| count != 0).collect();
    for _ in 0..FIXED_GROWTH_PX {
        grown = grown_by_one(&grown);
    }
    grown.iter().zip(keep).map(|(&fixed_pixel, &kept)| fixed_pixel || !kept).collect()
}

/// The pixels set (1280 x 720) and the pixels beside them: left, right, above and below.
fn grown_by_one(set: &[bool]) -> Vec<bool> {
    let mut grown = set.to_vec();
    for y in 0..H {
        for x in 0..W {
            let i = y * W + x;
            grown[i] = set[i]
                || (x > 0 && set[i - 1])
                || (x + 1 < W && set[i + 1])
                || (y > 0 && set[i - W])
                || (y + 1 < H && set[i + W]);
        }
    }
    grown
}

/// Copies `source`'s first `target_rows` columns (its rows `source_width` long) into `target` as rows: target's row r
/// is source's column r, `target_width` long (source's first rows).
fn transpose(
    source: &[Complex32],
    source_width: usize,
    target: &mut [Complex32],
    target_rows: usize,
    target_width: usize,
) {
    for row in 0..target_rows {
        for column in 0..target_width {
            target[row * target_width + column] = source[column * source_width + row];
        }
    }
}

/// The normalized cross-power spectrum of a tile's spectra now and before (rows of SPECTRUM_COLUMNS), into `columns`
/// column by column.
fn cross_power_columns(now: &[Complex32], before: &[Complex32], columns: &mut [Complex32]) {
    for column in 0..SPECTRUM_COLUMNS {
        for y in 0..TILE_SIDE {
            let at = y * SPECTRUM_COLUMNS + column;
            let product = now[at] * before[at].conj();
            columns[column * TILE_SIDE + y] = product / product.norm().max(MIN_MAGNITUDE);
        }
    }
}

/// The rows of a real tile's full spectrum from its half (`columns`, SPECTRUM_COLUMNS of TILE_SIDE): each column past
/// the half is the conjugate of its mirror.
fn full_spectrum_rows(columns: &[Complex32], rows: &mut [Complex32]) {
    for y in 0..TILE_SIDE {
        for x in 0..SPECTRUM_COLUMNS {
            rows[y * TILE_SIDE + x] = columns[x * TILE_SIDE + y];
        }
        for x in SPECTRUM_COLUMNS..TILE_SIDE {
            rows[y * TILE_SIDE + x] = columns[(TILE_SIDE - x) * TILE_SIDE + y].conj();
        }
    }
}

/// A cyclic shift (cells) as the one nearest 0: past half a tile it is the other way.
fn wrapped(cells: f32) -> f32 {
    if cells > (TILE_SIDE / 2) as f32 { cells - TILE_SIDE as f32 } else { cells }
}

/// A tile's shift (degrees, right and up positive) from its phase correlation (TILE_SIDE x TILE_SIDE): the highest
/// value's place, each axis refined by a parabola through its neighbors, or None where it peaks under MIN_PEAK.
fn peak_shift(correlation: &[f32]) -> Option<(f32, f32)> {
    let peak = (0..TILE_CELLS).fold(0, |b, i| if correlation[i] > correlation[b] { i } else { b });
    let (peak_y, peak_x) = (peak / TILE_SIDE, peak % TILE_SIDE);
    let height = correlation[peak];
    if height < MIN_PEAK {
        return None;
    }
    let at = |y: usize, x: usize| correlation[y * TILE_SIDE + x];
    let (left, right) = ((peak_x + TILE_SIDE - 1) % TILE_SIDE, (peak_x + 1) % TILE_SIDE);
    let (above, below) = ((peak_y + TILE_SIDE - 1) % TILE_SIDE, (peak_y + 1) % TILE_SIDE);
    let shift_x = peak_x as f32 + vertex(at(peak_y, left), at(peak_y, right), height);
    let shift_y = peak_y as f32 + vertex(at(above, peak_x), at(below, peak_x), height);
    Some((wrapped(shift_x) * GRID_STEP_DEG as f32, -wrapped(shift_y) * GRID_STEP_DEG as f32))
}

/// The camera watch over a recording: fed each frame's luma (1280 x 720), it keeps each frame's tile shifts and its
/// countdown bar's showing; the readings come once the tracks are known (a tile with a target in it is left out).
pub struct CameraWatch {
    /// Where each grid cell is read from on the frame, and each tile's center.
    grid: Grid,
    /// The tiles clear of the pixels no tile may be read from.
    clear_tiles: [bool; TILES],
    /// The forward FFT over TILE_SIDE values: a tile's rows, then its spectrum's columns.
    forward_fft: Arc<dyn Fft<f32>>,
    /// The inverse FFT over TILE_SIDE values, from the cross-power spectrum back to the phase correlation.
    inverse_fft: Arc<dyn Fft<f32>>,
    /// The FFTs' working space, kept from frame to frame.
    scratch: Box<[Complex32]>,
    /// The frame before's tile spectra.
    previous_spectra: Option<Vec<Complex32>>,
    /// The buffers a frame's work needs, kept from frame to frame (each is written whole before it is read).
    work: Work,
    /// Per frame added (or skipped), each tile's shift since the frame before; the first frame's are all None.
    pub shifts: Vec<TileShifts>,
    /// Per frame added (or skipped), whether KovaaK's countdown bar shows.
    pub countdown: Vec<bool>,
}

/// The camera watch's buffers: the grid's tiles, the spectra of the frame before the frame before (filled again with
/// this frame's), and the FFTs' rows, columns and correlation.
#[derive(Default)]
struct Work {
    /// The frame on the grid, as 18 tiles of TILE_CELLS (`Grid::tiles_into`).
    tiles: Vec<f32>,
    /// Spare spectra for the next frame to fill: the frame before's, once this frame's shifts are read from them.
    spectra: Vec<Complex32>,
    /// A tile's rows (TILE_SIDE x TILE_SIDE) for the FFT along x, and the full spectrum's rows on the way back.
    rows: Vec<Complex32>,
    /// A tile's half spectrum column by column (SPECTRUM_COLUMNS columns of TILE_SIDE), for the FFT along y.
    columns: Vec<Complex32>,
    /// A tile's phase correlation (TILE_SIDE x TILE_SIDE, row by row), whose peak gives its shift.
    correlation: Vec<f32>,
}

impl Work {
    /// The buffers at their full sizes, zeroed.
    fn new() -> Work {
        Work {
            tiles: vec![0.0; TILES * TILE_CELLS],
            spectra: vec![Complex32::default(); TILES * SPECTRUM_CELLS],
            rows: vec![Complex32::default(); TILE_CELLS],
            columns: vec![Complex32::default(); SPECTRUM_CELLS],
            correlation: vec![0.0; TILE_CELLS],
        }
    }
}

impl CameraWatch {
    /// A watch with no frames yet. `excluded_pixels` (1280 x 720, `excluded`) are the pixels no tile may be read from:
    /// a tile a tenth or more of which they cover is never read.
    pub fn new(excluded_pixels: &[bool]) -> CameraWatch {
        let grid = Grid::new();
        let excluded_share = grid.tiles(&excluded_pixels.iter().map(|&pixel| u8::from(pixel)).collect::<Vec<u8>>());
        let mut clear_tiles = [false; TILES];
        for (clear, cells) in clear_tiles.iter_mut().zip(excluded_share.chunks_exact(TILE_CELLS)) {
            *clear = tile_mean(cells) < MAX_EXCLUDED_SHARE;
        }
        let mut planner = FftPlanner::new();
        let (forward_fft, inverse_fft) = (planner.plan_fft_forward(TILE_SIDE), planner.plan_fft_inverse(TILE_SIDE));
        let scratch_length = forward_fft.get_inplace_scratch_len().max(inverse_fft.get_inplace_scratch_len());
        CameraWatch {
            grid,
            clear_tiles,
            forward_fft,
            inverse_fft,
            scratch: vec![Complex32::default(); scratch_length].into_boxed_slice(),
            previous_spectra: None,
            work: Work::new(),
            shifts: Vec::new(),
            countdown: Vec::new(),
        }
    }

    /// Each tile's spectrum (TILE_SIDE rows of SPECTRUM_COLUMNS: rfft2 of the tile less its mean, times the Hann
    /// window). The FFTs run a tile at a time: its rows in one call, then its spectrum's columns in one call.
    fn spectra(&mut self, luma: &[u8], work: &mut Work) -> Vec<Complex32> {
        self.grid.tiles_into(luma, &mut work.tiles);
        let mut out = std::mem::take(&mut work.spectra);
        out.resize(TILES * SPECTRUM_CELLS, Complex32::default());
        let (rows, columns) = (&mut work.rows, &mut work.columns);
        for (cells, spectrum) in work.tiles.chunks_exact(TILE_CELLS).zip(out.chunks_exact_mut(SPECTRUM_CELLS)) {
            let mean = tile_mean(cells) as f32;
            for ((value, &cell), &weight) in rows.iter_mut().zip(cells).zip(self.grid.hann.iter()) {
                *value = Complex32::new((cell - mean) * weight, 0.0);
            }
            self.forward_fft.process_with_scratch(rows, &mut self.scratch);
            transpose(rows, TILE_SIDE, columns, SPECTRUM_COLUMNS, TILE_SIDE);
            self.forward_fft.process_with_scratch(columns, &mut self.scratch);
            transpose(columns, TILE_SIDE, spectrum, TILE_SIDE, SPECTRUM_COLUMNS);
        }
        out
    }

    /// Each tile's shift from the frame before (`previous`, its spectra) to this one (`current`).
    fn tile_shifts(&mut self, current: &[Complex32], previous: &[Complex32], work: &mut Work) -> TileShifts {
        let mut shifts = [None; TILES];
        let (columns, rows, correlation) = (&mut work.columns, &mut work.rows, &mut work.correlation);
        let tiles = current.chunks_exact(SPECTRUM_CELLS).zip(previous.chunks_exact(SPECTRUM_CELLS));
        for (shift, (now, before)) in shifts.iter_mut().zip(tiles) {
            cross_power_columns(now, before, columns);
            // irfft2: the inverse along y for each column (one call), then the real inverse along x for each row
            self.inverse_fft.process_with_scratch(columns, &mut self.scratch);
            full_spectrum_rows(columns, rows);
            self.inverse_fft.process_with_scratch(rows, &mut self.scratch);
            for (value, sum) in correlation.iter_mut().zip(rows.iter()) {
                *value = sum.re / TILE_CELLS as f32;
            }
            *shift = peak_shift(correlation);
        }
        shifts
    }

    /// The watch for a recording, its tiles kept clear of the KovOBS overlay and of the fixed map (1280 x 720, a
    /// pixel fixed where its count is not 0).
    pub fn for_recording(fixed: &[u8]) -> CameraWatch {
        let overlay = crate::track::Mask::without(&crate::geometry::overlay_shares());
        CameraWatch::new(&excluded(overlay.kept(), fixed))
    }

    /// The readings, the tracks known: each frame's camera turn, and whether the countdown bar shows.
    pub fn finish(self, frames: &[TrackFrame]) -> VideoReadings {
        VideoReadings { camera: self.readings(frames), countdown: self.countdown }
    }

    /// Adds `frames` frames with no shifts and no countdown: the frames before the first one reviewed, for a review
    /// that starts part way in. Call it before any frame is added.
    pub fn skip(&mut self, frames: usize) {
        self.shifts.extend(std::iter::repeat_n([None; TILES], frames));
        self.countdown.extend(std::iter::repeat_n(false, frames));
    }

    /// What this run part of the recording read, to join with the other parts' (a recording split into run parts,
    /// reviewed in workers at once, has a watch for each).
    pub fn part(self) -> CameraPart {
        CameraPart { shifts: self.shifts, countdown: self.countdown }
    }

    /// Adds the next run part's readings. Each part but the last also reads the next part's first frame, for the
    /// camera's turn into it; the next part read that frame without the one before it, so its first entry is left
    /// out (unless this watch has no frames yet).
    pub fn join(&mut self, next: CameraPart) {
        let skip = usize::from(!self.countdown.is_empty());
        self.shifts.extend(next.shifts.into_iter().skip(skip));
        self.countdown.extend(next.countdown.into_iter().skip(skip));
    }

    /// One frame: its luma (1280 x 720) and its RGB24 (for the countdown bar).
    pub fn add(&mut self, luma: &[u8], rgb: &[u8]) {
        self.countdown.push(countdown_showing(rgb));
        let mut work = std::mem::take(&mut self.work);
        let spectra = self.spectra(luma, &mut work);
        let shifts = match self.previous_spectra.take() {
            Some(previous) => {
                let shifts = self.tile_shifts(&spectra, &previous, &mut work);
                work.spectra = previous;
                shifts
            }
            None => [None; TILES],
        };
        self.work = work;
        self.previous_spectra = Some(spectra);
        self.shifts.push(shifts);
    }

    /// A frame's reading from its tiles' shifts, leaving out the tiles a tracked target covers in any frame of `near`
    /// (the frame and the one before).
    pub fn reading(&self, shifts: &TileShifts, near: &[&TrackFrame]) -> CameraReading {
        let mut clear = self.clear_tiles;
        for frame in near {
            for (&(_, x, y), &area_px) in frame.t.iter().zip(&frame.a) {
                let reach_deg = disc_radius_deg(area_px) + TARGET_MARGIN_DEG;
                let within_deg = TILE_HALF_DEG + reach_deg;
                let centers = self.grid.tile_azimuth_deg.iter().zip(&self.grid.tile_elevation_deg);
                for (tile_clear, (&azimuth, &elevation)) in clear.iter_mut().zip(centers) {
                    if (azimuth - x).abs() < within_deg && (elevation - y).abs() < within_deg {
                        *tile_clear = false;
                    }
                }
            }
        }
        agreed(shifts, &clear)
    }

    /// Every frame's reading, the tracks known. The first frame has none; a frame past the tracks' end is read with no
    /// tile left out for targets.
    pub fn readings(&self, frames: &[TrackFrame]) -> Vec<CameraReading> {
        (0..self.shifts.len())
            .map(|i| {
                if i == 0 {
                    return None;
                }
                let near: &[&TrackFrame] = if i < frames.len() { &[&frames[i - 1], &frames[i]] } else { &[] };
                self.reading(&self.shifts[i], near)
            })
            .collect()
    }
}

/// What a tracking run reads from the video besides the tracks (CameraWatch::finish).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct VideoReadings {
    /// Per frame, the camera's reading; None on the first frame and where too few tiles agree.
    pub camera: Vec<CameraReading>,
    /// Per frame, whether KovaaK's countdown bar shows.
    pub countdown: Vec<bool>,
}

/// A run's part of the camera watch (CameraWatch::part): each frame's tile shifts and whether the countdown bar shows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraPart {
    /// Per frame of the part, each tile's shift since the frame before (`CameraWatch::shifts`).
    pub shifts: Vec<TileShifts>,
    /// Per frame of the part, whether KovaaK's countdown bar shows.
    pub countdown: Vec<bool>,
}

/// The rows of a frame the countdown test reads (y from, to): the camera watch needs only these rows of a frame's RGB.
pub const COUNTDOWN_ROWS: (usize, usize) = (214, 247);
/// The countdown bar's box's columns (x from, to).
const COUNTDOWN_COLUMNS: (usize, usize) = (520, 761);
/// The bar's first row, below its box's top (pixels).
const BAR_TOP: usize = 6;
/// The bar's height (pixels).
const BAR_ROWS: usize = 21;
/// The fill's color is read from the bar's left end: its first column, after the box's left (pixels).
const FILL_SAMPLE_LEFT: usize = 2;
/// The columns the fill's color is read from.
const FILL_SAMPLE_COLUMNS: usize = 4;
/// The track must show at the bar's right end: its first column, before the box's right (pixels).
const TRACK_END_LEFT: usize = 12;
/// The columns at the bar's right end that must show the track.
const TRACK_END_COLUMNS: usize = 8;
/// The bar's dark gray track (RGB).
const TRACK_RGB: [f64; 3] = [64.0, 60.0, 68.0];
/// A fill color this near the track's in every channel is the track: no fill shows.
const FILL_LIKE_TRACK: f64 = 40.0;
/// A pixel this near the fill's color in every channel is fill.
const FILL_TOLERANCE: f64 = 24.0;
/// A pixel this near the track's color in every channel is track.
const TRACK_TOLERANCE: f64 = 12.0;
/// A pixel this bright in every channel is a digit's (white).
const WHITE_MIN: i32 = 170;
/// The fill pixels a bar needs.
const MIN_FILL_PIXELS: usize = 40;
/// The share of the box that must be track.
const MIN_TRACK_SHARE: f64 = 0.05;
/// The share of the box that must be known: fill, track or a digit's white.
const MIN_KNOWN_SHARE: f64 = 0.9;
/// The share of the bar's right end that must be track.
const MIN_TRACK_END_SHARE: f64 = 0.8;

/// A pixel of a frame's RGB24 (1280 x 720).
fn pixel(rgb: &[u8], x: usize, y: usize) -> [i32; 3] {
    let i = (y * W + x) * 3;
    [i32::from(rgb[i]), i32::from(rgb[i + 1]), i32::from(rgb[i + 2])]
}

/// Whether a color is within `tolerance` of another in every channel.
fn near_color(color: [i32; 3], to: [f64; 3], tolerance: f64) -> bool {
    (0..3).all(|channel| (f64::from(color[channel]) - to[channel]).abs() <= tolerance)
}

/// The fill's color: per channel, the median of the bar's left end, which the fill covers to the last frame.
fn fill_color(rgb: &[u8]) -> [f64; 3] {
    let (left, top) = (COUNTDOWN_COLUMNS.0 + FILL_SAMPLE_LEFT, COUNTDOWN_ROWS.0 + BAR_TOP);
    std::array::from_fn(|channel| {
        let mut values: [i32; BAR_ROWS * FILL_SAMPLE_COLUMNS] =
            std::array::from_fn(|j| pixel(rgb, left + j % FILL_SAMPLE_COLUMNS, top + j / FILL_SAMPLE_COLUMNS)[channel]);
        values.sort();
        let middle = values.len() / 2;
        (values[middle - 1] + values[middle]) as f64 / 2.0
    })
}

/// What the countdown box holds: its fill pixels and its track pixels, each with the sum of their x from the box's
/// left, and the pixels known (fill, track or a digit's white).
#[derive(Default)]
struct BoxCounts {
    /// The pixels near the fill's color.
    fill: usize,
    /// The pixels near the track's dark gray.
    track: usize,
    /// The pixels that are fill, track or a digit's white.
    known: usize,
    /// The fill pixels' x from the box's left, summed (pixels), for their mean.
    fill_x_sum: usize,
    /// The track pixels' x from the box's left, summed (pixels), for their mean.
    track_x_sum: usize,
}

impl BoxCounts {
    /// Counts the countdown box's pixels in a frame's RGB24 (1280 x 720), with the fill's color `fill_rgb`.
    fn read(rgb: &[u8], fill_rgb: [f64; 3]) -> BoxCounts {
        let mut counts = BoxCounts::default();
        let ((left, right), (top, bottom)) = (COUNTDOWN_COLUMNS, COUNTDOWN_ROWS);
        for y in top..bottom {
            for x in left..right {
                let color = pixel(rgb, x, y);
                let is_fill = near_color(color, fill_rgb, FILL_TOLERANCE);
                let is_track = near_color(color, TRACK_RGB, TRACK_TOLERANCE);
                let white = color.iter().all(|&value| value >= WHITE_MIN);
                counts.fill += usize::from(is_fill);
                counts.track += usize::from(is_track);
                counts.known += usize::from(is_fill || is_track || white);
                counts.fill_x_sum += if is_fill { x - left } else { 0 };
                counts.track_x_sum += if is_track { x - left } else { 0 };
            }
        }
        counts
    }
}

/// How many of the pixels at the bar's right end are track.
fn track_end_pixels(rgb: &[u8]) -> usize {
    let (left, top) = (COUNTDOWN_COLUMNS.1 - TRACK_END_LEFT, COUNTDOWN_ROWS.0 + BAR_TOP);
    (top..top + BAR_ROWS)
        .flat_map(|y| (left..left + TRACK_END_COLUMNS).map(move |x| (x, y)))
        .filter(|&(x, y)| near_color(pixel(rgb, x, y), TRACK_RGB, TRACK_TOLERANCE))
        .count()
}

/// Whether a frame (RGB24, 1280 x 720) shows KovaaK's countdown bar ("Challenge begins in"): its box (x 520 to 760,
/// y 214 to 246) holds the bar's dark gray track and one fill color, the fill on the left, the track at the right
/// end, and white digits. The fill takes the HUD's color, so the color is read from the bar's left end, which the
/// fill covers to the last frame.
pub fn countdown_showing(rgb: &[u8]) -> bool {
    let fill_rgb = fill_color(rgb);
    if (0..3).all(|channel| (fill_rgb[channel] - TRACK_RGB[channel]).abs() <= FILL_LIKE_TRACK) {
        return false;
    }
    let counts = BoxCounts::read(rgb, fill_rgb);
    let area = (COUNTDOWN_COLUMNS.1 - COUNTDOWN_COLUMNS.0) * (COUNTDOWN_ROWS.1 - COUNTDOWN_ROWS.0);
    if counts.fill < MIN_FILL_PIXELS
        || (counts.track as f64) < MIN_TRACK_SHARE * area as f64
        || (counts.known as f64) < MIN_KNOWN_SHARE * area as f64
    {
        return false;
    }
    let fill_left_of_track =
        (counts.fill_x_sum as f64 / counts.fill as f64) < (counts.track_x_sum as f64 / counts.track as f64);
    fill_left_of_track && track_end_pixels(rgb) as f64 > MIN_TRACK_END_SHARE * (BAR_ROWS * TRACK_END_COLUMNS) as f64
}
