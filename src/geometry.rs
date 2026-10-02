//! The frame's geometry at the size the review works in (1280 x 720): where the crosshair is, and how a pixel maps to
//! an angle from it (python/review.py: `W`, `H`, `CX`, `CY`, `K`, `to_deg`, `to_px`).

use crate::python::hypot;

/// The frame the review works in, in pixels.
pub const W: usize = 1280;
pub const H: usize = 720;

/// The crosshair's center at this size (the red dot, measured).
pub const CX: f64 = 640.03;
pub const CY: f64 = 359.75;

/// The focal length in pixels for a 103 degree horizontal FOV (Overwatch scale): `(W / 2) / tan(51.5 deg)`, as
/// Python computes it. A literal, so the browser's `tan` (which can differ in the last bit) cannot change it.
pub const K: f64 = 509.0789866674102;

/// The KovOBS overlay at 1280 x 720, the areas excluded by default: session box, timer, clock and FPS, settings box,
/// gun and title, the scenario's name (any length), crosshair zoom and hand cam, version number (python/review.py:
/// `OVERLAY`). Pixels: x0, y0, x1, y1.
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

/// The KovOBS overlay as shares of the frame, as `OVERLAY_SHARES` holds them (each pixel bound divided once).
pub fn overlay_shares() -> Vec<[f64; 4]> {
    OVERLAY
        .iter()
        .map(|&[x0, y0, x1, y1]| {
            [x0 as f64 / W as f64, y0 as f64 / H as f64, x1 as f64 / W as f64, y1 as f64 / H as f64]
        })
        .collect()
}

/// CPython's `math.degrees`: the angle times `180 / pi` rounded once, which can differ in the last bit from Rust's
/// own `to_degrees` constant.
pub fn degrees(radians: f64) -> f64 {
    radians * (180.0 / std::f64::consts::PI)
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
pub fn to_px(xd: f64, yd: f64) -> (f64, f64) {
    let x = CX + K * radians(xd).tan();
    (x, CY - radians(yd).tan() * hypot(K, x - CX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_crosshair_is_at_no_angle() {
        assert_eq!(to_deg(CX, CY), (0.0, 0.0));
    }

    #[test]
    fn to_px_undoes_to_deg() {
        let (xd, yd) = to_deg(100.0, 600.0);
        let (x, y) = to_px(xd, yd);
        assert!((x - 100.0).abs() < 1e-9 && (y - 600.0).abs() < 1e-9);
    }
}
