//! What each frame keeps of the HUD (src/hud/mod.rs): its value rows' glyphs, each row's kept once while it stays
//! the same from frame to frame, so a long recording stays small (`GlyphStore`).
//!
//! In: each frame's glyphs, cut by layout.rs, from the watch (watch.rs). Out: the glyph lines the readers
//! (reading.rs) learn the digits from and read.

use std::iter::repeat_n;

use super::layout::Cut;
use super::reading::tall;
use super::{GLYPH_PIXELS, RECENT_GLYPHS, ROWS};
use crate::capped::Capped;

/// A row's glyphs are the ones before while every glyph has the same ink size and differs from the kept image by at
/// most NEAR_MAX at any pixel and NEAR_SUM in all (the box is see-through: the scene behind it moves the grey levels).
const NEAR_MAX: u8 = 24;
/// The most two images of the same glyph differ by over all their pixels (3 levels a pixel on average).
const NEAR_SUM: u32 = 3 * GLYPH_PIXELS as u32;

/// A stored glyph: its image's index, and its ink's height and width.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Glyph {
    /// The image's index in the store's images.
    pub(super) image: u32,
    /// The ink's height in the band, pixels.
    pub(super) height: u16,
    /// The glyph's width in the band, pixels.
    pub(super) width: u16,
}

/// The glyphs each frame read in each row: the distinct lines of glyphs, and each row's line frame by frame.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct GlyphStore {
    /// The glyph images, GLYPH_PIXELS bytes each (the ink's strength, 0 to 255).
    pub(super) images: Vec<u8>,
    /// The distinct lines of glyphs; line 0 is the empty one.
    pub(super) lines: Vec<Box<[Glyph]>>,
    /// Each row's lines frame by frame, as runs (line, frames).
    pub(super) rows: [Vec<(u32, u32)>; ROWS],
}

impl Default for GlyphStore {
    /// A store with only the empty line 0 and no frames.
    fn default() -> GlyphStore {
        GlyphStore { images: Vec::new(), lines: vec![Box::default()], rows: Default::default() }
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

impl GlyphStore {
    /// The glyph image at `index`, GLYPH_PIXELS bytes.
    pub(super) fn image(&self, index: u32) -> &[u8] {
        &self.images[index as usize * GLYPH_PIXELS..(index as usize + 1) * GLYPH_PIXELS]
    }

    /// Whether a stored glyph and a cut are the same glyph: the same ink size and a near image.
    fn same_glyph(&self, glyph: &Glyph, cut: &Cut) -> bool {
        glyph.height == cut.height && glyph.width == cut.width && near(self.image(glyph.image), &cut.image)
    }

    /// A line's tall glyphs, in a band `band` rows high.
    pub(super) fn tall_glyphs(&self, line: u32, band: usize) -> impl Iterator<Item = &Glyph> {
        self.lines[line as usize].iter().filter(move |glyph| tall(glyph, band))
    }

    /// Adds `frames` frames of `line` to a row: the last run grows when it is the same line; no frames add nothing.
    pub(super) fn add_run(&mut self, row: usize, line: u32, frames: u32) {
        match self.rows[row].last_mut() {
            Some(last) if last.0 == line => last.1 += frames,
            _ if frames > 0 => self.rows[row].push((line, frames)),
            _ => {}
        }
    }

    /// One frame's glyphs in a row: the row's line before when they are the same glyphs, else a new line, whose
    /// glyphs reuse the images of the row's `recent` glyphs they are the same as (a number changes a digit at a time).
    pub(super) fn push(&mut self, row: usize, cuts: Vec<Cut>, recent: &mut Capped<Glyph, RECENT_GLYPHS>) {
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
    pub(super) fn per_frame(&self, row: usize) -> Vec<u32> {
        self.rows[row].iter().flat_map(|&(line, frames)| repeat_n(line, frames as usize)).collect()
    }

    /// The next run part's store after this one's, its first `left_out` frames left out.
    pub(super) fn append(&mut self, next: GlyphStore, left_out: usize) {
        let image_offset = (self.images.len() / GLYPH_PIXELS) as u32;
        let line_offset = self.lines.len() as u32 - 1;
        self.images.extend(next.images);
        self.lines.extend(
            next.lines.into_iter().skip(1).map(|line| {
                line.into_iter().map(|glyph| Glyph { image: glyph.image + image_offset, ..glyph }).collect()
            }),
        );
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
