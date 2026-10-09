//! The HUD watch (src/hud/mod.rs): the key frames' layout, then each frame's glyphs, a run part at a time, its part
//! saved as JSON and the parts joined, then the reading.
//!
//! In: the key frames' and frames' Y planes from the review (src/session.rs). Out: each run part's `HudPart`, the
//! joined watch's `HudReading` (reading.rs reads it), and `HudKeys` / `SessionRows` for the run parts and the area
//! finder.

use serde::{Deserialize, Serialize};

use super::glyphs::{Glyph, GlyphStore};
use super::layout::{Cut, Layout, SessionRows, aim_glyphs, find_box, median_of_keys, value_glyphs};
use super::reading::{aimlab, kovaak};
use super::scale::{Filter, Scale, read_levels};
use super::{
    ACCURACY_ROW, AH, AIM_BAND, AIM_POINTS, AIM_TIME, AW, BH, BOX, BW, GLYPH_PIXELS, HudReading, KILL_COUNT_ROW,
    POINTS_ROW, RECENT_GLYPHS, ROWS, TIME_ROW,
};
use crate::capped::Capped;

/// The most key frames kept for the box's layout: past it every other one is dropped (a median needs no more). The
/// layout needs at least MIN_KEY_FRAMES (python/hud.py's layout).
const MAX_KEY_FRAMES: usize = 64;
/// The fewest key frames the box's layout is worked out from; with fewer there is no box.
const MIN_KEY_FRAMES: usize = 3;

/// A run part's share of the watch (a review split into run parts: each part's watch reads its own frames, the page
/// joins them).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "PartText", into = "PartText")]
pub struct HudPart {
    /// The frames the part's watch read, skipped ones included.
    pub(super) frames: usize,
    /// KovaaK's box, None without one (each run part's watch works it out from the same key frames).
    pub(super) layout: Option<Layout>,
    /// The box's text rows, None without a box.
    pub(super) session: Option<SessionRows>,
    /// The glyphs the part's frames read.
    pub(super) store: GlyphStore,
}

/// What a watch reads in the key frames (`HudWatch::keys`): KovaaK's box, None without one, and its text rows. Each
/// run's watch starts from it (`HudWatch::from_keys`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HudKeys {
    /// KovaaK's box's value rows; None without a box, or a compact box whose colons were not found.
    pub(super) layout: Option<Layout>,
    /// The box's text rows; None without a box.
    pub(super) session: Option<SessionRows>,
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
        let store = GlyphStore { images, lines, rows };
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
    pub(super) store: GlyphStore,
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
            store: GlyphStore::default(),
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
