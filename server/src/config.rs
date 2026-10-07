//! The server's settings: the command line, over a settings file (TOML), over the defaults. The defaults are the
//! repo's settings (aimview.defaults.json under this computer's aimview.json: aimview::local_config): the repo's
//! test_out/ as the data folder (python/server.py's layout), the recordings' folder this computer names, KovaaK's
//! folders under Steam's, and the models in python/model/.
//!
//! In: the flags (clap) and the settings file's text. Out: the `Settings` main.rs runs with, which glue.rs turns into
//! the review service's `Config`.

use std::path::{Path, PathBuf};

use aimview::local_config::LocalConfig;
use clap::{Parser, ValueEnum};
use serde::Deserialize;

/// The settings file read when --config is not given: this name in the current folder, when it is there.
pub const DEFAULT_FILE: &str = "aimview-server.toml";
/// The ffmpeg setting that means "the PATH's ffmpeg only", in any case.
const FFMPEG_FROM_PATH: &str = "path";
/// The characters a token may hold besides ASCII letters and digits (URL-safe without percent-encoding).
const TOKEN_SYMBOLS: &[u8] = b"-._~";

/// A setting turned on or off on the command line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Switch {
    On,
    Off,
}

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
        self.to_possible_value().map_or_else(String::new, |value| value.get_name().to_string())
    }
}

/// Where ffmpeg comes from.
#[derive(Clone, Debug, PartialEq)]
pub enum FfmpegChoice {
    /// The PATH's
    Path,
    /// The one in this folder (named by the user); when it has none, as `Auto`
    Folder(PathBuf),
    /// None named, as KovOBS finds it: the PATH's when it has ffmpeg and ffprobe, else the one in this folder (the data
    /// folder's), downloaded into it before the first review
    Auto(PathBuf),
}

/// The command line. Each flag overrides the same setting in the settings file.
#[derive(Parser, Debug, Default)]
#[command(name = "aimview-server", version, about = "Aim View's review server: the UI's server mode over plain HTTP")]
pub struct Flags {
    /// The settings file (TOML) [default: aimview-server.toml in the current folder, when there is one]
    #[arg(long, value_name = "FILE")]
    pub config: Option<PathBuf>,
    /// The address to listen on. Any address but a loopback one needs --token [default: aimview.json's server.host]
    #[arg(long)]
    pub host: Option<String>,
    /// The port [default: aimview.json's server.port]
    #[arg(long)]
    pub port: Option<u16>,
    /// The data folder: reviews, uploads and the user's marks, in python/server.py's layout [default: aimview.json's
    /// data, the repo's test_out]
    #[arg(long, value_name = "FOLDER")]
    pub data: Option<PathBuf>,
    /// The recordings, one folder per scenario; empty (--vods=) for the folder last chosen in the app [default:
    /// aimview.json's vods]
    #[arg(long, value_name = "FOLDER", value_parser = any_path)]
    pub vods: Option<PathBuf>,
    /// KovaaK's stats folder [default: KovaaK's in Steam's folder, as aimview.json says where]
    #[arg(long, value_name = "FOLDER")]
    pub stats: Option<PathBuf>,
    /// The folders of scenario files (.sce): give the flag more than once, or several folders after it [default:
    /// KovaaK's Scenarios folder and the workshop's]
    #[arg(long, value_name = "FOLDER", num_args = 1..)]
    pub scenarios: Vec<PathBuf>,
    /// The models: the detector_<name>_u8in.onnx exports, with models.json there or in the folder above [default:
    /// aimview.json's models, the repo's python/model/exports]
    #[arg(long, value_name = "FOLDER")]
    pub models: Option<PathBuf>,
    /// Where the detector runs [default: auto]
    #[arg(long, value_enum)]
    pub device: Option<Device>,
    /// Decode and convert the frames on the GPU where the video allows it (Windows, 2560 x 1440 AV1 or H.264 MP4s;
    /// the reviews are the same, byte for byte, with a third to a half of the CPU) [default: on]
    #[arg(long, value_enum)]
    pub gpu_frames: Option<Switch>,
    /// ffmpeg: "path" for the one on the PATH only, or a folder: the ffmpeg in it [default: the PATH's when it has one,
    /// else ffmpeg/ in the data folder, downloaded there when missing (BtbN's build, with the dav1d AV1 decoder)]
    #[arg(long, value_name = "FOLDER|path")]
    pub ffmpeg: Option<PathBuf>,
    /// The server-mode UI build (`bun run build:server`) [default: aimview.json's ui, the repo's
    /// ui/dist/server/browser]
    #[arg(long, value_name = "FOLDER")]
    pub ui: Option<PathBuf>,
    /// The access token: letters, digits and - . _ ~. Prefer the settings file: a flag shows in the process list
    #[arg(long)]
    pub token: Option<String>,
    /// Dev mode: no token. On a network address (--host 0.0.0.0), any device on the local network gets in; only on a
    /// network you trust [default: off]
    #[arg(long)]
    pub dev: bool,
}

/// A path as given, empty too (clap's own parser refuses an empty one).
fn any_path(text: &str) -> Result<PathBuf, std::convert::Infallible> {
    Ok(PathBuf::from(text))
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
    pub gpu_frames: Option<bool>,
    pub ffmpeg: Option<PathBuf>,
    pub ui: Option<PathBuf>,
    pub token: Option<String>,
    pub dev: Option<bool>,
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
    pub gpu_frames: bool,
    pub ffmpeg: FfmpegChoice,
    pub ui: PathBuf,
    pub token: Option<String>,
    /// Dev mode: no token (access.rs).
    pub dev: bool,
}


impl Settings {
    /// The settings with no file and no flags.
    pub fn defaults() -> Settings {
        let config = LocalConfig::load();
        let (host, port) = config.server();
        let folder = |key: &str| config.folder(key).unwrap_or_default();
        Settings {
            host,
            port,
            data: folder("data"),
            vods: config.folder("vods"),
            stats: config.kovaak("stats").unwrap_or_default(),
            scenarios: config.kovaak_scenarios(),
            models: folder("models"),
            device: Device::Auto,
            gpu_frames: true,
            ffmpeg: FfmpegChoice::Auto(folder("ffmpeg")),
            ui: folder("ui"),
            token: None,
            dev: false,
        }
    }

    /// The URL to open, for the log (with the host as given, or `localhost` for an address that means "every one").
    pub fn url(&self) -> String {
        let host = match self.host.as_str() {
            "0.0.0.0" | "::" | "[::]" => "localhost",
            ipv6 if ipv6.contains(':') && !ipv6.starts_with('[') => return format!("http://[{ipv6}]:{}/", self.port),
            host => host,
        };
        format!("http://{host}:{}/", self.port)
    }
}

/// A path from the settings file, taken from the file's folder when it is relative.
fn from_file(base: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() || path.as_os_str().is_empty() { path } else { base.join(path) }
}

/// An empty path or token means "none".
fn some_path(path: PathBuf) -> Option<PathBuf> {
    (!path.as_os_str().is_empty()).then_some(path)
}

/// The settings file's text.
pub fn parse_file(text: &str) -> Result<FileSettings, String> {
    toml::from_str(text).map_err(|error| error.to_string())
}

/// The scenario folders: the flags' when they give any, else the file's, else the defaults'.
fn resolve_scenarios(
    flags: Vec<PathBuf>,
    file: Option<Vec<PathBuf>>,
    base: &Path,
    defaults: Vec<PathBuf>,
) -> Vec<PathBuf> {
    if !flags.is_empty() {
        flags
    } else if let Some(folders) = file {
        folders.into_iter().map(|folder| from_file(base, folder)).collect()
    } else {
        defaults
    }
}

/// The token, the flag's over the file's; empty is none. Refused when it holds a character a URL would encode.
fn resolve_token(flag: Option<String>, file: Option<String>) -> Result<Option<String>, String> {
    let token = flag.or(file).filter(|token| !token.is_empty());
    if let Some(token) = &token
        && !token.bytes().all(|b| b.is_ascii_alphanumeric() || TOKEN_SYMBOLS.contains(&b))
    {
        return Err("the token may hold only letters, digits and - . _ ~".into());
    }
    Ok(token)
}

/// Where ffmpeg comes from: "path" for the PATH's; without a choice, the PATH's or else the data folder's (which
/// follows --data).
fn resolve_ffmpeg(flag: Option<PathBuf>, file: Option<PathBuf>, base: &Path, data: &Path) -> FfmpegChoice {
    let is_path = |choice: &PathBuf| choice.as_os_str().eq_ignore_ascii_case(FFMPEG_FROM_PATH);
    match flag.or(file.map(|choice| if is_path(&choice) { choice } else { from_file(base, choice) })) {
        Some(choice) if is_path(&choice) => FfmpegChoice::Path,
        Some(folder) if !folder.as_os_str().is_empty() => FfmpegChoice::Folder(folder),
        _ => FfmpegChoice::Auto(data.join("ffmpeg")),
    }
}

/// The flags over the file (whose relative paths are taken from `base`, its folder) over the defaults.
pub fn resolve(flags: Flags, file: FileSettings, base: &Path, defaults: Settings) -> Result<Settings, String> {
    let path = |flag: Option<PathBuf>, file: Option<PathBuf>| flag.or_else(|| file.map(|path| from_file(base, path)));
    let scenarios = resolve_scenarios(flags.scenarios, file.scenarios, base, defaults.scenarios);
    let token = resolve_token(flags.token, file.token)?;
    let data = path(flags.data, file.data).unwrap_or(defaults.data);
    let ffmpeg = resolve_ffmpeg(flags.ffmpeg, file.ffmpeg, base, &data);
    let host = flags.host.or(file.host).unwrap_or(defaults.host);
    if host.is_empty() {
        return Err("the host is empty".into());
    }
    Ok(Settings {
        host,
        port: flags.port.or(file.port).unwrap_or(defaults.port),
        data,
        vods: match path(flags.vods, file.vods) {
            Some(vods) => some_path(vods),
            None => defaults.vods,
        },
        stats: path(flags.stats, file.stats).unwrap_or(defaults.stats),
        scenarios,
        models: path(flags.models, file.models).unwrap_or(defaults.models),
        device: flags.device.or(file.device).unwrap_or(defaults.device),
        gpu_frames: flags.gpu_frames.map(|switch| switch == Switch::On).or(file.gpu_frames).unwrap_or(defaults.gpu_frames),
        ffmpeg,
        ui: path(flags.ui, file.ui).unwrap_or(defaults.ui),
        token,
        dev: flags.dev || file.dev.unwrap_or(defaults.dev),
    })
}

/// The settings from the command line, and from the settings file it names (or aimview-server.toml in the current
/// folder). Also gives the file read, if any.
pub fn load(flags: Flags) -> Result<(Settings, Option<PathBuf>), String> {
    let path = flags.config.clone().or_else(|| Path::new(DEFAULT_FILE).is_file().then(|| PathBuf::from(DEFAULT_FILE)));
    let (file, base) = match &path {
        Some(path) => {
            let unreadable = |error: String| format!("the settings file {}: {error}", path.display());
            let text = std::fs::read_to_string(path).map_err(|error| unreadable(error.to_string()))?;
            let file = parse_file(&text).map_err(unreadable)?;
            let path = std::path::absolute(path).unwrap_or_else(|_| path.clone());
            (file, path.parent().map(Path::to_path_buf).unwrap_or_default())
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
        let settings = resolve(Flags::default(), FileSettings::default(), Path::new(""), Settings::defaults()).unwrap();
        assert_eq!(settings, Settings::defaults());
        assert_eq!((settings.host.as_str(), settings.port), ("127.0.0.1", 8770));
        assert!(settings.data.ends_with("test_out"));
        assert!(settings.ui.ends_with(Path::new("ui").join("dist").join("server").join("browser")));
        assert_eq!(settings.scenarios.len(), 2);
        assert_eq!(settings.device, Device::Auto);
        assert_eq!(settings.token, None);
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
            "#,
        )
        .unwrap();
        let base = std::path::absolute("base").unwrap();
        let settings = resolve(Flags::default(), file, &base, Settings::defaults()).unwrap();
        assert_eq!((settings.host.as_str(), settings.port), ("0.0.0.0", 9000));
        assert_eq!(settings.data, base.join("aim-data"));
        assert_eq!(settings.vods, Some(PathBuf::from(r"E:\OBS\KovOBS")));
        assert_eq!(settings.scenarios, vec![base.join("a"), base.join("sub/b")]);
        assert_eq!(settings.device, Device::DirectMl);
        assert_eq!(settings.token.as_deref(), Some("abc-123"));
        assert_eq!(settings.ffmpeg, FfmpegChoice::Folder(base.join("tools")));
        // the settings the file leaves out stay the defaults
        assert_eq!(settings.stats, Settings::defaults().stats);
    }

    #[test]
    fn flags_override_the_file() {
        let file = parse_file("port = 9000\ndevice = \"cpu\"\nscenarios = [\"x\"]\ntoken = \"from-file\"").unwrap();
        let given = flags(&[
            "--port", "8775", "--device", "cuda", "--scenarios", "s1", "s2", "--scenarios", "s3", "--token", "flag",
            "--vods=", "--ffmpeg", "path",
        ]);
        let settings = resolve(given, file, Path::new("base"), Settings::defaults()).unwrap();
        assert_eq!(settings.port, 8775);
        assert_eq!(settings.device, Device::Cuda);
        assert_eq!(settings.scenarios, ["s1", "s2", "s3"].map(PathBuf::from).to_vec());
        assert_eq!(settings.token.as_deref(), Some("flag"));
        // "" for the VODs folder: none
        assert_eq!(settings.vods, None);
        assert_eq!(settings.ffmpeg, FfmpegChoice::Path);
        // ffmpeg follows the data folder
        let given = flags(&["--data", "d"]);
        let settings = resolve(given, FileSettings::default(), Path::new(""), Settings::defaults()).unwrap();
        assert_eq!(settings.ffmpeg, FfmpegChoice::Auto(Path::new("d").join("ffmpeg")));
        let file = parse_file("ffmpeg = \"PATH\"").unwrap();
        let settings = resolve(Flags::default(), file, Path::new("base"), Settings::defaults()).unwrap();
        assert_eq!(settings.ffmpeg, FfmpegChoice::Path);
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
    fn gpu_frames_are_on_unless_turned_off() {
        assert!(Settings::defaults().gpu_frames);
        assert_eq!(flags(&["--gpu-frames", "off"]).gpu_frames, Some(Switch::Off));
        assert_eq!(parse_file("gpu_frames = false").unwrap().gpu_frames, Some(false));
        assert!(Flags::try_parse_from(["aimview-server", "--gpu-frames", "maybe"]).is_err());
    }

    #[test]
    fn a_bad_file_or_token_is_refused() {
        assert!(parse_file("prot = 8770").is_err(), "an unknown setting");
        assert!(parse_file("port = \"eighty\"").is_err());
        let bad = flags(&["--token", "has space"]);
        assert!(resolve(bad, FileSettings::default(), Path::new(""), Settings::defaults()).is_err());
        // an empty token is no token
        let empty = flags(&["--token", ""]);
        let settings = resolve(empty, FileSettings::default(), Path::new(""), Settings::defaults()).unwrap();
        assert_eq!(settings.token, None);
    }

    #[test]
    fn the_url_to_open() {
        let mut settings = Settings::defaults();
        assert_eq!(settings.url(), "http://127.0.0.1:8770/");
        settings.host = "0.0.0.0".into();
        assert_eq!(settings.url(), "http://localhost:8770/");
        settings.host = "::1".into();
        assert_eq!(settings.url(), "http://[::1]:8770/");
    }
}
