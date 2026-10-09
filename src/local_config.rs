//! Aim View's settings for a checkout of the repo and this computer: aimview.defaults.json (in git, built in here:
//! the project's own layout and where KovaaK keeps its folders under Steam's) under aimview.json (beside it at the
//! repo's root, out of git, optional: this computer's own, such as the recordings' folder). No folder is written in
//! the code: the review server's defaults, the examples and the tests read them here (python/local_config.py reads the
//! same files for the scripts). Steam's folder, unless named, is where Steam records it: the registry on Windows,
//! else under the home folder. In: the two files. Out: folders, and the server's address.

use std::path::{MAIN_SEPARATOR_STR, Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// The committed defaults, as built.
const DEFAULTS: &str = include_str!("../aimview.defaults.json");
/// This computer's file, beside the defaults at the repo's root.
const LOCAL_FILE: &str = "aimview.json";

/// The settings: the defaults with this computer's file over them, and the folder their relative paths start at.
pub struct LocalConfig {
    /// The folder relative paths start at: the repo's root, where aimview.json is looked for.
    root: PathBuf,
    /// The settings: the defaults' top-level keys, each replaced by this computer's file where it has the key.
    value: Value,
}

impl LocalConfig {
    /// The settings of the repo this was built from (its aimview.json read now, when there is one).
    pub fn load() -> LocalConfig {
        LocalConfig::at(Path::new(env!("CARGO_MANIFEST_DIR")))
    }

    /// The settings with `root`'s aimview.json over the defaults; relative paths start at `root`.
    pub fn at(root: &Path) -> LocalConfig {
        let mut value: Value = serde_json::from_str(DEFAULTS).expect("aimview.defaults.json is JSON");
        let local = std::fs::read_to_string(root.join(LOCAL_FILE)).ok();
        match local.as_deref().map(serde_json::from_str::<Value>) {
            Some(Ok(Value::Object(over))) => {
                for (key, item) in over {
                    value[key] = item;
                }
            }
            Some(_) => {
                eprintln!("{} is not a JSON object: its settings are left out", root.join(LOCAL_FILE).display());
            }
            None => {}
        }
        LocalConfig { root: root.to_path_buf(), value }
    }

    /// A folder the settings name (data, models, ui, ffmpeg, vods): relative ones from the repo's root; None when the
    /// key is null, missing or not a string.
    pub fn folder(&self, key: &str) -> Option<PathBuf> {
        self.value[key].as_str().map(|path| self.root.join(native(path)))
    }

    /// The server's default address and port; an empty host or port 0 when the settings lack them.
    pub fn server(&self) -> (String, u16) {
        let server = &self.value["server"];
        let host = server["host"].as_str().unwrap_or_default().to_string();
        (host, server["port"].as_u64().and_then(|port| u16::try_from(port).ok()).unwrap_or_default())
    }

    /// A text setting by its JSON pointer into the settings ("/downloads/ytdlp_releases"); None when it is missing or
    /// not a string.
    pub fn text(&self, pointer: &str) -> Option<&str> {
        self.value.pointer(pointer)?.as_str()
    }

    /// Steam's folder: as named, else where Steam records it; None when neither has one.
    pub fn steam(&self) -> Option<PathBuf> {
        if let Some(named) = self.folder("steam") {
            return Some(named);
        }
        let found = &self.value["steam_found"];
        if cfg!(windows) {
            let key = found["registry_key"].as_str()?;
            let name = found["registry_value"].as_str()?;
            return registry_folder(key, name);
        }
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        Some(home.join(found["under_home"].as_str()?)).filter(|folder| folder.is_dir())
    }

    /// One of KovaaK's folders (game, workshop, stats, scenarios, crosshairs): game and workshop under Steam's, the
    /// others under the game's. None without Steam's folder.
    pub fn kovaak(&self, name: &str) -> Option<PathBuf> {
        let kovaak = &self.value["kovaak"];
        let steam = self.steam()?;
        let under_steam = |key: &str| kovaak[key].as_str().map(|path| steam.join(native(path)));
        match name {
            "game" | "workshop" => under_steam(name),
            other => Some(under_steam("game")?.join(native(kovaak[other].as_str()?))),
        }
    }

    /// KovaaK's scenario folders: the user's own, then the workshop's (one folder per scenario).
    pub fn kovaak_scenarios(&self) -> Vec<PathBuf> {
        ["scenarios", "workshop"].iter().filter_map(|name| self.kovaak(name)).collect()
    }
}

/// A folder a registry value holds (`reg query`, so no Windows library is needed here).
fn registry_folder(key: &str, name: &str) -> Option<PathBuf> {
    let output = Command::new("reg").args(["query", key, "/v", name]).output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().find(|line| line.trim_start().starts_with(name))?;
    let folder = line.split("REG_SZ").nth(1)?.trim();
    (!folder.is_empty()).then(|| native(folder))
}

/// A path as the settings write it ("/" between names, as JSON keeps it plainly), with this system's separator.
fn native(path: &str) -> PathBuf {
    PathBuf::from(path.replace('/', MAIN_SEPARATOR_STR))
}

/// Checks how this computer's file and the defaults combine.
#[cfg(test)]
mod tests {
    use super::*;

    /// A computer's file overrides a default and leaves the rest; relative paths start at the root.
    #[test]
    fn the_computers_file_over_the_defaults() {
        let root = std::env::temp_dir().join(format!("aimview-config-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(LOCAL_FILE), r#"{"vods": "recordings", "steam": "steam"}"#).unwrap();
        let config = LocalConfig::at(&root);
        assert_eq!(config.folder("vods"), Some(root.join("recordings")));
        assert_eq!(config.folder("data"), Some(root.join("test_out")));
        let game = ["steam", "steamapps", "common", "FPSAimTrainer", "FPSAimTrainer"].iter().collect::<PathBuf>();
        assert_eq!(config.kovaak("stats"), Some(root.join(game).join("stats")));
        std::fs::remove_dir_all(root).unwrap();
        assert_eq!(LocalConfig::at(Path::new("no such folder")).folder("vods"), None);
    }

    /// A text setting is found by its pointer; a missing one, or one that is not text, is None.
    #[test]
    fn text_settings_by_pointer() {
        let config = LocalConfig::at(Path::new("no such folder"));
        assert_eq!(config.text("/server/host"), Some("127.0.0.1"));
        assert_eq!(config.text("/server/port"), None, "a number");
        assert_eq!(config.text("/no/such/setting"), None);
    }
}
