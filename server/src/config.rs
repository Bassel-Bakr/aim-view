//! The server's settings: the command line, over a settings file (TOML), over the defaults. The defaults run on the
//! machine Aim View is made on with no flags at all: the repo's test_out/ as the data folder (python/server.py's
//! layout), KovOBS's recordings, KovaaK's folders and the models in python/model/.

use std::path::{Path, PathBuf};

use clap::{Parser, ValueEnum};
use serde::Deserialize;

/// The settings file read when --config is not given: this name in the current folder, when it is there.
pub const DEFAULT_FILE: &str = "aimview-server.toml";

/// Where the detector runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Device {
    /// The GPU when there is one (DirectML on Windows, CUDA in a build with the `cuda` feature), else the CPU
    Auto,
    /// Any GPU on Windows
    #[value(name = "directml")]
    DirectMl,
    /// An NVIDIA GPU (a build with the `cuda` feature)
    Cuda,
    /// The CPU
    Cpu,
}

impl Device {
    /// Its name on the command line and in the settings file.
    pub fn flag(self) -> String {
        self.to_possible_value().map_or_else(String::new, |v| v.get_name().to_string())
    }
}

/// Where ffmpeg comes from.
#[derive(Clone, Debug, PartialEq)]
pub enum FfmpegChoice {
    /// The PATH's
    Path,
    /// The one in this folder, or one downloaded into it
    Folder(PathBuf),
}

/// The command line. Each flag overrides the same setting in the settings file.
#[derive(Parser, Debug, Default)]
#[command(name = "aimview-server", version, about = "Aim View's review server: the UI's server mode over plain HTTP")]
pub struct Flags {
    /// The settings file (TOML) [default: aimview-server.toml in the current folder, when there is one]
    #[arg(long, value_name = "FILE")]
    pub config: Option<PathBuf>,
    /// The address to listen on. Any address but a loopback one needs --token [default: 127.0.0.1]
    #[arg(long)]
    pub host: Option<String>,
    /// The port [default: 8770]
    #[arg(long)]
    pub port: Option<u16>,
    /// The data folder: reviews, uploads and the user's marks, in python/server.py's layout [default: the repo's
    /// test_out]
    #[arg(long, value_name = "FOLDER")]
    pub data: Option<PathBuf>,
    /// The recordings, one folder per scenario; empty (--vods=) for the folder last chosen in the app [default:
    /// E:\OBS\KovOBS]
    #[arg(long, value_name = "FOLDER", value_parser = any_path)]
    pub vods: Option<PathBuf>,
    /// KovaaK's stats folder [default: FPSAimTrainer\stats in Steam's folder]
    #[arg(long, value_name = "FOLDER")]
    pub stats: Option<PathBuf>,
    /// The folders of scenario files (.sce): give the flag more than once, or several folders after it [default:
    /// KovaaK's Scenarios folder and the workshop's]
    #[arg(long, value_name = "FOLDER", num_args = 1..)]
    pub scenarios: Vec<PathBuf>,
    /// The models: the detector_<name>_u8in.onnx exports, with models.json there or in the folder above [default:
    /// the repo's python/model/exports]
    #[arg(long, value_name = "FOLDER")]
    pub models: Option<PathBuf>,
    /// Where the detector runs [default: auto]
    #[arg(long, value_enum)]
    pub device: Option<Device>,
    /// ffmpeg: "path" for the one on the PATH, or a folder: the ffmpeg in it, or one downloaded into it when it has
    /// none (BtbN's build, with the dav1d AV1 decoder) [default: ffmpeg in the data folder]
    #[arg(long, value_name = "FOLDER|path")]
    pub ffmpeg: Option<PathBuf>,
    /// The server-mode UI build (`bun run build:server`) [default: the repo's ui/dist/server/browser]
    #[arg(long, value_name = "FOLDER")]
    pub ui: Option<PathBuf>,
    /// The old page, served at /old/ until the Angular app replaces it [default: the repo's python/app]
    #[arg(long, value_name = "FOLDER")]
    pub old: Option<PathBuf>,
    /// The access token: letters, digits and - . _ ~. Prefer the settings file: a flag shows in the process list
    #[arg(long)]
    pub token: Option<String>,
}

/// A path as given, empty too (clap's own parser refuses an empty one).
fn any_path(s: &str) -> Result<PathBuf, std::convert::Infallible> {
    Ok(PathBuf::from(s))
}

/// The settings file: the same settings as the flags, each one optional. Relative paths are taken from the file's
/// folder.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileSettings {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub data: Option<PathBuf>,
    pub vods: Option<PathBuf>,
    pub stats: Option<PathBuf>,
    pub scenarios: Option<Vec<PathBuf>>,
    pub models: Option<PathBuf>,
    pub device: Option<Device>,
    pub ffmpeg: Option<PathBuf>,
    pub ui: Option<PathBuf>,
    pub old: Option<PathBuf>,
    pub token: Option<String>,
}

/// The settings the server runs with.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub host: String,
    pub port: u16,
    pub data: PathBuf,
    pub vods: Option<PathBuf>,
    pub stats: PathBuf,
    pub scenarios: Vec<PathBuf>,
    pub models: PathBuf,
    pub device: Device,
    pub ffmpeg: FfmpegChoice,
    pub ui: PathBuf,
    pub old: PathBuf,
    pub token: Option<String>,
}

/// The repo this server was built from (the defaults point into it).
pub fn repo() -> PathBuf {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    here.parent().unwrap_or(here).to_path_buf()
}

/// Steam's folder, where KovaaK's (FPSAimTrainer) is installed.
fn steam() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\Program Files (x86)\Steam")
    } else {
        std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".steam/steam")
    }
}

impl Settings {
    /// The settings with no file and no flags.
    pub fn defaults() -> Settings {
        let repo = repo();
        let steamapps = steam().join("steamapps");
        let kovaak = steamapps.join("common").join("FPSAimTrainer").join("FPSAimTrainer");
        Settings {
            host: "127.0.0.1".into(),
            port: 8770,
            data: repo.join("test_out"),
            vods: cfg!(windows).then(|| PathBuf::from(r"E:\OBS\KovOBS")),
            stats: kovaak.join("stats"),
            scenarios: vec![
                kovaak.join("Saved").join("SaveGames").join("Scenarios"),
                steamapps.join("workshop").join("content").join("824270"),
            ],
            models: repo.join("python").join("model").join("exports"),
            device: Device::Auto,
            ffmpeg: FfmpegChoice::Folder(repo.join("test_out").join("ffmpeg")),
            ui: repo.join("ui").join("dist").join("server").join("browser"),
            old: repo.join("python").join("app"),
            token: None,
        }
    }

    /// The URL to open, for the log (with the host as given, or `localhost` for an address that means "every one").
    pub fn url(&self) -> String {
        let host = match self.host.as_str() {
            "0.0.0.0" | "::" | "[::]" => "localhost",
            h if h.contains(':') && !h.starts_with('[') => return format!("http://[{h}]:{}/", self.port),
            h => h,
        };
        format!("http://{host}:{}/", self.port)
    }
}

/// A path from the settings file, taken from the file's folder when it is relative.
fn from_file(base: &Path, p: PathBuf) -> PathBuf {
    if p.is_absolute() || p.as_os_str().is_empty() { p } else { base.join(p) }
}

/// An empty path or token means "none".
fn some_path(p: PathBuf) -> Option<PathBuf> {
    (!p.as_os_str().is_empty()).then_some(p)
}

/// The settings file's text.
pub fn parse_file(text: &str) -> Result<FileSettings, String> {
    toml::from_str(text).map_err(|e| e.to_string())
}

/// The flags over the file (whose relative paths are taken from `base`, its folder) over the defaults.
pub fn resolve(flags: Flags, file: FileSettings, base: &Path, defaults: Settings) -> Result<Settings, String> {
    let path = |flag: Option<PathBuf>, file: Option<PathBuf>| flag.or_else(|| file.map(|p| from_file(base, p)));
    let scenarios = if !flags.scenarios.is_empty() {
        flags.scenarios
    } else if let Some(s) = file.scenarios {
        s.into_iter().map(|p| from_file(base, p)).collect()
    } else {
        defaults.scenarios
    };
    let token = flags.token.or(file.token).filter(|t| !t.is_empty());
    if let Some(t) = &token
        && !t.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
    {
        return Err("the token may hold only letters, digits and - . _ ~".into());
    }
    let data = path(flags.data, file.data).unwrap_or(defaults.data);
    // "path": the PATH's ffmpeg; without a choice, the data folder's (which follows --data)
    let is_path = |p: &PathBuf| p.as_os_str().eq_ignore_ascii_case("path");
    let ffmpeg = match flags.ffmpeg.or(file.ffmpeg.map(|p| if is_path(&p) { p } else { from_file(base, p) })) {
        Some(p) if is_path(&p) => FfmpegChoice::Path,
        Some(p) if !p.as_os_str().is_empty() => FfmpegChoice::Folder(p),
        _ => FfmpegChoice::Folder(data.join("ffmpeg")),
    };
    let host = flags.host.or(file.host).unwrap_or(defaults.host);
    if host.is_empty() {
        return Err("the host is empty".into());
    }
    Ok(Settings {
        host,
        port: flags.port.or(file.port).unwrap_or(defaults.port),
        data,
        vods: match path(flags.vods, file.vods) {
            Some(p) => some_path(p),
            None => defaults.vods,
        },
        stats: path(flags.stats, file.stats).unwrap_or(defaults.stats),
        scenarios,
        models: path(flags.models, file.models).unwrap_or(defaults.models),
        device: flags.device.or(file.device).unwrap_or(defaults.device),
        ffmpeg,
        ui: path(flags.ui, file.ui).unwrap_or(defaults.ui),
        old: path(flags.old, file.old).unwrap_or(defaults.old),
        token,
    })
}

/// The settings from the command line, and from the settings file it names (or aimview-server.toml in the current
/// folder). Also gives the file read, if any.
pub fn load(flags: Flags) -> Result<(Settings, Option<PathBuf>), String> {
    let path = flags.config.clone().or_else(|| Path::new(DEFAULT_FILE).is_file().then(|| PathBuf::from(DEFAULT_FILE)));
    let (file, base) = match &path {
        Some(p) => {
            let text = std::fs::read_to_string(p).map_err(|e| format!("the settings file {}: {e}", p.display()))?;
            let file = parse_file(&text).map_err(|e| format!("the settings file {}: {e}", p.display()))?;
            let p = std::path::absolute(p).unwrap_or_else(|_| p.clone());
            (file, p.parent().map(Path::to_path_buf).unwrap_or_default())
        }
        None => (FileSettings::default(), PathBuf::new()),
    };
    Ok((resolve(flags, file, &base, Settings::defaults())?, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(args: &[&str]) -> Flags {
        Flags::try_parse_from(std::iter::once("aimview-server").chain(args.iter().copied())).expect("the flags parse")
    }

    #[test]
    fn no_file_and_no_flags_give_the_defaults() {
        let s = resolve(Flags::default(), FileSettings::default(), Path::new(""), Settings::defaults()).unwrap();
        assert_eq!(s, Settings::defaults());
        assert_eq!((s.host.as_str(), s.port), ("127.0.0.1", 8770));
        assert!(s.data.ends_with("test_out"));
        assert!(s.ui.ends_with(Path::new("ui").join("dist").join("server").join("browser")));
        assert!(s.old.ends_with(Path::new("python").join("app")));
        assert_eq!(s.scenarios.len(), 2);
        assert_eq!(s.device, Device::Auto);
        assert_eq!(s.token, None);
    }

    #[test]
    fn the_file_gives_settings_and_its_relative_paths_start_at_its_folder() {
        let file = parse_file(
            r#"
            host = "0.0.0.0"
            port = 9000
            data = "aim-data"
            vods = 'E:\OBS\KovOBS'
            scenarios = ["a", "sub/b"]
            device = "directml"
            token = "abc-123"
            ffmpeg = "tools"
            old = "old-page"
            "#,
        )
        .unwrap();
        let base = std::path::absolute("base").unwrap();
        let s = resolve(Flags::default(), file, &base, Settings::defaults()).unwrap();
        assert_eq!((s.host.as_str(), s.port), ("0.0.0.0", 9000));
        assert_eq!(s.data, base.join("aim-data"));
        assert_eq!(s.vods, Some(PathBuf::from(r"E:\OBS\KovOBS")));
        assert_eq!(s.scenarios, vec![base.join("a"), base.join("sub/b")]);
        assert_eq!(s.device, Device::DirectMl);
        assert_eq!(s.token.as_deref(), Some("abc-123"));
        assert_eq!(s.ffmpeg, FfmpegChoice::Folder(base.join("tools")));
        assert_eq!(s.old, base.join("old-page"));
        // the settings the file leaves out stay the defaults
        assert_eq!(s.stats, Settings::defaults().stats);
    }

    #[test]
    fn flags_override_the_file() {
        let file = parse_file("port = 9000\ndevice = \"cpu\"\nscenarios = [\"x\"]\ntoken = \"from-file\"").unwrap();
        let f = flags(&[
            "--port", "8775", "--device", "cuda", "--scenarios", "s1", "s2", "--scenarios", "s3", "--token", "flag",
            "--vods=", "--ffmpeg", "path",
        ]);
        let s = resolve(f, file, Path::new("base"), Settings::defaults()).unwrap();
        assert_eq!(s.port, 8775);
        assert_eq!(s.device, Device::Cuda);
        assert_eq!(s.scenarios, ["s1", "s2", "s3"].map(PathBuf::from).to_vec());
        assert_eq!(s.token.as_deref(), Some("flag"));
        // "" for the VODs folder: none
        assert_eq!(s.vods, None);
        assert_eq!(s.ffmpeg, FfmpegChoice::Path);
        // ffmpeg follows the data folder
        let s = resolve(flags(&["--data", "d"]), FileSettings::default(), Path::new(""), Settings::defaults()).unwrap();
        assert_eq!(s.ffmpeg, FfmpegChoice::Folder(Path::new("d").join("ffmpeg")));
        let file = parse_file("ffmpeg = \"PATH\"").unwrap();
        let s = resolve(Flags::default(), file, Path::new("base"), Settings::defaults()).unwrap();
        assert_eq!(s.ffmpeg, FfmpegChoice::Path);
    }

    #[test]
    fn every_device_name_parses() {
        for (name, device) in
            [("auto", Device::Auto), ("directml", Device::DirectMl), ("cuda", Device::Cuda), ("cpu", Device::Cpu)]
        {
            assert_eq!(flags(&["--device", name]).device, Some(device));
            assert_eq!(parse_file(&format!("device = \"{name}\"")).unwrap().device, Some(device));
            assert_eq!(device.flag(), name);
        }
        assert!(Flags::try_parse_from(["aimview-server", "--device", "gpu"]).is_err());
    }

    #[test]
    fn a_bad_file_or_token_is_refused() {
        assert!(parse_file("prot = 8770").is_err(), "an unknown setting");
        assert!(parse_file("port = \"eighty\"").is_err());
        let bad = flags(&["--token", "has space"]);
        assert!(resolve(bad, FileSettings::default(), Path::new(""), Settings::defaults()).is_err());
        // an empty token is no token
        let empty = flags(&["--token", ""]);
        let s = resolve(empty, FileSettings::default(), Path::new(""), Settings::defaults()).unwrap();
        assert_eq!(s.token, None);
    }

    #[test]
    fn the_url_to_open() {
        let mut s = Settings::defaults();
        assert_eq!(s.url(), "http://127.0.0.1:8770/");
        s.host = "0.0.0.0".into();
        assert_eq!(s.url(), "http://localhost:8770/");
        s.host = "::1".into();
        assert_eq!(s.url(), "http://[::1]:8770/");
    }
}
