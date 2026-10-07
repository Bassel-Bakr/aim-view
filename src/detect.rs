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
/// when its score, on the reference model's scale, passes the model's threshold (its settings file), or its weaker one
/// at the crosshair with the box's center that near it (`AtCrosshair`); the box keeps that score. The position is
/// worked out in float64 and stored as float32, as NumPy does it.
pub fn decode(
    score: &[f32],
    regression: &[f32],
    grid_width: usize,
    grid_height: usize,
    model: &ModelSettings,
) -> Vec<RawBox> {
    let cells = grid_width * grid_height;
    let floor = model.floor();
    let (crosshair_x, crosshair_y) = crate::geometry::to_px(0.0, 0.0);
    let mut out = Vec::new();
    for y in 0..grid_height {
        for x in 0..grid_width {
            let i = y * grid_width + x;
            if score[i] as f64 <= floor {
                continue;
            }
            let mapped_score = model.mapped(score[i]);
            let cx = ((x as f64 + regression[i] as f64) * STRIDE) as f32;
            let cy = ((y as f64 + regression[cells + i] as f64) * STRIDE) as f32;
            let at_crosshair = model.at_crosshair.as_ref().is_some_and(|weak| {
                mapped_score > weak.threshold
                    && (cx as f64 - crosshair_x).hypot(cy as f64 - crosshair_y) <= weak.reach_px
            });
            if mapped_score > model.threshold || at_crosshair {
                let (width, height) = (regression[2 * cells + i].exp(), regression[3 * cells + i].exp());
                out.push(RawBox { cx, cy, w: width, h: height, score: mapped_score });
            }
        }
    }
    out
}

/// Checks the weaker threshold at the crosshair.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AtCrosshair;

    /// A 320 x 180 grid (1280 x 720 at STRIDE 4) with one cell scored `score` at the cell holding (x_px, y_px).
    fn one_cell(x_px: f64, y_px: f64, score: f32) -> (Vec<f32>, Vec<f32>, usize, usize) {
        let (grid_width, grid_height) = (320, 180);
        let mut scores = vec![0f32; grid_width * grid_height];
        scores[(y_px / STRIDE) as usize * grid_width + (x_px / STRIDE) as usize] = score;
        (scores, vec![0f32; 4 * grid_width * grid_height], grid_width, grid_height)
    }

    /// A cell under the model's threshold but over the weaker one is a box at the crosshair and nowhere else.
    #[test]
    fn a_weak_cell_is_a_target_only_at_the_crosshair() {
        let weak = ModelSettings {
            at_crosshair: Some(AtCrosshair { threshold: 0.2, reach_px: 30.0, kinds: None }),
            ..ModelSettings::default()
        };
        let (crosshair_x, crosshair_y) = crate::geometry::to_px(0.0, 0.0);
        let boxes = |settings: &ModelSettings, x_px: f64, score: f32| {
            let (scores, regression, grid_width, grid_height) = one_cell(x_px, crosshair_y, score);
            decode(&scores, &regression, grid_width, grid_height, settings).len()
        };
        assert_eq!(boxes(&weak, crosshair_x, 0.25), 1, "at the crosshair");
        assert_eq!(boxes(&weak, crosshair_x + 100.0, 0.25), 0, "away from it");
        assert_eq!(boxes(&weak, crosshair_x, 0.15), 0, "under the weaker threshold");
        assert_eq!(boxes(&weak, crosshair_x + 100.0, 0.35), 1, "over the model's own threshold anywhere");
        assert_eq!(boxes(&ModelSettings::default(), crosshair_x, 0.25), 0, "a model without the rule");
    }
}
