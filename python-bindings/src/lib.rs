//! The Python module `aimview`: the review service (service/) and the review core (src/) for Python.
//! python/server.py is a thin HTTP layer over `Library.handle`; the scripts use the library's recordings and stats
//! files; the evaluation scripts use the native review (`Library.review_video`) and the core's (`review_json`). The
//! Rust work runs without Python's lock (the GIL), so a review does not hold up other Python threads.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use aimview::scenario::{Facts, Kind};
use aimview_service::config::{kovaak_stats, KOVAAK_DEFAULT};
use aimview_service::review::{AreaBox, Request, TimeWindow, review};
use aimview_service::{ApiRequest, Config, Device, Failure, Ffmpeg, Layout};
use pyo3::exceptions::{PyFileNotFoundError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict};
use serde_json::Value;

/// The settings a `Library` takes (keyword arguments, or a dict).
const SETTINGS: [&str; 9] = ["data", "layout", "models", "vods", "stats", "scenarios", "device", "ffmpeg", "download_ffmpeg"];

/// An API error as a Python exception: FileNotFoundError (404), ValueError (400) or RuntimeError.
fn failure(f: Failure) -> PyErr {
    match f.status {
        404 => PyFileNotFoundError::new_err(f.message),
        400 => PyValueError::new_err(f.message),
        _ => PyRuntimeError::new_err(f.message),
    }
}

/// JSON as Python's objects, read by Python's json module (as a client reads the API's answers).
fn to_python<'py>(py: Python<'py>, v: &Value) -> PyResult<Bound<'py, PyAny>> {
    let text = serde_json::to_string(v).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    py.import("json")?.call_method1("loads", (text,))
}

/// A library's configuration from its settings: data and models (needed), layout ("python", the default: python/server.py's
/// folders in test_out/; or "app": the desktop app's), vods, stats, scenarios (a list of folders), device ("auto",
/// "directml", "cuda", "cpu"), ffmpeg (a folder holding ffmpeg and ffprobe; None: the PATH) and download_ffmpeg (True:
/// downloaded into that folder when a review first needs it).
fn config(settings: &Bound<'_, PyDict>) -> PyResult<Config> {
    for key in settings.keys() {
        let key: String = key.extract()?;
        if !SETTINGS.contains(&key.as_str()) {
            return Err(PyValueError::new_err(format!("no setting called {key} (the settings: {})", SETTINGS.join(", "))));
        }
    }
    let get = |key: &str| -> PyResult<Option<Bound<'_, PyAny>>> { Ok(settings.get_item(key)?.filter(|v| !v.is_none())) };
    let path = |key: &str| -> PyResult<Option<PathBuf>> { get(key)?.map(|v| v.extract::<PathBuf>()).transpose() };
    let text = |key: &str, default: &str| -> PyResult<String> { Ok(get(key)?.map(|v| v.extract()).transpose()?.unwrap_or(default.into())) };
    let needed = |key: &str| path(key)?.ok_or_else(|| PyValueError::new_err(format!("a library needs {key}=")));
    let layout = match text("layout", "python")?.as_str() {
        "python" => Layout::Python,
        "app" => Layout::App,
        other => return Err(PyValueError::new_err(format!("layout: python or app, not {other}"))),
    };
    let mut c = Config::new(needed("data")?, layout, needed("models")?);
    c.vods = path("vods")?;
    if let Some(stats) = path("stats")? {
        c.stats = stats;
    }
    if let Some(folders) = get("scenarios")? {
        c.scenarios = folders.extract()?;
    }
    c.device = match text("device", "auto")?.as_str() {
        "auto" => Device::Auto,
        "directml" => Device::DirectMl,
        "cuda" => Device::Cuda,
        "cpu" => Device::Cpu,
        other => return Err(PyValueError::new_err(format!("device: auto, directml, cuda or cpu, not {other}"))),
    };
    let download = get("download_ffmpeg")?.map(|v| v.is_truthy()).transpose()?.unwrap_or(false);
    c.ffmpeg = match path("ffmpeg")? {
        None if download => return Err(PyValueError::new_err("download_ffmpeg needs ffmpeg= (the folder to keep it in)")),
        None => Ffmpeg::Path,
        Some(folder) if download => Ffmpeg::Download(folder),
        Some(folder) => Ffmpeg::Folder(folder),
    };
    Ok(c)
}

/// The id of the recording at `video`: its path in the VODs folder ("<folder>/<file>"), or "uploads/<name>"; None for a
/// video outside the library's folders.
fn id_of(lib: &aimview_service::Library, video: &Path) -> Option<String> {
    let video = video.canonicalize().ok()?;
    let inside = |root: Option<PathBuf>| {
        let root = root?.canonicalize().ok()?;
        let rel = video.strip_prefix(root).ok()?;
        Some(rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"))
    };
    let id = inside(lib.vods()).or_else(|| inside(Some(lib.folders().uploads.clone())).map(|n| format!("uploads/{n}")))?;
    lib.resolve(&id).is_ok().then_some(id)
}

/// Areas from Python: [x0, y0, x1, y1] or [x0, y0, x1, y1, kind id], as shares of the frame.
fn area_boxes(list: Vec<Vec<Bound<'_, PyAny>>>) -> PyResult<Vec<AreaBox>> {
    list.into_iter()
        .map(|b| {
            if !(4..=5).contains(&b.len()) {
                return Err(PyValueError::new_err("an area is [x0, y0, x1, y1] or [x0, y0, x1, y1, kind]"));
            }
            let kind = b.get(4).map(|k| k.extract::<String>()).transpose()?.unwrap_or("other".into());
            Ok((b[0].extract()?, b[1].extract()?, b[2].extract()?, b[3].extract()?, kind))
        })
        .collect()
}

fn kind(name: &str) -> PyResult<Kind> {
    match name {
        "static" => Ok(Kind::Static),
        "dynamic" => Ok(Kind::Dynamic),
        "tracking" => Ok(Kind::Tracking),
        "switching" => Ok(Kind::Switching),
        other => Err(PyValueError::new_err(format!("kind: static, dynamic, tracking or switching, not {other}"))),
    }
}

/// A native review's outcome: tracks.json, readings.json and hud.json (their bytes), the review's time in seconds, and
/// the report when one was worked out.
struct Outcome {
    files: Vec<(&'static str, Vec<u8>)>,
    seconds: f64,
    report: Option<Value>,
}

/// The review service's library (aimview_service::Library): the recordings, their stats files, the models, and each
/// recording's reviews and marks in the data folder.
#[pyclass(frozen, module = "aimview", name = "Library")]
struct PyLibrary {
    lib: Arc<aimview_service::Library>,
}

#[pymethods]
impl PyLibrary {
    /// Library(config=None, **settings): the library the settings describe (a dict, keyword arguments, or both): see
    /// `config`.
    #[new]
    #[pyo3(signature = (config = None, **settings))]
    fn new(py: Python<'_>, config: Option<&Bound<'_, PyDict>>, settings: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        let all = PyDict::new(py);
        for part in [config, settings].into_iter().flatten() {
            all.update(part.as_mapping())?;
        }
        let config = self::config(&all)?;
        let lib = py.detach(|| aimview_service::Library::open(config)).map_err(PyRuntimeError::new_err)?;
        Ok(PyLibrary { lib })
    }

    /// handle(method, path_and_query, range=None, body=None) -> (status, headers, body): one request of the review
    /// server's API (/api/... and /video), answered as python/server.py answered it.
    #[pyo3(signature = (method, path_and_query, range = None, body = None))]
    fn handle<'py>(
        &self,
        py: Python<'py>,
        method: &str,
        path_and_query: &str,
        range: Option<&str>,
        body: Option<&[u8]>,
    ) -> (u16, Vec<(String, String)>, Bound<'py, PyBytes>) {
        let req = ApiRequest { method, path_and_query, range, body: body.unwrap_or_default() };
        let res = py.detach(|| aimview_service::handle(&self.lib, &req));
        (res.status, res.headers, PyBytes::new(py, &res.body))
    }

    /// The recordings, newest first (what /api/vods gives).
    fn recordings<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let list = py.detach(|| self.lib.recordings()).map_err(failure)?;
        to_python(py, &list)
    }

    /// A recording's video from its id (its path in the VODs folder, or "uploads/<name>"); FileNotFoundError if none.
    fn resolve(&self, id: &str) -> PyResult<PathBuf> {
        self.lib.resolve(id).map_err(failure)
    }

    /// A recording's folder in the data folder: its reviews, areas and marks.
    fn review_dir(&self, id: &str) -> PathBuf {
        self.lib.review_dir(id)
    }

    /// KovaaK's stats file of a run of `scenario` that ended at `stamp` (a file-name time stamp), within 5 s; or None.
    fn stats_for(&self, py: Python<'_>, scenario: &str, stamp: &str) -> Option<PathBuf> {
        py.detach(|| self.lib.stats_for(scenario, stamp))
    }

    /// The stats file of the recording at `video`: the user's choice, else one uploaded beside it, else by name and
    /// time. The service's own `stats_of` is not public: this is its answer for the recording at that path
    /// (`stats_path`; the folder a pairing is kept in depends only on the file's name, as `id`'s does), and by name and
    /// time for a video outside the library's folders.
    fn stats_of(&self, py: Python<'_>, id: &str, video: PathBuf) -> Option<PathBuf> {
        let _ = id;
        py.detach(|| match id_of(&self.lib, &video) {
            Some(id) => self.lib.stats_path(&id),
            None => {
                let name = video.with_extension("mp4").file_name()?.to_string_lossy().into_owned();
                let (scenario, _, stamp) = aimview_service::library::parse_name(&name)?;
                self.lib.stats_for(&scenario, &stamp)
            }
        })
    }

    /// The stats file a recording uses (its id), or None.
    fn stats_path(&self, py: Python<'_>, id: &str) -> Option<PathBuf> {
        py.detach(|| self.lib.stats_path(id))
    }

    /// The VODs folder, or None.
    #[getter]
    fn vods(&self) -> Option<PathBuf> {
        self.lib.vods()
    }

    /// The folders the library keeps its files in: files (settings.json and the lists), recordings, uploads, cutoff
    /// and mouse.
    #[getter]
    fn folders<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let f = self.lib.folders();
        let out = PyDict::new(py);
        for (key, p) in [("files", &f.files), ("recordings", &f.recordings), ("uploads", &f.uploads), ("cutoff", &f.cutoff), ("mouse", &f.mouse)] {
            out.set_item(key, p)?;
        }
        Ok(out)
    }

    /// review_video(video, model=None, out=None, *, stats=None, kind=None, limit=None, cap=None, areas=None, runs=None,
    /// batch=4, window=None, progress=None) -> dict: a video reviewed as the app reviews a recording (ffmpeg's
    /// frames, the core, the detector on the configured device), without touching the library's reviews.
    ///
    /// model: a model's name (its _u8in export in the models folder) or an .onnx file; None: the app's pick. The
    /// scenario's facts: kind ("static", "dynamic", "tracking", "switching"), limit (its time limit, seconds) and cap
    /// (its targets alive at once; None: not known). areas: [[x0, y0, x1, y1, kind], ...] to leave out; None: the
    /// recording's in the app (its saved areas, else KovOBS's layout), KovOBS's for a video outside the library.
    /// runs: the parts reviewed at once (None: 2 with 8 threads or more, as the app). window: (start, end) in seconds,
    /// only that part tracked. progress: called with (stage, done, total).
    ///
    /// Gives {tracks, readings, hud, seconds}: tracks.json's content, the camera's readings, what the HUD read (None:
    /// not read). With `out`, a folder: tracks.json, readings.json and hud.json are written there, and the report the
    /// app shows is worked out from them (with the stats file when one is given): `report`.
    #[pyo3(signature = (video, model = None, out = None, *, stats = None, kind = None, limit = None, cap = None,
                        areas = None, runs = None, batch = 4, window = None, progress = None))]
    #[allow(clippy::too_many_arguments)]
    fn review_video<'py>(
        &self,
        py: Python<'py>,
        video: PathBuf,
        model: Option<&str>,
        out: Option<PathBuf>,
        stats: Option<PathBuf>,
        kind: Option<&str>,
        limit: Option<f64>,
        cap: Option<usize>,
        areas: Option<Vec<Vec<Bound<'py, PyAny>>>>,
        runs: Option<usize>,
        batch: usize,
        window: Option<(f64, f64)>,
        progress: Option<Py<PyAny>>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let lib = &self.lib;
        let model = match model {
            Some(m) if m.to_lowercase().ends_with(".onnx") => PathBuf::from(m),
            Some(name) => lib.model_file(name),
            None => lib.model_file(&lib.model()),
        };
        let areas = match areas {
            Some(list) => area_boxes(list)?,
            None => match id_of(lib, &video) {
                Some(id) => lib.exclude_boxes(&id).map_err(failure)?,
                None => aimview_service::areas::kovobs_areas(),
            },
        };
        let facts = kind.map(self::kind).transpose()?.map(|kind| Facts { kind, limit, targets: cap });
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
        let req = Request {
            video,
            model,
            device: lib.config().device,
            batch,
            cap: cap.unwrap_or(0),
            runs: runs.unwrap_or(if threads >= 8 { 2 } else { 1 }),
            window: window.map(|(start, end)| TimeWindow { start, end }),
            areas,
        };
        let progress = |stage: &str, done: usize, total: usize| {
            if let Some(f) = &progress {
                Python::attach(|py| {
                    if let Err(e) = f.call1(py, (stage, done, total)) {
                        e.print(py);
                    }
                });
            }
        };
        let started = Instant::now();
        let done = py.detach(|| -> Result<Outcome, String> {
            aimview_service::ffmpeg::ensure(|mb, of| progress("ffmpeg", mb, of))?;
            let r = review(&req, &progress)?;
            let seconds = started.elapsed().as_secs_f64();
            let json = |v: Result<Vec<u8>, serde_json::Error>| v.map_err(|e| e.to_string());
            let files = vec![
                ("tracks", json(serde_json::to_vec(&r.tracks))?),
                ("readings", json(serde_json::to_vec(&r.readings))?),
                ("hud", json(serde_json::to_vec(&r.hud))?),
            ];
            let Some(dir) = &out else { return Ok(Outcome { files, seconds, report: None }) };
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            for (name, bytes) in &files {
                let p = dir.join(format!("{name}.json"));
                std::fs::write(&p, bytes).map_err(|e| format!("{}: {e}", p.display()))?;
            }
            let report = aimview_service::report::work_out(dir, &req.video, stats.as_deref(), None, facts.as_ref(), None)?;
            Ok(Outcome { files, seconds, report })
        });
        let Outcome { files, seconds, report } = done.map_err(PyRuntimeError::new_err)?;
        let out = PyDict::new(py);
        let loads = py.import("json")?.getattr("loads")?;
        for (name, bytes) in files {
            out.set_item(name, loads.call1((PyBytes::new(py, &bytes),))?)?;
        }
        out.set_item("seconds", seconds)?;
        if let Some(report) = report {
            out.set_item("report", to_python(py, &report)?)?;
        }
        Ok(out)
    }
}

/// review_json(request) -> str: the core's review of a request (src/review.rs: `review_json`), JSON in and out: the
/// tracks, the stats file's text, the HUD's reading and the camera's, and the user's marks in; {"report": ...} or
/// {"error": ...} out.
#[pyfunction]
fn review_json(py: Python<'_>, request: &str) -> String {
    let request = request.as_bytes();
    let out = py.detach(|| aimview::review::review_json(request));
    String::from_utf8_lossy(&out).into_owned()
}

/// slug(id) -> str: the name of a recording's folder (python/server.py: cache_dir).
#[pyfunction]
fn slug(id: &str) -> String {
    aimview_service::library::slug(id)
}

/// parse_name(name) -> (scenario, score, stamp) or None: a recording's name as KovOBS writes it
/// ("<scenario> - <score> - <yyyy.mm.dd-hh.mm.ss>.mp4").
#[pyfunction]
fn parse_name(name: &str) -> Option<(String, f64, String)> {
    aimview_service::library::parse_name(name)
}

#[pymodule(name = "aimview")]
fn aimview_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyLibrary>()?;
    m.add_function(wrap_pyfunction!(review_json, m)?)?;
    m.add_function(wrap_pyfunction!(slug, m)?)?;
    m.add_function(wrap_pyfunction!(parse_name, m)?)?;
    m.add("KOVAAK_DEFAULT", KOVAAK_DEFAULT)?;
    m.add("STATS_DEFAULT", kovaak_stats(Path::new(KOVAAK_DEFAULT)).to_string_lossy().into_owned())?;
    Ok(())
}
