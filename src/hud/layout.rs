//! Where the HUD's boxes and their text rows are (src/hud/mod.rs): the grey levels and ink of a scaled region,
//! KovaaK's session box found in the key frames' median (a flood fill from a patch of the box's level) and its value
//! rows, and each row's glyphs cut apart; Aim Lab's POINTS and TIME boxes' glyphs too.
//!
//! In: the key frames' and each frame's scaled regions (scale.rs), from the watch (watch.rs). Out: the box's layout
//! (`HudKeys`, `SessionRows` for the area finder) and each frame's glyphs (`Cut`), which glyphs.rs keeps.

use std::ops::Range;

use serde::{Deserialize, Serialize};

use super::scale::glyph_image;
use super::watch::HudKeys;
use super::{AH, AW, BH, BW, GLYPH_PIXELS};
use crate::statistics::median;

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
/// A box shows a value when its TOP_PERCENTILE-th percentile level is at least this many grey levels above its
/// background (the value is white).
const AIM_MIN_TOP_LEVELS: f64 = 40.0;
/// A glyph of two dots or more, at most this many times as wide as it is tall, is the colon.
const COLON_MAX_ASPECT: f64 = 0.5;

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
pub(super) struct Row {
    /// The band's first row in the scaled region.
    pub(super) y0: usize,
    /// The row after the band's last.
    pub(super) y1: usize,
    /// The column the value can start at, from the box's x0; None: the rightmost group of ink.
    pub(super) start: Option<usize>,
}

/// Where KovaaK's box has its values: the columns x0..x1 of the scaled region, and the Kill Count and Accuracy rows.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Layout {
    /// The first column read, inside the box's left edge.
    pub(super) x0: usize,
    /// The column after the last one read, inside the box's right edge.
    pub(super) x1: usize,
    /// The Kill Count row.
    pub(super) kills: Row,
    /// The Accuracy row.
    pub(super) accuracy: Row,
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
pub(super) fn median_of_keys(keys: &[Box<[u8]>]) -> Vec<f64> {
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
pub(super) fn find_box(key_median: &[f64]) -> HudKeys {
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
pub(super) struct Cut {
    /// The ink's strength scaled to GLYPH_WIDTH_PX x GLYPH_HEIGHT_PX (0 to 255).
    pub(super) image: [u8; GLYPH_PIXELS],
    /// The ink's height in the band, pixels.
    pub(super) height: u16,
    /// The glyph's width in the band, pixels.
    pub(super) width: u16,
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
pub(super) fn value_glyphs(band: &[u8], width: usize, start: Option<usize>) -> Vec<Cut> {
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
pub(super) fn aim_glyphs(band: &[u8], (c0, c1): (usize, usize)) -> Vec<Cut> {
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
