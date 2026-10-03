//! What the user set, kept in settings.json: the VODs folder they chose in the app (`vods`) and the model new reviews
//! use (`model`). Other keys in the file are kept as they are (the desktop app also kept KovaaK's folder as `kovaak`;
//! python/server.py keeps only `model`). And the models to pick from.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use super::{Answer, Failure, Library, read_json, write_json};

pub(super) const FILE: &str = "settings.json";
/// The model new reviews use until the user picks one (infer.BEST).
pub const BEST: &str = "full_v3";

/// settings.json's keys and values.
#[derive(Clone, Default)]
pub(super) struct Settings(Map<String, Value>);

impl Settings {
    /// The settings in `p`; none when it is missing or not a JSON object.
    pub(super) fn read(p: &Path) -> Settings {
        Settings(read_json(p).unwrap_or_default())
    }
}

impl Library {
    fn settings(&self) -> Settings {
        self.settings.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn save_settings(&self, key: &str, value: Value) -> Answer<()> {
        let mut s = self.settings.lock().map_err(|_| "the settings are broken".to_string())?;
        s.0.insert(key.into(), value);
        write_json(&self.file(FILE), &s.0)
    }

    /// The VODs folder: the one the configuration gives, else the one the user chose in the app.
    pub fn vods(&self) -> Option<PathBuf> {
        self.config.vods.clone().or_else(|| self.settings().0.get("vods").and_then(Value::as_str).map(PathBuf::from))
    }

    /// The VODs folder the user chose.
    pub fn set_vods(&self, folder: PathBuf) -> Answer<Value> {
        self.save_settings("vods", json!(folder))?;
        Ok(json!({ "folder": folder }))
    }

    /// The model new reviews use: the user's pick, else full_v3 (also when the pick's export is not here: a pick
    /// python/server.py kept can be a model only it runs).
    pub fn model(&self) -> String {
        let picked = self.settings().0.get("model").and_then(Value::as_str).map(str::to_string);
        picked.filter(|m| self.model_file(m).is_file()).unwrap_or_else(|| BEST.into())
    }

    /// The detector export of a model.
    pub fn model_file(&self, name: &str) -> PathBuf {
        self.config.models.join(format!("detector_{name}_u8in.onnx"))
    }

    /// models.json: in the models folder, else in the folder above it (python/model).
    fn models_info(&self) -> Option<Value> {
        let here = self.config.models.join("models.json");
        read_json(&here).or_else(|| read_json(&self.config.models.parent()?.join("models.json")))
    }

    /// The models to pick from: the ones models.json describes whose exports are here (python/server.py: models).
    pub fn models(&self) -> Answer<Value> {
        let info: Value = self.models_info().ok_or("models.json is missing".to_string())?;
        let mut models = Vec::new();
        for (name, m) in info["models"].as_object().into_iter().flatten() {
            if !self.model_file(name).is_file() {
                continue;
            }
            let mut m = m.clone();
            m["name"] = json!(name);
            if m.get("label").is_none() {
                m["label"] = json!(name);
            }
            m["default"] = json!(name == BEST);
            m["available"] = json!(true);
            models.push(m);
        }
        Ok(json!({
            "chosen": self.model(), "device": self.config.device.name(), "speed": info["speed"], "checks": info["checks"],
            "checked_on": info["checked_on"], "models": models,
        }))
    }

    /// The model new reviews use, kept for the next start.
    pub fn pick(&self, name: &str) -> Answer<Value> {
        if !self.model_file(name).is_file() {
            return Err(Failure::bad(format!("no model called {name} ships with the app")));
        }
        self.save_settings("model", json!(name))?;
        self.models()
    }
}
