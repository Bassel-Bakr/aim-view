//! The frame's geometry at the size the review works in (1280 x 720): where the crosshair is, and how a pixel maps to
//! an angle from it (python/retired/review.py: `W`, `H`, `CX`, `CY`, `K`, `to_deg`, `to_px`).
//!
//! In: pixels of the 720p frame, or angles from the crosshair. Out: the other one, for every step that places a target
//! (track.rs, matching.rs, measure.rs, tracking.rs, the camera watch, the area finder, the faint cut-off), and the
//! KovOBS overlay that a review leaves out by default.

use crate::python::hypot;

/// The width in pixels of the frame the review works in.
#[expect(clippy::min_ident_chars, reason = "review.py's name, which the core's modules and tests import")]
pub const W: usize = 1280;
/// The height in pixels of the frame the review works in.
#[expect(clippy::min_ident_chars, reason = "review.py's name, which the core's modules and tests import")]
pub const H: usize = 720;

/// The x of the crosshair's center at this size, in pixels (the red dot, measured).
pub const CX: f64 = 640.03;
/// The y of the crosshair's center at this size, in pixels (the red dot, measured).
pub const CY: f64 = 359.75;

/// The focal length in pixels for a 103 degree horizontal FOV (Overwatch scale): `(W / 2) / tan(51.5 deg)`, as
/// Python computes it. A literal, so the browser's `tan` (which can differ in the last bit) cannot change it.
#[expect(clippy::min_ident_chars, reason = "review.py's name, which the core's modules import")]
pub const K: f64 = 509.0789866674102;

/// The KovOBS overlay at 1280 x 720, the areas excluded by default: session box, timer, clock and FPS, settings box,
/// gun and title, the scenario's name (any length), crosshair zoom and hand cam, version number
/// (python/retired/review.py: `OVERLAY`). Pixels: x0, y0, x1, y1.
pub const OVERLAY: [[u32; 4]; 8] = [
    [0, 0, 205, 150],
    [590, 0, 690, 60],
    [1160, 0, 1280, 100],
    [0, 620, 430, 720],
    [570, 630, 715, 720],
    [400, 675, 880, 720],
    [960, 535, 1280, 720],
    [0, 700, 60, 720],
];

/// The KovOBS overlay as shares of the frame (x0, y0, x1, y1), as python/retired/review.py's `OVERLAY_SHARES` holds
/// them (each pixel bound divided once, so the floats are the same bits).
pub fn overlay_shares() -> [[f64; 4]; OVERLAY.len()] {
    OVERLAY.map(|[x0, y0, x1, y1]| {
        [x0 as f64 / W as f64, y0 as f64 / H as f64, x1 as f64 / W as f64, y1 as f64 / H as f64]
    })
}

/// CPython's `math.degrees`: the angle times `180 / pi` rounded once, which can differ in the last bit from Rust's
/// own `to_degrees` constant.
pub fn degrees(radians: f64) -> f64 {
    radians * (180.0 / std::f64::consts::PI)
}

/// The radius in degrees of a round blob of `area_px` pixels at the crosshair.
pub fn blob_radius_deg(area_px: f64) -> f64 {
    degrees((area_px / std::f64::consts::PI).sqrt() / K)
}

/// CPython's `math.radians`.
pub fn radians(degrees: f64) -> f64 {
    degrees * (std::f64::consts::PI / 180.0)
}

/// A pixel's angle from the crosshair, in degrees: right and up are positive.
pub fn to_deg(x: f64, y: f64) -> (f64, f64) {
    (
        degrees(((x - CX) / K).atan()),
        degrees(((CY - y) / hypot(K, x - CX)).atan()),
    )
}

/// The pixel at an angle from the crosshair: the inverse of `to_deg`.
pub fn to_px(x_deg: f64, y_deg: f64) -> (f64, f64) {
    let x = CX + K * radians(x_deg).tan();
    (x, CY - radians(y_deg).tan() * hypot(K, x - CX))
}

/// Checks the mapping between pixels and degrees.
#[cfg(test)]
mod tests {
    use super::*;

    /// The crosshair's own pixel is 0 degrees both ways.
    #[test]
    fn the_crosshair_is_at_no_angle() {
        assert_eq!(to_deg(CX, CY), (0.0, 0.0));
    }

    /// A pixel taken to degrees and back lands within 1e-9 pixels of where it started.
    #[test]
    fn to_px_undoes_to_deg() {
        let (x_deg, y_deg) = to_deg(100.0, 600.0);
        let (x, y) = to_px(x_deg, y_deg);
        assert!((x - 100.0).abs() < 1e-9 && (y - 600.0).abs() < 1e-9);
    }
}
