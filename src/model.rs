//! The detector model's settings file (python/model/MODEL_FILE.md): `detector_<name>.json` beside the model's exports,
//! so a retrained model needs no change to the code. Format 1: the score a cell must pass to be a target, and the map
//! that puts the model's scores on the reference model's scale. A model with no file gets today's values
//! (`ModelSettings::default`).

use serde::{Deserialize, Serialize};

/// The settings file's format this core reads.
pub const FORMAT: u32 = 1;
/// The threshold of a model with no settings file (python/model/infer.py: THRESHOLD, chosen on the val split).
pub const DEFAULT_THRESHOLD: f32 = 0.3;
/// The model whose score scale every other model's scores are mapped onto.
pub const REFERENCE: &str = "full_v3";
/// The exports a settings file sits beside (python/model/export.py): detector_<name><suffix>.onnx.
const EXPORTS: [&str; 5] = ["_u8in", "_embed", "_fp32", "_fp16", "_int8"];

/// A model's settings. `threshold` is on the reference model's scale: a cell is a target when its score, mapped by
/// `score_map`, is over it. `score_map` holds [raw, mapped] points, rising in both: the model's raw score is mapped
/// onto the reference model's scale between them (linearly, held at the end points' values outside them); None maps
/// nothing. Every score the review keeps (the tracks' `s`, which the faint cut-off reads) is the mapped one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelSettings {
    pub format: u32,
    pub name: String,
    pub threshold: f32,
    #[serde(default)]
    pub score_map: Option<Vec<[f64; 2]>>,
    pub reference: String,
}

impl Default for ModelSettings {
    /// Today's values, for a model with no settings file.
    fn default() -> ModelSettings {
        ModelSettings {
            format: FORMAT,
            name: String::new(),
            threshold: DEFAULT_THRESHOLD,
            score_map: None,
            reference: REFERENCE.into(),
        }
    }
}

impl ModelSettings {
    /// A settings file's text, checked: its format, a threshold from 0 to 1, and a map of 2 or more points from 0 to 1
    /// that rise in both values.
    pub fn from_json(text: &str) -> Result<ModelSettings, String> {
        let s: ModelSettings = serde_json::from_str(text).map_err(|e| format!("not a model settings file: {e}"))?;
        if s.format != FORMAT {
            return Err(format!("format {}: this version reads format {FORMAT}", s.format));
        }
        if !(0.0..=1.0).contains(&s.threshold) {
            return Err(format!("the threshold {} is not from 0 to 1", s.threshold));
        }
        if let Some(points) = &s.score_map {
            if points.len() < 2 {
                return Err("the score map has fewer than 2 points".into());
            }
            if points.iter().flatten().any(|v| !(0.0..=1.0).contains(v)) {
                return Err("the score map has a value that is not from 0 to 1".into());
            }
            if points.windows(2).any(|p| p[1][0] <= p[0][0] || p[1][1] <= p[0][1]) {
                return Err("the score map's points do not rise in both values".into());
            }
        }
        Ok(s)
    }

    /// The model's raw score on the reference model's scale: as NumPy's `interp` gives it over the map's points (in
    /// float64; the end points' values outside them), stored as float32 as the boxes' scores are.
    pub fn mapped(&self, raw: f32) -> f32 {
        let Some(p) = &self.score_map else { return raw };
        let x = raw as f64;
        let last = p.len() - 1;
        if x.is_nan() {
            return raw;
        }
        if x < p[0][0] {
            return p[0][1] as f32;
        }
        if x >= p[last][0] {
            return p[last][1] as f32;
        }
        // the segment that holds x: p[j][0] <= x < p[j + 1][0]
        let j = p.partition_point(|q| q[0] <= x) - 1;
        if x == p[j][0] {
            return p[j][1] as f32;
        }
        let slope = (p[j + 1][1] - p[j][1]) / (p[j + 1][0] - p[j][0]);
        (slope * (x - p[j][0]) + p[j][1]) as f32
    }

    /// A cell's score on the reference model's scale, when it passes the threshold.
    pub fn passes(&self, raw: f32) -> Option<f32> {
        let s = self.mapped(raw);
        (s > self.threshold).then_some(s)
    }

    /// The raw score at or under which no cell passes, so a decoder can skip those cells without mapping them (most
    /// cells are far under it). Without a map it is the threshold itself. With one, the last point whose mapped value
    /// is under the threshold (by more than float64's rounding, so a value mapped between the points before it cannot
    /// round up past it); -infinity when there is none.
    pub fn floor(&self) -> f64 {
        let Some(p) = &self.score_map else { return self.threshold as f64 };
        let under = self.threshold as f64 - 1e-9;
        p.iter().rev().find(|q| q[1] < under).map_or(f64::NEG_INFINITY, |q| q[0])
    }
}

/// The settings file's name for a model export's file name: detector_full_v3.json for detector_full_v3_u8in.onnx (and
/// the other exports). None for a file that is not an ONNX export.
pub fn settings_file(export: &str) -> Option<String> {
    let stem = export.strip_suffix(".onnx")?;
    let name = EXPORTS.iter().find_map(|s| stem.strip_suffix(s)).unwrap_or(stem);
    Some(format!("{name}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_map(threshold: f32, points: &[[f64; 2]]) -> ModelSettings {
        ModelSettings { threshold, score_map: Some(points.to_vec()), ..ModelSettings::default() }
    }

    /// The file as it is written for today's models reads as today's values: the threshold exactly the float32 0.3.
    #[test]
    fn a_file_with_todays_values_reads_as_the_default() {
        let text = r#"{"format": 1, "name": "full_v3", "threshold": 0.3, "score_map": null, "reference": "full_v3"}"#;
        let s = ModelSettings::from_json(text).unwrap();
        assert_eq!(s, ModelSettings { name: "full_v3".into(), ..ModelSettings::default() });
        assert_eq!(s.threshold, 0.3f32);
        assert_eq!(s.mapped(0.123_456_79), 0.123_456_79);
    }

    #[test]
    fn bad_files_are_refused() {
        let file = |rest: &str| format!(r#"{{"name": "x", "reference": "full_v3", {rest}}}"#);
        assert!(ModelSettings::from_json(&file(r#""format": 2, "threshold": 0.3"#)).is_err());
        assert!(ModelSettings::from_json(&file(r#""format": 1"#)).is_err());
        assert!(ModelSettings::from_json(&file(r#""format": 1, "threshold": 1.5"#)).is_err());
        assert!(ModelSettings::from_json(&file(r#""format": 1, "threshold": 0.3, "score_map": [[0, 0]]"#)).is_err());
        let falls = r#""format": 1, "threshold": 0.3, "score_map": [[0, 0], [0.5, 0.6], [1, 0.5]]"#;
        assert!(ModelSettings::from_json(&file(falls)).is_err());
        let fine = r#""format": 1, "threshold": 0.3, "score_map": [[0, 0], [0.5, 0.6], [1, 1]], "more": 1"#;
        assert!(ModelSettings::from_json(&file(fine)).is_ok());
    }

    /// The map between its points, at them, and held at the ends, as np.interp gives it.
    #[test]
    fn the_map_is_numpys_interp() {
        let s = with_map(0.3, &[[0.125, 0.0], [0.5, 0.25], [0.875, 1.0]]);
        assert_eq!(s.mapped(0.05), 0.0);
        assert_eq!(s.mapped(0.95), 1.0);
        assert_eq!(s.mapped(0.875), 1.0);
        assert_eq!(s.mapped(0.5), 0.25);
        let x = 0.3f32 as f64;
        assert_eq!(s.mapped(0.3), ((0.25 - 0.0) / (0.5 - 0.125) * (x - 0.125) + 0.0) as f32);
        let x = 0.7f32 as f64;
        assert_eq!(s.mapped(0.7), ((1.0 - 0.25) / (0.875 - 0.5) * (x - 0.5) + 0.25) as f32);
    }

    /// Skipping the cells at or under the floor changes nothing: none of them passes.
    #[test]
    fn no_cell_under_the_floor_passes() {
        let maps = [
            None,
            Some(vec![[0.0, 0.0], [0.5, 0.25], [1.0, 1.0]]),
            Some(vec![[0.2, 0.1], [0.4, 0.3], [0.6, 0.31], [1.0, 0.9]]),
            Some(vec![[0.0, 0.5], [1.0, 0.9]]),
        ];
        for map in maps {
            let s = ModelSettings { score_map: map, ..ModelSettings::default() };
            let floor = s.floor();
            for k in 0..=100_000 {
                let raw = k as f32 / 100_000.0;
                if raw as f64 <= floor {
                    assert_eq!(s.passes(raw), None, "{raw} with {:?}", s.score_map);
                }
            }
        }
    }

    #[test]
    fn the_settings_file_sits_beside_every_export() {
        assert_eq!(settings_file("detector_full_v3_u8in.onnx").as_deref(), Some("detector_full_v3.json"));
        assert_eq!(settings_file("detector_small_v13_embed.onnx").as_deref(), Some("detector_small_v13.json"));
        assert_eq!(settings_file("mine.onnx").as_deref(), Some("mine.json"));
        assert_eq!(settings_file("detector_full_v3.pt"), None);
    }
}
