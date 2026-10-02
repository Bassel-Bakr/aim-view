//! The fixed map: the pixels that stay put on screen while the view moves (crosshair, HUD text, a gun model), found in
//! the recording's key frames. It is the detector model's 4th input (python/review.py: `contrast`, `_blur_up`,
//! `fixed_map`). The arithmetic is NumPy's and SciPy's, float32 where they keep float32, so the map is the same bit
//! for bit.

use crate::geometry::{H, W};
use crate::scipy::{Edge, uniform_filter};

/// How far a spot differs from the wall behind it: brightness difference + 2 x colour difference.
pub const DIFF: f32 = 30.0;
/// A pixel is fixed when it stands out in at least this share of the key frames.
pub const SHARE: f64 = 0.8;

/// The wall behind every pixel of a plane (h x w): block means over block x block pixels (sums in integers, divided
/// in float32), blurred over 5 x 5 blocks, each block's value repeated `up` times both ways.
fn blur_up(plane: &[u8], w: usize, h: usize, block: usize, up: usize) -> Vec<f32> {
    let (bw, bh) = (w / block, h / block);
    let mut small = vec![0f32; bw * bh];
    for by in 0..bh {
        for bx in 0..bw {
            let mut sum = 0u32;
            for y in by * block..(by + 1) * block {
                for x in bx * block..(bx + 1) * block {
                    sum += plane[y * w + x] as u32;
                }
            }
            small[by * bw + bx] = sum as f32 / (block * block) as f32;
        }
    }
    let small = uniform_filter(&small, bh, bw, 5, Edge::Nearest);
    let (ow, oh) = (bw * up, bh * up);
    let mut out = vec![0f32; ow * oh];
    for y in 0..oh {
        for x in 0..ow {
            out[y * ow + x] = small[(y / up) * bw + x / up];
        }
    }
    out
}

/// How far every pixel of a 1280 x 720 YUV 4:2:0 frame differs from the wall behind it, in any colour:
/// |Y - wall| + 2 x (|U - wall| + |V - wall|), the colour planes at half size.
pub fn contrast(yuv: &[u8]) -> Vec<f32> {
    let (y, rest) = yuv.split_at(W * H);
    let (u, v) = rest.split_at(W * H / 4);
    let (cw, ch) = (W / 2, H / 2);
    let (wy, wu, wv) = (blur_up(y, W, H, 4, 4), blur_up(u, cw, ch, 2, 2), blur_up(v, cw, ch, 2, 2));
    let mut c = vec![0f32; W * H];
    for row in 0..H {
        for col in 0..W {
            let i = row * W + col;
            let j = (row / 2) * cw + col / 2;
            let cc = (u[j] as f32 - wu[j]).abs() + (v[j] as f32 - wv[j]).abs();
            c[i] = (y[i] as f32 - wy[i]).abs() + 2.0 * cc;
        }
    }
    c
}

/// Counts, per pixel, the key frames it stands out in.
#[derive(Clone, Debug)]
pub struct FixedMap {
    counts: Vec<u16>,
    frames: usize,
}

impl Default for FixedMap {
    fn default() -> Self {
        FixedMap { counts: vec![0; W * H], frames: 0 }
    }
}

impl FixedMap {
    /// One key frame, YUV 4:2:0 at 1280 x 720.
    pub fn add(&mut self, yuv: &[u8]) {
        for (n, c) in self.counts.iter_mut().zip(contrast(yuv)) {
            *n += (c > DIFF) as u16;
        }
        self.frames += 1;
    }

    /// The map: 1 where a pixel stood out in at least 80% of the key frames, else 0 (row by row).
    pub fn map(&self) -> Vec<u8> {
        let n = self.frames as f64;
        self.counts.iter().map(|&c| (c as f64 / n >= SHARE) as u8).collect()
    }
}
