//! aimview-tool: the review service's library and its native review from the command line, for the Python scripts
//! (python/aimview_tools.py runs it). Each command prints JSON on stdout; a failure prints {"error", "status"} there
//! and exits with 1 (status: 404 for something missing, 400 for a bad value, else 500). A review's progress goes to
//! stderr. `aimview-tool help` lists the commands and their options.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::str::FromStr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use aimview::scenario::{Facts, Kind};
use aimview_service::areas::kovobs_areas;
use aimview_service::library::parse_name;
use aimview_service::review::{AreaBox, Request, TimeWindow, review};
use aimview_service::{Config, Device, Failure, Ffmpeg, Layout, Library};
use serde_json::{Value, json};

const USAGE: &str = r#"aimview-tool: Aim View's library and native review for scripts. JSON on stdout; a review's progress on stderr.

aimview-tool recordings [library options]
    Every recording, newest first, as /api/vods lists it, with its video, its folder in the data folder (dir), the
    stats file it uses (stats_file) and the one its scenario and time stamp give (stats_found).
aimview-tool scenarios [library options]
    Every scenario's facts by its lower-case name, from the scenario folders: {kind, limit, targets, reload}.
aimview-tool lookup [library options] [--video FILE]... [--id ID]... [--run SCENARIO STAMP]...
    Videos (their recording's id, folder and stats file; for a video outside the library, the stats file its name
    gives), recordings by id, and the stats files of runs by scenario and time stamp.
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
  --runs N                the parts of the video reviewed at once [2 with 8 threads or more, else 1]
  --batch N               the frames the detector takes at once [4]
  --window START END      only this part of the video tracked, in seconds
  --no-report             no report.json
  --quiet                 no progress on stderr
"#;

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
    ("ffmpeg", Takes::One),
    ("download-ffmpeg", Takes::Nothing),
];

const LOOKUP: Options = &[("video", Takes::One), ("id", Takes::One), ("run", Takes::Two)];

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
            let Some(&(name, takes)) = known.iter().flat_map(|o| o.iter()).find(|(n, _)| *n == name) else {
                return Err(format!("no option --{name}"));
            };
            let mut values: Vec<String> = inline.into_iter().collect();
            match takes {
                Takes::Nothing if !values.is_empty() => return Err(format!("--{name} takes no value")),
                Takes::Nothing => {}
                Takes::One | Takes::Two => {
                    let n = if takes == Takes::One { 1 } else { 2 };
                    while values.len() < n {
                        let value = args.get(i).ok_or_else(|| format!("--{name} takes {n} value{}", if n > 1 { "s" } else { "" }))?;
                        values.push(value.clone());
                        i += 1;
                    }
                }
                Takes::Many => {
                    while i < args.len() && !args[i].starts_with("--") {
                        values.push(args[i].clone());
                        i += 1;
                    }
                    if values.is_empty() {
                        return Err(format!("--{name} takes one value or more"));
                    }
                }
            }
            line.given.push((name, values));
        }
        Ok(line)
    }

    fn has(&self, name: &str) -> bool {
        self.given.iter().any(|(n, _)| *n == name)
    }

    /// Each time the option was given, its values.
    fn all<'a>(&'a self, name: &str) -> impl Iterator<Item = &'a [String]> + use<'a> {
        let name = name.to_string();
        self.given.iter().filter(move |(n, _)| *n == name).map(|(_, v)| v.as_slice())
    }

    /// The option's value (the last one given).
    fn one(&self, name: &str) -> Option<&str> {
        self.all(name).last().map(|v| v[0].as_str())
    }

    fn path(&self, name: &str) -> Option<PathBuf> {
        self.one(name).map(PathBuf::from)
    }

    fn number<T: FromStr>(&self, name: &str) -> Result<Option<T>, Failure> {
        self.one(name).map(|v| v.parse().map_err(|_| Failure::bad(format!("--{name}: not a number: {v}")))).transpose()
    }
}

/// The repo this tool was built from (the defaults point into it).
fn repo() -> PathBuf {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    here.parent().unwrap_or(here).to_path_buf()
}

/// The library's configuration from the options, over the defaults (python/aimview_tools.py's: the repo's test_out/
/// in Python's layout, KovOBS's recordings, KovaaK's folders, the models in python/model/exports, ffmpeg from the PATH).
fn config(line: &Line) -> Result<Config, Failure> {
    let repo = repo();
    let layout = match line.one("layout").unwrap_or("python") {
        "python" => Layout::Python,
        "app" => Layout::App,
        other => return Err(Failure::bad(format!("--layout: python or app, not {other}"))),
    };
    let data = line.path("data").unwrap_or_else(|| repo.join("test_out"));
    let models = line.path("models").unwrap_or_else(|| repo.join("python").join("model").join("exports"));
    let mut c = Config::new(data, layout, models);
    c.vods = match line.one("vods") {
        Some("") => None,
        Some(folder) => Some(folder.into()),
        None => cfg!(windows).then(|| PathBuf::from(r"E:\OBS\KovOBS")),
    };
    if let Some(stats) = line.path("stats") {
        c.stats = stats;
    }
    if let Some(folders) = line.all("scenarios").last() {
        c.scenarios = folders.iter().map(PathBuf::from).collect();
    }
    c.device = match line.one("device").unwrap_or("auto") {
        "auto" => Device::Auto,
        "directml" => Device::DirectMl,
        "cuda" => Device::Cuda,
        "cpu" => Device::Cpu,
        other => return Err(Failure::bad(format!("--device: auto, directml, cuda or cpu, not {other}"))),
    };
    let download = line.has("download-ffmpeg");
    c.ffmpeg = match line.one("ffmpeg") {
        None if download => return Err(Failure::bad("--download-ffmpeg needs --ffmpeg <the folder to keep it in>")),
        None => Ffmpeg::Path,
        Some(p) if p.eq_ignore_ascii_case("path") => Ffmpeg::Path,
        Some(folder) if download => Ffmpeg::Download(folder.into()),
        Some(folder) => Ffmpeg::Folder(folder.into()),
    };
    Ok(c)
}

/// The id of the recording at `video`: its path in the VODs folder ("<folder>/<file>"), or "uploads/<name>"; None for a
/// video outside the library's folders.
fn id_of(lib: &Library, video: &Path) -> Option<String> {
    let video = video.canonicalize().ok()?;
    let inside = |root: Option<PathBuf>| {
        let root = root?.canonicalize().ok()?;
        let rel = video.strip_prefix(root).ok()?;
        Some(rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"))
    };
    let id = inside(lib.vods()).or_else(|| inside(Some(lib.folders().uploads.clone())).map(|n| format!("uploads/{n}")))?;
    lib.resolve(&id).is_ok().then_some(id)
}

/// The stats file of a run by its video's name (KovOBS's) and time, as for a video outside the library.
fn stats_by_name(lib: &Library, video: &Path) -> Option<PathBuf> {
    let name = video.with_extension("mp4").file_name()?.to_string_lossy().into_owned();
    let (scenario, _, stamp) = parse_name(&name)?;
    lib.stats_for(&scenario, &stamp)
}

/// Every recording (/api/vods), each with its video, folder, stats file, and the stats file its scenario and time stamp
/// give; and the library's folders.
fn recordings(lib: &Library) -> Result<Value, Failure> {
    let Value::Array(list) = lib.recordings(false)? else { return Err(Failure::from("the recordings are not a list".to_string())) };
    let list: Vec<Value> = list
        .into_iter()
        .map(|mut r| {
            let id = r["id"].as_str().unwrap_or_default().to_string();
            let found = match (r["scenario"].as_str(), r["stamp"].as_str()) {
                (Some(scenario), Some(stamp)) => lib.stats_for(scenario, stamp),
                _ => None,
            };
            r["video"] = json!(lib.resolve(&id).ok());
            r["dir"] = json!(lib.review_dir(&id));
            r["stats_file"] = json!(lib.stats_path(&id));
            r["stats_found"] = json!(found);
            r
        })
        .collect();
    let f = lib.folders();
    Ok(json!({
        "vods": lib.vods(),
        "stats": lib.stats_folder(),
        "folders": { "files": f.files, "recordings": f.recordings, "uploads": f.uploads, "cutoff": f.cutoff, "mouse": f.mouse },
        "recordings": list,
    }))
}

/// Videos, recordings by id and runs by scenario and time stamp, each with its stats file.
fn lookup(lib: &Library, line: &Line) -> Value {
    let videos: Vec<Value> = line
        .all("video")
        .map(|v| {
            let video = PathBuf::from(&v[0]);
            match id_of(lib, &video) {
                Some(id) => json!({ "video": video, "id": id, "dir": lib.review_dir(&id), "stats_file": lib.stats_path(&id) }),
                None => json!({ "video": video, "id": null, "dir": null, "stats_file": stats_by_name(lib, &video) }),
            }
        })
        .collect();
    let ids: Vec<Value> = line
        .all("id")
        .map(|v| {
            let id = &v[0];
            json!({ "id": id, "video": lib.resolve(id).ok(), "dir": lib.review_dir(id), "stats_file": lib.stats_path(id) })
        })
        .collect();
    let runs: Vec<Value> = line
        .all("run")
        .map(|v| json!({ "scenario": v[0], "stamp": v[1], "stats_file": lib.stats_for(&v[0], &v[1]) }))
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
        std::fs::read_to_string(given).map_err(|e| Failure::missing(format!("--areas {given}: {e}")))?
    };
    let list: Vec<Vec<Value>> = serde_json::from_str(&text).map_err(|e| Failure::bad(format!("--areas: {e}")))?;
    let shape = || Failure::bad("--areas: an area is [x0, y0, x1, y1] or [x0, y0, x1, y1, kind]");
    list.into_iter()
        .map(|b| {
            if !(4..=5).contains(&b.len()) {
                return Err(shape());
            }
            let n = |i: usize| b[i].as_f64().ok_or_else(shape);
            let kind = b.get(4).map(|k| k.as_str().map(str::to_string).ok_or_else(shape)).transpose()?;
            Ok((n(0)?, n(1)?, n(2)?, n(3)?, kind.unwrap_or_else(|| "other".into())))
        })
        .collect()
}

/// A review's progress on stderr: one line that changes, in a terminal; else a line for each stage.
struct Progress {
    quiet: bool,
    terminal: bool,
    /// The stage last shown, and when
    last: Mutex<(String, Instant)>,
}

impl Progress {
    fn new(quiet: bool) -> Progress {
        Progress { quiet, terminal: std::io::stderr().is_terminal(), last: Mutex::new((String::new(), Instant::now())) }
    }

    fn show(&self, stage: &str, done: usize, total: usize) {
        if self.quiet {
            return;
        }
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        let new = last.0 != stage;
        if self.terminal && (new || done == total || last.1.elapsed() >= Duration::from_millis(100)) {
            eprint!("\r{stage} {done}/{total}      ");
        } else if !self.terminal && new {
            eprintln!("{stage} {done}/{total}");
        }
        if new {
            last.0 = stage.to_string();
        }
        last.1 = Instant::now();
    }

    fn finish(&self, seconds: f64) {
        if !self.quiet {
            eprintln!("{}reviewed in {seconds:.1} s", if self.terminal { "\n" } else { "" });
        }
    }
}

/// The video reviewed as the app reviews a recording (python-bindings' `review_video`, retired): the files in --out,
/// the report worked out from them unless --no-report.
fn review_video(lib: &Library, line: &Line) -> Result<Value, Failure> {
    let [video] = line.free.as_slice() else { return Err(Failure::bad("review takes one video")) };
    let video = PathBuf::from(video);
    let out = line.path("out").ok_or_else(|| Failure::bad("review needs --out <folder>"))?;
    let model = match line.one("model") {
        Some(m) if m.to_lowercase().ends_with(".onnx") => PathBuf::from(m),
        Some(name) => lib.model_file(name),
        None => lib.model_file(&lib.model()),
    };
    let areas = match line.one("areas") {
        Some(given) => area_boxes(given)?,
        None => match id_of(lib, &video) {
            Some(id) => lib.exclude_boxes(&id)?,
            None => kovobs_areas(),
        },
    };
    let cap: Option<usize> = line.number("cap")?;
    let limit: Option<f64> = line.number("limit")?;
    let facts = line.one("kind").map(kind).transpose()?.map(|kind| Facts { kind, limit, targets: cap, reload: None });
    let window = match line.all("window").last() {
        Some(w) => {
            let at = |s: &str| s.parse::<f64>().map_err(|_| Failure::bad(format!("--window: not a number: {s}")));
            Some(TimeWindow { start: at(&w[0])?, end: at(&w[1])? })
        }
        None => None,
    };
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let req = Request {
        video,
        model,
        device: lib.config().device,
        batch: line.number("batch")?.unwrap_or(4),
        cap: cap.unwrap_or(0),
        runs: line.number("runs")?.unwrap_or(if threads >= 8 { 2 } else { 1 }),
        window,
        areas,
        keep_parts: None,
    };
    let progress = Progress::new(line.has("quiet"));
    let started = Instant::now();
    aimview_service::ffmpeg::ensure(|mb, of| progress.show("ffmpeg", mb, of))?;
    let r = review(&req, &|stage, done, total| progress.show(stage, done, total), &|_| {})?;
    let seconds = started.elapsed().as_secs_f64();
    progress.finish(seconds);
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let write = |name: &str, bytes: Result<Vec<u8>, serde_json::Error>| -> Result<(), Failure> {
        let p = out.join(format!("{name}.json"));
        let bytes = bytes.map_err(|e| e.to_string())?;
        std::fs::write(&p, bytes).map_err(|e| Failure::from(format!("{}: {e}", p.display())))
    };
    write("tracks", serde_json::to_vec(&r.tracks))?;
    write("readings", serde_json::to_vec(&r.readings))?;
    write("hud", serde_json::to_vec(&r.hud))?;
    let stats = line.path("stats-file");
    let report = if line.has("no-report") {
        None
    } else {
        aimview_service::report::work_out(&out, &req.video, stats.as_deref(), None, facts.as_ref(), None)?
    };
    if let Some(report) = &report {
        write("report", serde_json::to_vec(report))?;
    }
    Ok(json!({ "seconds": seconds, "out": out, "report": report.is_some() }))
}

/// Prints a line of JSON on stdout.
fn print(v: &Value) {
    let mut out = std::io::stdout().lock();
    // a closed pipe: the reader has gone, nothing to tell
    let _ = serde_json::to_writer(&mut out, v).map_err(std::io::Error::from).and_then(|_| writeln!(out)).and_then(|_| out.flush());
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = |e: &str| {
        eprintln!("aimview-tool: {e}\n\n{USAGE}");
        ExitCode::from(2)
    };
    let Some(command) = args.first() else { return usage("no command") };
    let options: &[Options] = match command.as_str() {
        "recordings" | "scenarios" => &[LIBRARY],
        "lookup" => &[LIBRARY, LOOKUP],
        "review" => &[LIBRARY, REVIEW],
        "help" | "--help" | "-h" => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        other => return usage(&format!("no command {other}")),
    };
    let line = match Line::parse(&args[1..], options) {
        Ok(line) => line,
        Err(e) => return usage(&e),
    };
    if command != "review" && !line.free.is_empty() {
        return usage(&format!("{command} takes no value {}", line.free[0]));
    }
    let answer = config(&line).and_then(|c| Library::open(c).map_err(Failure::from)).and_then(|lib| match command.as_str() {
        "recordings" => recordings(&lib),
        "scenarios" => Ok(lib.scenarios()),
        "lookup" => Ok(lookup(&lib, &line)),
        _ => review_video(&lib, &line),
    });
    match answer {
        Ok(v) => {
            print(&v);
            ExitCode::SUCCESS
        }
        Err(f) => {
            print(&json!({ "error": f.message, "status": f.status }));
            ExitCode::FAILURE
        }
    }
}
