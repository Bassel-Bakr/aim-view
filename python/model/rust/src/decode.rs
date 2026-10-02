//! Turns the model's two outputs into detections: (cx, cy, w, h, score) in input pixels.

pub const THRESHOLD: f32 = 0.3;

/// `score` is [H/4 * W/4], `reg` is [4, H/4, W/4] flat, both row-major.
pub fn decode(score: &[f32], reg: &[f32], gh: usize, gw: usize) -> Vec<[f32; 5]> {
    let plane = gh * gw;
    let mut out = Vec::new();
    for i in 0..gh {
        for j in 0..gw {
            let k = i * gw + j;
            let s = score[k];
            if s > THRESHOLD {
                out.push([
                    (j as f32 + reg[k]) * 4.0,
                    (i as f32 + reg[plane + k]) * 4.0,
                    reg[2 * plane + k].exp(),
                    reg[3 * plane + k].exp(),
                    s,
                ]);
            }
        }
    }
    out
}
