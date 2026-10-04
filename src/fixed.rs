//! The fixed map: the pixels that stay put on screen while the view moves (crosshair, HUD text, a gun model), found in
//! the recording's key frames. It is the detector model's 4th input (python/retired/review.py: `contrast`, `_blur_up`,
//! `fixed_map`). The arithmetic is NumPy's and SciPy's, float32 where they keep float32, so the map is the same bit
//! for bit.
//!
//! In: the key frames, as 720p YUV 4:2:0 (the review session's key frame pass: src/session.rs `Keys`). Out: the map,
//! which the browser's review worker and the native review give the detector with every frame, and which keeps the
//! camera watch's tiles off the HUD; the area finder (areas.rs) reads each key frame's `contrast` too.

use crate::geometry::{H, W};
use crate::scipy::{Edge, uniform_filter};

/// How far a spot differs from the wall behind it: brightness difference + 2 x colour difference.
pub const DIFF: f32 = 30.0;
/// A pixel is fixed when it stands out in at least this share of the key frames.
pub const SHARE: f64 = 0.8;
/// The colour difference counts this many times the brightness difference.
const COLOUR_WEIGHT: f32 = 2.0;
/// The wall's blocks: 4 x 4 pixels of the frame, which are 4 x 4 pixels of Y and 2 x 2 of U and V at half size.
const BLOCK_PX: usize = 4;
const CHROMA_BLOCK_PX: usize = 2;
/// The colour planes of YUV 4:2:0 are half the frame's width and height.
const CHROMA_SCALE: usize = 2;
/// The wall is blurred over this many blocks a side.
const WALL_BLUR_BLOCKS: usize = 5;

/// The wall behind a plane (height x width), one value a block: block means over block x block pixels (sums in
/// integers, divided in float32), blurred over 5 x 5 blocks. Python repeats each block's value over its pixels
/// (`_blur_up`); here a pixel reads its block's.
fn walls(plane: &[u8], width: usize, height: usize, block: usize) -> Vec<f32> {
    let (blocks_wide, blocks_high) = (width / block, height / block);
    let mut small = vec![0f32; blocks_wide * blocks_high];
    for block_row in 0..blocks_high {
        for block_col in 0..blocks_wide {
            let mut sum = 0u32;
            for y in block_row * block..(block_row + 1) * block {
                for x in block_col * block..(block_col + 1) * block {
                    sum += plane[y * width + x] as u32;
                }
            }
            small[block_row * blocks_wide + block_col] = sum as f32 / (block * block) as f32;
        }
    }
    uniform_filter(&small, blocks_high, blocks_wide, WALL_BLUR_BLOCKS, Edge::Nearest)
}

/// How far every pixel of a 1280 x 720 YUV 4:2:0 frame differs from the wall behind it, in any colour:
/// |Y - wall| + 2 x (|U - wall| + |V - wall|), the colour planes at half size.
pub fn contrast(yuv: &[u8]) -> Vec<f32> {
    let (luma, chroma) = yuv.split_at(W * H);
    let (u_plane, v_plane) = chroma.split_at(W * H / (CHROMA_SCALE * CHROMA_SCALE));
    let (chroma_width, chroma_height) = (W / CHROMA_SCALE, H / CHROMA_SCALE);
    // all three walls have a value for each 4 x 4 block of the frame (Y's blocks of 4, U's and V's of 2 at half size)
    let luma_wall = walls(luma, W, H, BLOCK_PX);
    let u_wall = walls(u_plane, chroma_width, chroma_height, CHROMA_BLOCK_PX);
    let v_wall = walls(v_plane, chroma_width, chroma_height, CHROMA_BLOCK_PX);
    let blocks_wide = W / BLOCK_PX;
    let mut out = vec![0f32; W * H];
    for row in 0..H {
        let row_blocks = (row / BLOCK_PX) * blocks_wide;
        for col in 0..W {
            let pixel = row * W + col;
            let chroma_pixel = (row / CHROMA_SCALE) * chroma_width + col / CHROMA_SCALE;
            let block = row_blocks + col / BLOCK_PX;
            let colour = (u_plane[chroma_pixel] as f32 - u_wall[block]).abs()
                + (v_plane[chroma_pixel] as f32 - v_wall[block]).abs();
            out[pixel] = (luma[pixel] as f32 - luma_wall[block]).abs() + COLOUR_WEIGHT * colour;
        }
    }
    out
}

/// Counts, per pixel, the key frames it stands out in.
#[derive(Clone, Debug)]
pub struct FixedMap {
    counts: Box<[u16; W * H]>,
    frames: usize,
}

impl Default for FixedMap {
    fn default() -> Self {
        FixedMap { counts: vec![0; W * H].try_into().unwrap(), frames: 0 }
    }
}

impl FixedMap {
    /// One key frame, YUV 4:2:0 at 1280 x 720.
    pub fn add(&mut self, yuv: &[u8]) {
        self.add_contrast(&contrast(yuv));
    }

    /// One key frame's `contrast`, when the caller needs it for something else too.
    pub fn add_contrast(&mut self, contrast: &[f32]) {
        for (count, &difference) in self.counts.iter_mut().zip(contrast) {
            *count += (difference > DIFF) as u16;
        }
        self.frames += 1;
    }

    /// The map: 1 where a pixel stood out in at least 80% of the key frames, else 0 (row by row).
    pub fn map(&self) -> Vec<u8> {
        let frames = self.frames as f64;
        self.counts.iter().map(|&count| (count as f64 / frames >= SHARE) as u8).collect()
    }
}
