//! The detector model's output maps as boxes (python/model/infer.py: `decode_np`, for the `_u8in` and `_fp32`
//! exports, whose score map already holds only the peaks).
//!
//! In: one frame's maps from the detector (the browser's review worker or the native review runs it) and the model's
//! settings (model.rs). Out: the frame's boxes in pixels, which the tracker (tracker.rs) keeps or drops.

use crate::model::ModelSettings;
use crate::track::RawBox;

/// The maps' cells are this many pixels on a side.
pub const STRIDE: f64 = 4.0;

/// The boxes in the maps, row by row as NumPy's `nonzero` finds them: `score` (grid_height x grid_width) and
/// `regression` (4 x grid_height x grid_width: x and y offsets in cells, log width and log height). A cell is a box
/// when its score, on the reference model's scale, passes the model's threshold (its settings file); the box keeps
/// that score. The position is worked out in float64 and stored as float32, as NumPy does it.
pub fn decode(
    score: &[f32],
    regression: &[f32],
    grid_width: usize,
    grid_height: usize,
    model: &ModelSettings,
) -> Vec<RawBox> {
    let cells = grid_width * grid_height;
    let floor = model.floor();
    let mut out = Vec::new();
    for y in 0..grid_height {
        for x in 0..grid_width {
            let i = y * grid_width + x;
            if score[i] as f64 <= floor {
                continue;
            }
            if let Some(mapped_score) = model.passes(score[i]) {
                out.push(RawBox {
                    cx: ((x as f64 + regression[i] as f64) * STRIDE) as f32,
                    cy: ((y as f64 + regression[cells + i] as f64) * STRIDE) as f32,
                    w: regression[2 * cells + i].exp(),
                    h: regression[3 * cells + i].exp(),
                    score: mapped_score,
                });
            }
        }
    }
    out
}
