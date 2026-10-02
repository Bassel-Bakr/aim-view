//! The detector model's output maps as boxes (python/model/infer.py: `decode_np`, for the `_u8in` and `_fp32`
//! exports, whose score map already holds only the peaks).

use crate::track::RawBox;

/// The score a cell must pass to be a target (infer.THRESHOLD, chosen on the val split).
pub const THRESHOLD: f32 = 0.3;
/// The maps' cells are this many pixels on a side.
pub const STRIDE: f64 = 4.0;

/// The boxes in the maps, row by row as NumPy's `nonzero` finds them: score (gh x gw) and reg (4 x gh x gw: x and y
/// offsets in cells, log width and log height). The position is worked out in float64 and stored as float32, as
/// NumPy does it.
pub fn decode(score: &[f32], reg: &[f32], gw: usize, gh: usize, threshold: f32) -> Vec<RawBox> {
    let n = gw * gh;
    let mut out = Vec::new();
    for y in 0..gh {
        for x in 0..gw {
            let i = y * gw + x;
            let s = score[i];
            if s > threshold {
                out.push(RawBox {
                    cx: ((x as f64 + reg[i] as f64) * STRIDE) as f32,
                    cy: ((y as f64 + reg[n + i] as f64) * STRIDE) as f32,
                    w: reg[2 * n + i].exp(),
                    h: reg[3 * n + i].exp(),
                    score: s,
                });
            }
        }
    }
    out
}
