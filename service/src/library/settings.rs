//! What the user set, kept in settings.json: the VODs folder they chose in the app (`vods`), the model new reviews
//! use (`model`), the device the detector runs on (`device`) and the frames it takes at once on each device (`batch`,
//! by device name). Other keys in the file are kept as they are (the desktop app also kept KovaaK's folder as `kovaak`;
//! python/retired/server.py kept only `model`). And the models to pick from (models.json and the exports in the models
//! folder). In: /api/model, /api/device, /api/batch and the app's folder dialog. Out: settings.json, /api/models'
//! answer, and the model, device and frames at once new reviews use (reviews.rs).

use std::path::PathBuf;

use serde_json::{Map, Value, json};

use super::{Answer, Failure, Library, keep_json, read_json, read_kept};
use crate::config::Device;
use crate::store::{Item, Store};

/// The model new reviews use until the user picks one when models.json names no default (its "default": infer.BEST).
pub const BEST: &str = "full_v3";
/// The frames the detector can take at once (the browser offers the same).
pub const BATCHES: [usize; 4] = [1, 2, 4, 8];
/// The frames at once until the user picks (av1: 4 was the fastest on the GPU).
const DEFAULT_BATCH: usize = 4;

/// settings.json's keys and values.
#[derive(Clone, Default)]
pub(super) struct Settings(Map<String, Value>);

impl Settings {
    /// The settings kept in `store`; none when they are missing or not a JSON object.
    pub(super) fn read(store: &dyn Store) -> Settings {
        Settings(read_kept(store, Item::Settings).unwrap_or_default())
    }
}

impl Library {
    /// A copy of the settings; none when their lock is broken.
    fn settings(&self) -> Settings {
        self.settings.lock().map(|settings| settings.clone()).unwrap_or_default()
    }

    /// Sets `key` to `value` and writes every setting to settings.json.
    fn save_settings(&self, key: &str, value: Value) -> Answer<()> {
        let mut settings = self.settings.lock().map_err(|_| "the settings are broken".to_string())?;
        settings.0.insert(key.into(), value);
        keep_json(self.store(), Item::Settings, &settings.0)
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

    /// The model new reviews use: the user's pick, else the default (`default_model`; also when the pick's export is
    /// not here: a pick python/retired/server.py kept can be a model only it ran).
    pub fn model(&self) -> String {
        let picked = self.settings().0.get("model").and_then(Value::as_str).map(str::to_string);
        picked.filter(|name| crate::disk::is_file(self.model_file(name))).unwrap_or_else(|| self.default_model())
    }

    /// The device new reviews run the detector on: the user's pick when this build has it, else the configuration's.
    pub fn device(&self) -> Device {
        let picked = self.settings().0.get("device").and_then(Value::as_str).and_then(Device::from_name);
        picked.filter(|device| Device::built().contains(device)).unwrap_or(self.config.device)
    }

    /// The frames new reviews give the detector at once on `device`: the user's pick for it, else DEFAULT_BATCH.
    pub fn batch(&self, device: Device) -> usize {
        let settings = self.settings();
        let picked = settings.0.get("batch").and_then(|batches| batches.get(device.name())).and_then(Value::as_u64);
        picked.map(|frames| frames as usize).filter(|frames| BATCHES.contains(frames)).unwrap_or(DEFAULT_BATCH)
    }

    /// The device new reviews use, kept for the next start.
    pub fn use_device(&self, name: &str) -> Answer<Value> {
        let device = Device::from_name(name).filter(|device| Device::built().contains(device));
        let device = device.ok_or_else(|| Failure::bad(format!("the detector cannot run on {name} here")))?;
        self.save_settings("device", json!(device.name()))?;
        self.models()
    }

    /// The frames at once on the device in use, kept for each device.
    pub fn use_batch(&self, frames: &str) -> Answer<Value> {
        let batch = frames.parse::<usize>().ok().filter(|batch| BATCHES.contains(batch));
        let batch = batch.ok_or_else(|| Failure::bad(format!("{frames} frames at once is not a choice")))?;
        let mut all = self.settings().0.get("batch").cloned().unwrap_or_else(|| json!({}));
        all[self.device().name()] = json!(batch);
        self.save_settings("batch", all)?;
        self.models()
    }

    /// The model models.json names as the default ("default"), else BEST: a new model becomes the default by a change
    /// to that file, not to code.
    pub fn default_model(&self) -> String {
        let named = self.models_info().and_then(|info| info["default"].as_str().map(str::to_string));
        named.filter(|name| crate::disk::is_file(self.model_file(name))).unwrap_or_else(|| BEST.into())
    }

    /// The detector export of a model.
    pub fn model_file(&self, name: &str) -> PathBuf {
        self.config.models.join(format!("detector_{name}_u8in.onnx"))
    }

    /// models.json: in the models folder, else in the folder above it (python/model).
    pub(super) fn models_info(&self) -> Option<Value> {
        let here = self.config.models.join("models.json");
        read_json(&here).or_else(|| read_json(&self.config.models.parent()?.join("models.json")))
    }

    /// The models to pick from: the ones models.json describes whose exports are here, with the chosen model, the
    /// device and the frames at once and their choices (python/retired/server.py: models). An error without
    /// models.json.
    pub fn models(&self) -> Answer<Value> {
        let info: Value = self.models_info().ok_or("models.json is missing".to_string())?;
        let mut models = Vec::new();
        let default = self.default_model();
        for (name, described) in info["models"].as_object().into_iter().flatten() {
            if !crate::disk::is_file(self.model_file(name)) {
                continue;
            }
            let mut model = described.clone();
            model["name"] = json!(name);
            if model.get("label").is_none() {
                model["label"] = json!(name);
            }
            model["default"] = json!(*name == default);
            model["available"] = json!(true);
            models.push(model);
        }
        let device = self.device();
        let devices: Vec<&str> = Device::built().into_iter().map(Device::name).collect();
        Ok(json!({
            "chosen": self.model(), "device": device.name(), "devices": devices, "batch": self.batch(device),
            "batches": BATCHES, "speed": info["speed"], "checks": info["checks"], "checked_on": info["checked_on"],
            "models": models,
        }))
    }

    /// The model new reviews use, kept for the next start.
    pub fn pick(&self, name: &str) -> Answer<Value> {
        if !crate::disk::is_file(self.model_file(name)) {
            return Err(Failure::bad(format!("no model called {name} ships with the app")));
        }
        self.save_settings("model", json!(name))?;
        self.models()
    }
}

/// The user's picks of device and frames at once.
#[cfg(test)]
mod tests {
    use crate::config::{Config, Device, Layout};
    use crate::library::Library;

    /// The device and the frames at once are the user's picks, kept in settings.json across a restart, the frames for
    /// each device; a device this build cannot run, or frames not offered, are refused and change nothing.
    #[test]
    fn the_device_and_frames_at_once_are_kept_for_each_device() {
        let dir = std::env::temp_dir().join(format!("aimview-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("models")).unwrap();
        std::fs::write(dir.join("models").join("models.json"), r#"{"models": {}}"#).unwrap();
        let open = || Library::open(Config::new(dir.clone(), Layout::App, dir.join("models"))).unwrap();
        let lib = open();
        let gpu = Device::built()[0];
        assert_eq!(lib.batch(lib.device()), 4);
        lib.use_device(gpu.name()).unwrap();
        lib.use_batch("8").unwrap();
        lib.use_device("cpu").unwrap();
        lib.use_batch("1").unwrap();
        assert!(lib.use_device("tpu").is_err());
        assert!(lib.use_batch("3").is_err());
        let again = open();
        assert_eq!(again.device(), Device::Cpu);
        assert_eq!(again.batch(Device::Cpu), 1);
        assert_eq!(again.batch(gpu), if gpu == Device::Cpu { 1 } else { 8 });
        let _ = std::fs::remove_dir_all(&dir);
    }
}
