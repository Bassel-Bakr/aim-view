//! aimview-tool: the review service's library and its native review from the command line, for the Python scripts
//! (python/aimview_tools.py runs it). Each command prints JSON on stdout; a failure prints {"error", "status"} there
//! and exits with 1 (status: 404 for something missing, 400 for a bad value, else 500). A review's progress goes to
//! stderr. `aimview-tool help` lists the commands and their options.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::str::FromStr;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use aimview::scenario::{Facts, Kind};
use aimview_service::areas::kovobs_areas;
use aimview_service::library::parse_name;
use aimview_service::review::{AreaBox, Request, TimeWindow, parts_at_once, review};
use aimview_service::{Config, Device, Failure, Ffmpeg, Layout, Library};
use serde_json::{Value, json};

const USAGE: &str = concat!(
    "aimview-tool: Aim View's library and native review for scripts. ",
    "JSON on stdout; a review's progress on stderr.",
    r#"

aimview-tool recordings [library options]
    Every recording, newest first, as /api/vods lists it, with its video, its folder in the data folder (dir), the
    stats file it uses (stats_file) and the one its scenario and time stamp give (stats_found).
aimview-tool scenarios [library options]
    Every scenario's facts by its lower-case name, from the scenario folders: {kind, limit, targets, reload}.
aimview-tool lookup [library options] [--video FILE]... [--id ID]... [--run SCENARIO STAMP]...
    Videos (their recording's id, folder and stats file; for a video outside the library, the stats file its name
    gives), recordings by id, and the stats files of runs by scenario and time stamp.
aimview-tool crop-labels PAGE [--sets SET...] [library options]
    The training labels of a check folder's answers drawn on the Crops page (src/shapes.rs): by crop id, each
    target's visible box, its shapes and roles, the ignore boxes and the targets' pixels as run lengths.
aimview-tool review VIDEO --out FOLDER [review options] [library options]
    The video reviewed as the app reviews a recording, without touching the library's reviews: tracks.json,
    readings.json, hud.json and report.json in FOLDER, and {seconds, out, report} on stdout.

Library options:
  --data FOLDER           the data folder [the repo's test_out]
  --layout python|app     its layout [python]
  --models FOLDER         the models (detector_<name>_u8in.onnx, models.json) [the repo's python/model/exports]
  --vods FOLDER           the recordings; --vods= for the folder chosen in the app [E:\OBS\KovOBS]
  --stats FOLDER          KovaaK's stats folder [FPSAimTrainer\stats in Steam's folder]
  --scenarios FOLDER...   the scenario folders [KovaaK's and the workshop's]
  --device auto|directml|cuda|cpu   where the detector runs [auto]
  --gpu-frames on|off     decode and convert on the GPU where the video allows it (Windows, 2560 x 1440 AV1 or
                          H.264 MP4s) [on]
  --ffmpeg FOLDER|path    where ffmpeg and ffprobe are [path: the PATH's]
  --download-ffmpeg       download ffmpeg into the --ffmpeg folder when a review first needs it

Review options:
  --model NAME|FILE.onnx  a model's name, or its _u8in export [the model picked in the app]
  --stats-file FILE       the run's stats file, for the report
  --kind static|dynamic|tracking|switching   the scenario's kind, for the report
  --limit SECONDS         the scenario's time limit, for the report
  --cap N                 the scenario's targets alive at once
  --areas FILE|JSON       the areas to leave out, [[x0, y0, x1, y1, kind], ...] as shares of the frame [the
                          recording's in the app, else KovOBS's layout]
  --runs N                the parts of the video reviewed at once [with 8 threads or more: 4 with GPU frames,
                          else 2; else 1]
  --batch N               the frames the detector takes at once [4]
  --gpu-share SHARE       the share of the time the detector runs, 0.05 to 1, resting the rest so a game beside it
                          keeps the GPU; the results do not change [1]
  --window START END      only this part of the video tracked, in seconds
  --no-report             no report.json
  --quiet                 no progress on stderr
"#
);

/// The exit code for a command line that cannot be read (the usage is printed).
const USAGE_EXIT: u8 = 2;
/// The recordings when --vods is not given (Windows only): KovOBS's folder.
const DEFAULT_VODS: &str = r"E:\OBS\KovOBS";
/// The frames the detector takes at once when --batch is not given.
const DEFAULT_BATCH: usize = 4;
/// How often a terminal's progress line is written again within a stage.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// The values an option takes.
#[derive(Clone, Copy, PartialEq)]
enum Takes {
    Nothing,
    One,
    Two,
    /// One or more, up to the next option
    Many,
}

type Options = &'static [(&'static str, Takes)];

const LIBRARY: Options = &[
    ("data", Takes::One),
    ("layout", Takes::One),
    ("models", Takes::One),
    ("vods", Takes::One),
    ("stats", Takes::One),
    ("scenarios", Takes::Many),
    ("device", Takes::One),
    ("gpu-frames", Takes::One),
    ("ffmpeg", Takes::One),
    ("download-ffmpeg", Takes::Nothing),
];

const LOOKUP: Options = &[("video", Takes::One), ("id", Takes::One), ("run", Takes::Two)];

const CROP_LABELS: Options = &[("sets", Takes::Many)];

const REVIEW: Options = &[
    ("out", Takes::One),
    ("model", Takes::One),
    ("stats-file", Takes::One),
    ("kind", Takes::One),
    ("limit", Takes::One),
    ("cap", Takes::One),
    ("areas", Takes::One),
    ("runs", Takes::One),
    ("batch", Takes::One),
    ("gpu-share", Takes::One),
    ("window", Takes::Two),
    ("no-report", Takes::Nothing),
    ("quiet", Takes::Nothing),
];

/// A command line: the values given before or between the options, and each option with its values, in order.
struct Line {
    free: Vec<String>,
    given: Vec<(&'static str, Vec<String>)>,
}

impl Line {
    /// `--name value`, `--name=value`, `--name a b` (Takes::Two) or `--name a b c` (Takes::Many).
    fn parse(args: &[String], known: &[Options]) -> Result<Line, String> {
        let mut line = Line { free: Vec::new(), given: Vec::new() };
        let mut i = 0;
        while i < args.len() {
            let arg = &args[i];
            i += 1;
            let Some(option) = arg.strip_prefix("--") else {
                line.free.push(arg.clone());
                continue;
            };
            let (name, inline) = match option.split_once('=') {
                Some((name, value)) => (name, Some(value.to_string())),
                None => (option, None),
            };
            let known_option = known.iter().flat_map(|options| options.iter()).find(|(known, _)| *known == name);
            let Some(&(name, takes)) = known_option else {
                return Err(format!("no option --{name}"));
            };
            let values = option_values(name, takes, inline, args, &mut i)?;
            line.given.push((name, values));
        }
        Ok(line)
    }

    fn has(&self, name: &str) -> bool {
        self.given.iter().any(|(given, _)| *given == name)
    }

    /// Each time the option was given, its values.
    fn all<'a>(&'a self, name: &str) -> impl Iterator<Item = &'a [String]> + use<'a> {
        let name = name.to_string();
        self.given.iter().filter(move |(given, _)| *given == name).map(|(_, values)| values.as_slice())
    }

    /// The option's value (the last one given).
    fn one(&self, name: &str) -> Option<&str> {
        self.all(name).last().map(|values| values[0].as_str())
    }

    fn path(&self, name: &str) -> Option<PathBuf> {
        self.one(name).map(PathBuf::from)
    }

    fn number<T: FromStr>(&self, name: &str) -> Result<Option<T>, Failure> {
        let parse = |value: &str| value.parse().map_err(|_| Failure::bad(format!("--{name}: not a number: {value}")));
        self.one(name).map(parse).transpose()
    }
}

/// An option's values: the one given with "=" (`inline`), and those after it in `args` from `next`, which moves past
/// them.
fn option_values(
    name: &str,
    takes: Takes,
    inline: Option<String>,
    args: &[String],
    next: &mut usize,
) -> Result<Vec<String>, String> {
    let mut values: Vec<String> = inline.into_iter().collect();
    match takes {
        Takes::Nothing if !values.is_empty() => return Err(format!("--{name} takes no value")),
        Takes::Nothing => {}
        Takes::One | Takes::Two => {
            let wanted = if takes == Takes::One { 1 } else { 2 };
            while values.len() < wanted {
                let plural = if wanted > 1 { "s" } else { "" };
                let value = args.get(*next).ok_or_else(|| format!("--{name} takes {wanted} value{plural}"))?;
                values.push(value.clone());
                *next += 1;
            }
        }
        Takes::Many => {
            while *next < args.len() && !args[*next].starts_with("--") {
                values.push(args[*next].clone());
                *next += 1;
            }
            if values.is_empty() {
                return Err(format!("--{name} takes one value or more"));
            }
        }
    }
    Ok(values)
}

/// The repo this tool was built from (the defaults point into it).
fn repo() -> PathBuf {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    here.parent().unwrap_or(here).to_path_buf()
}

/// The library's configuration from the options, over the defaults (python/aimview_tools.py's: the repo's test_out/ in
/// Python's layout, KovOBS's recordings, KovaaK's folders, the models in python/model/exports, ffmpeg from the PATH).
fn config(line: &Line) -> Result<Config, Failure> {
    let repo = repo();
    let layout = match line.one("layout").unwrap_or("python") {
        "python" => Layout::Python,
        "app" => Layout::App,
        other => return Err(Failure::bad(format!("--layout: python or app, not {other}"))),
    };
    let data = line.path("data").unwrap_or_else(|| repo.join("test_out"));
    let models = line.path("models").unwrap_or_else(|| repo.join("python").join("model").join("exports"));
    let mut config = Config::new(data, layout, models);
    config.vods = match line.one("vods") {
        Some("") => None,
        Some(folder) => Some(folder.into()),
        None => cfg!(windows).then(|| PathBuf::from(DEFAULT_VODS)),
    };
    if let Some(stats) = line.path("stats") {
        config.stats = stats;
    }
    if let Some(folders) = line.all("scenarios").last() {
        config.scenarios = folders.iter().map(PathBuf::from).collect();
    }
    config.device = match line.one("device").unwrap_or("auto") {
        "auto" => Device::Auto,
        "directml" => Device::DirectMl,
        "cuda" => Device::Cuda,
        "cpu" => Device::Cpu,
        other => return Err(Failure::bad(format!("--device: auto, directml, cuda or cpu, not {other}"))),
    };
    config.gpu_frames = match line.one("gpu-frames").unwrap_or("on") {
        "on" => true,
        "off" => false,
        other => return Err(Failure::bad(format!("--gpu-frames: on or off, not {other}"))),
    };
    let download = line.has("download-ffmpeg");
    config.ffmpeg = match line.one("ffmpeg") {
        None if download => return Err(Failure::bad("--download-ffmpeg needs --ffmpeg <the folder to keep it in>")),
        None => Ffmpeg::Path,
        Some(path) if path.eq_ignore_ascii_case("path") => Ffmpeg::Path,
        Some(folder) if download => Ffmpeg::Download(folder.into()),
        Some(folder) => Ffmpeg::Folder(folder.into()),
    };
    Ok(config)
}

/// The id of the recording at `video`: its path in the VODs folder ("<folder>/<file>"), or "uploads/<name>"; None for a
/// video outside the library's folders.
fn id_of(library: &Library, video: &Path) -> Option<String> {
    let video = video.canonicalize().ok()?;
    let inside = |root: Option<PathBuf>| {
        let root = root?.canonicalize().ok()?;
        let relative = video.strip_prefix(root).ok()?;
        let parts: Vec<_> = relative.components().map(|part| part.as_os_str().to_string_lossy()).collect();
        Some(parts.join("/"))
    };
    let uploads = Some(library.folders().uploads.clone());
    let id = inside(library.vods()).or_else(|| inside(uploads).map(|name| format!("uploads/{name}")))?;
    library.resolve(&id).is_ok().then_some(id)
}

/// The stats file of a run by its video's name (KovOBS's) and time, as for a video outside the library.
fn stats_by_name(library: &Library, video: &Path) -> Option<PathBuf> {
    let name = video.with_extension("mp4").file_name()?.to_string_lossy().into_owned();
    let (scenario, _, stamp) = parse_name(&name)?;
    library.stats_for(&scenario, &stamp)
}

/// Every recording (/api/vods), each with its video, folder, stats file, and the stats file its scenario and time stamp
/// give; and the library's folders.
fn recordings(library: &Library) -> Result<Value, Failure> {
    let Value::Array(list) = library.recordings(false)? else {
        return Err(Failure::from("the recordings are not a list".to_string()));
    };
    let list: Vec<Value> = list
        .into_iter()
        .map(|mut row| {
            let id = row["id"].as_str().unwrap_or_default().to_string();
            let found = match (row["scenario"].as_str(), row["stamp"].as_str()) {
                (Some(scenario), Some(stamp)) => library.stats_for(scenario, stamp),
                _ => None,
            };
            row["video"] = json!(library.resolve(&id).ok());
            row["dir"] = json!(library.review_dir(&id));
            row["stats_file"] = json!(library.stats_path(&id));
            row["stats_found"] = json!(found);
            row
        })
        .collect();
    let folders = library.folders();
    Ok(json!({
        "vods": library.vods(),
        "stats": library.stats_folder(),
        "folders": {
            "files": folders.files, "recordings": folders.recordings, "uploads": folders.uploads,
            "cutoff": folders.cutoff, "mouse": folders.mouse,
        },
        "recordings": list,
    }))
}

/// Videos, recordings by id and runs by scenario and time stamp, each with its stats file.
fn lookup(library: &Library, line: &Line) -> Value {
    let videos: Vec<Value> = line
        .all("video")
        .map(|values| {
            let video = PathBuf::from(&values[0]);
            match id_of(library, &video) {
                Some(id) => {
                    let (dir, stats_file) = (library.review_dir(&id), library.stats_path(&id));
                    json!({ "video": video, "id": id, "dir": dir, "stats_file": stats_file })
                }
                None => {
                    json!({ "video": video, "id": null, "dir": null, "stats_file": stats_by_name(library, &video) })
                }
            }
        })
        .collect();
    let ids: Vec<Value> = line
        .all("id")
        .map(|values| {
            let id = &values[0];
            let (video, dir, stats_file) = (library.resolve(id).ok(), library.review_dir(id), library.stats_path(id));
            json!({ "id": id, "video": video, "dir": dir, "stats_file": stats_file })
        })
        .collect();
    let runs: Vec<Value> = line
        .all("run")
        .map(|values| {
            let (scenario, stamp) = (&values[0], &values[1]);
            json!({ "scenario": scenario, "stamp": stamp, "stats_file": library.stats_for(scenario, stamp) })
        })
        .collect();
    json!({ "videos": videos, "ids": ids, "runs": runs })
}

fn kind(name: &str) -> Result<Kind, Failure> {
    match name {
        "static" => Ok(Kind::Static),
        "dynamic" => Ok(Kind::Dynamic),
        "tracking" => Ok(Kind::Tracking),
        "switching" => Ok(Kind::Switching),
        other => Err(Failure::bad(format!("--kind: static, dynamic, tracking or switching, not {other}"))),
    }
}

/// The areas to leave out, from a JSON file or the JSON itself: [x0, y0, x1, y1] or [x0, y0, x1, y1, kind id], as
/// shares of the frame.
fn area_boxes(given: &str) -> Result<Vec<AreaBox>, Failure> {
    let text = if given.trim_start().starts_with('[') {
        given.to_string()
    } else {
        std::fs::read_to_string(given).map_err(|error| Failure::missing(format!("--areas {given}: {error}")))?
    };
    let list: Vec<Vec<Value>> =
        serde_json::from_str(&text).map_err(|error| Failure::bad(format!("--areas: {error}")))?;
    let shape = || Failure::bad("--areas: an area is [x0, y0, x1, y1] or [x0, y0, x1, y1, kind]");
    list.into_iter()
        .map(|values| {
            if !(4..=5).contains(&values.len()) {
                return Err(shape());
            }
            let edge = |i: usize| values[i].as_f64().ok_or_else(shape);
            let kind = values.get(4).map(|kind| kind.as_str().map(str::to_string).ok_or_else(shape)).transpose()?;
            Ok((edge(0)?, edge(1)?, edge(2)?, edge(3)?, kind.unwrap_or_else(|| "other".into())))
        })
        .collect()
}

/// A review's progress on stderr: one line that changes, in a terminal; else a line for each stage.
struct Progress {
    quiet: bool,
    terminal: bool,
    last: Mutex<Shown>,
}

/// The stage a progress line last showed, and when it was last told.
struct Shown {
    stage: String,
    at: Instant,
}

impl Progress {
    fn new(quiet: bool) -> Progress {
        let last = Mutex::new(Shown { stage: String::new(), at: Instant::now() });
        Progress { quiet, terminal: std::io::stderr().is_terminal(), last }
    }

    fn show(&self, stage: &str, done: usize, total: usize) {
        if self.quiet {
            return;
        }
        let mut last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
        let new = last.stage != stage;
        if self.terminal && (new || done == total || last.at.elapsed() >= PROGRESS_INTERVAL) {
            eprint!("\r{stage} {done}/{total}      ");
        } else if !self.terminal && new {
            eprintln!("{stage} {done}/{total}");
        }
        if new {
            last.stage = stage.to_string();
        }
        last.at = Instant::now();
    }

    fn finish(&self, seconds: f64) {
        if !self.quiet {
            eprintln!("{}reviewed in {seconds:.1} s", if self.terminal { "\n" } else { "" });
        }
    }
}

/// The model file a review uses: --model's _u8in export, or the model of that name, or the model picked in the app.
fn review_model(library: &Library, line: &Line) -> PathBuf {
    match line.one("model") {
        Some(file) if file.to_lowercase().ends_with(".onnx") => PathBuf::from(file),
        Some(name) => library.model_file(name),
        None => library.model_file(&library.model()),
    }
}

/// The areas a review leaves out: --areas, else the recording's in the app, else KovOBS's layout.
fn review_areas(library: &Library, line: &Line, video: &Path) -> Result<Vec<AreaBox>, Failure> {
    match line.one("areas") {
        Some(given) => area_boxes(given),
        None => match id_of(library, video) {
            Some(id) => library.exclude_boxes(&id),
            None => Ok(kovobs_areas()),
        },
    }
}

/// The part of the video tracked (--window, seconds); None: all of it.
fn review_window(line: &Line) -> Result<Option<TimeWindow>, Failure> {
    let Some(window) = line.all("window").last() else { return Ok(None) };
    let at = |text: &str| text.parse::<f64>().map_err(|_| Failure::bad(format!("--window: not a number: {text}")));
    Ok(Some(TimeWindow { start: at(&window[0])?, end: at(&window[1])? }))
}

/// The video reviewed as the app reviews a recording (python-bindings' `review_video`, retired): the files in --out,
/// the report worked out from them unless --no-report.
fn review_video(library: &Library, line: &Line) -> Result<Value, Failure> {
    let [video] = line.free.as_slice() else { return Err(Failure::bad("review takes one video")) };
    let video = PathBuf::from(video);
    let out = line.path("out").ok_or_else(|| Failure::bad("review needs --out <folder>"))?;
    let model = review_model(library, line);
    let areas = review_areas(library, line, &video)?;
    let cap: Option<usize> = line.number("cap")?;
    let limit: Option<f64> = line.number("limit")?;
    let facts = line.one("kind").map(kind).transpose()?.map(|kind| Facts { kind, limit, targets: cap, reload: None });
    let window = review_window(line)?;
    let threads = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    let request = Request {
        video,
        model,
        device: library.config().device,
        batch: line.number("batch")?.unwrap_or(DEFAULT_BATCH),
        cap: cap.unwrap_or(0),
        runs: line.number("runs")?.unwrap_or(parts_at_once(threads, library.config().gpu_frames)),
        window,
        areas,
        keep_parts: None,
        gpu_frames: library.config().gpu_frames,
        gpu_share: line.number("gpu-share")?.unwrap_or(1.0),
    };
    let progress = Progress::new(line.has("quiet"));
    let started = Instant::now();
    aimview_service::ffmpeg::ensure(|megabytes, of| progress.show("ffmpeg", megabytes, of))?;
    let reviewed = review(&request, &|stage, done, total| progress.show(stage, done, total), &|_| {})?;
    let seconds = started.elapsed().as_secs_f64();
    progress.finish(seconds);
    std::fs::create_dir_all(&out).map_err(|error| format!("{}: {error}", out.display()))?;
    let write = |name: &str, bytes: Result<Vec<u8>, serde_json::Error>| -> Result<(), Failure> {
        let path = out.join(format!("{name}.json"));
        let bytes = bytes.map_err(|error| error.to_string())?;
        std::fs::write(&path, bytes).map_err(|error| Failure::from(format!("{}: {error}", path.display())))
    };
    write("tracks", serde_json::to_vec(&reviewed.tracks))?;
    write("readings", serde_json::to_vec(&reviewed.readings))?;
    write("hud", serde_json::to_vec(&reviewed.hud))?;
    let stats = line.path("stats-file");
    let report = if line.has("no-report") {
        None
    } else {
        aimview_service::report::work_out(&out, &request.video, stats.as_deref(), None, facts.as_ref(), None)?
    };
    if let Some(report) = &report {
        write("report", serde_json::to_vec(report))?;
    }
    Ok(json!({ "seconds": seconds, "out": out, "report": report.is_some() }))
}

/// Prints a line of JSON on stdout.
fn print(value: &Value) {
    let mut out = std::io::stdout().lock();
    // a closed pipe: the reader has gone, nothing to tell
    let _ = serde_json::to_writer(&mut out, value)
        .map_err(std::io::Error::from)
        .and_then(|()| writeln!(out))
        .and_then(|()| out.flush());
}

/// The answer to a command whose line was read: the library opened with the line's options, then asked.
fn answer(command: &str, line: &Line) -> Result<Value, Failure> {
    let library = Library::open(config(line)?).map_err(Failure::from)?;
    match command {
        "recordings" => recordings(&library),
        "scenarios" => Ok(library.scenarios()),
        "lookup" => Ok(lookup(&library, line)),
        "crop-labels" => {
            let sets: Vec<String> = line.all("sets").flatten().cloned().collect();
            library.crop_labels(line.free.first().map_or("", String::as_str), &sets)
        }
        _ => review_video(&library, line),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = |error: &str| {
        eprintln!("aimview-tool: {error}\n\n{USAGE}");
        ExitCode::from(USAGE_EXIT)
    };
    let Some(command) = args.first() else { return usage("no command") };
    let options: &[Options] = match command.as_str() {
        "recordings" | "scenarios" => &[LIBRARY],
        "lookup" => &[LIBRARY, LOOKUP],
        "crop-labels" => &[LIBRARY, CROP_LABELS],
        "review" => &[LIBRARY, REVIEW],
        "help" | "--help" | "-h" => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        other => return usage(&format!("no command {other}")),
    };
    let line = match Line::parse(&args[1..], options) {
        Ok(line) => line,
        Err(error) => return usage(&error),
    };
    if !matches!(command.as_str(), "review" | "crop-labels") && !line.free.is_empty() {
        return usage(&format!("{command} takes no value {}", line.free[0]));
    }
    match answer(command, &line) {
        Ok(value) => {
            print(&value);
            ExitCode::SUCCESS
        }
        Err(failure) => {
            print(&json!({ "error": failure.message, "status": failure.status }));
            ExitCode::FAILURE
        }
    }
}
