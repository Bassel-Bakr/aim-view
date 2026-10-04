//! The areas a review leaves out (python/server.py: exclude, set_exclude, kinds, save_kind, find_areas, labelled): a
//! webcam, another player's overlay. Each recording's areas are kept in its folder (exclude.json: [x0, y0, x1, y1,
//! kind id], shares of the frame); an added recording without its own takes the ones last saved for one
//! (exclude_uploads.json), else KovOBS's layout. The kinds an area can be are area_kinds.json; the area finder learns
//! from the saved areas (area_examples.jsonl, python/areas.py). The review tracks with the recording's areas, and a
//! review tracked with other areas is made again.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use aimview::areas::{self as finder_areas, Area, Examples, Labelled, Maps, SavedBox};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::finder;
use crate::library::{Answer, Failure, Library};
use crate::pyjson;
use crate::review::AreaBox;

const EXCLUDE: &str = "exclude.json";
const EXCLUDE_UPLOADS: &str = "exclude_uploads.json";
const AREA_KINDS: &str = "area_kinds.json";
const AREA_EXAMPLES: &str = "area_examples.jsonl";
/// The finder's areas found in a recording, kept in its folder (finder.rs).
const FOUND: &str = "areas.json";
/// An area the user removed: an example of "not an area" (python/areas.py: NONE).
const NONE: &str = "none";

/// One change to area_examples.jsonl at a time (the finder learns in the background).
static EXAMPLES: Mutex<()> = Mutex::new(());

/// The kinds an area can be, built in (python/review.py: EXCLUDE_KINDS), and what each is (python/server.py:
/// KIND_ABOUT).
const BUILT_IN: [(&str, &str); 11] = [
    ("Session stats", "KovaaK's SESSION box (kills, accuracy, damage), or a game's score and accuracy boxes"),
    ("Timer", "the run's time left"),
    ("Clock", "the time of day, a session clock, FPS"),
    ("Scenario name", "the scenario's name"),
    ("Magazine", "the ammo count"),
    ("Weapon", "the weapon's name or model"),
    ("Settings", "a box of settings (sensitivity, FOV, theme, sounds)"),
    ("Webcam", "a hand cam, face cam or avatar"),
    ("Zoomed crosshair", "a magnified view of the screen round the crosshair"),
    ("Version", "a version number"),
    ("Other", "anything else that is not the game"),
];

/// What each of KovOBS's areas is (python/review.py: OVERLAY_KINDS), in the order of aimview::geometry::OVERLAY.
const OVERLAY_KINDS: [&str; 8] = ["Session stats", "Timer", "Clock", "Settings", "Weapon", "Scenario name", "Webcam", "Version"];

/// A kind of area: its id never changes; its name and what it is can.
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "AreaKind"))]
pub struct Kind {
    id: String,
    name: String,
    about: String,
}

/// A kind as area_kinds.json kept it before kinds had ids.
#[derive(Deserialize)]
struct OldKind {
    id: Option<String>,
    name: String,
    #[serde(default)]
    about: Value,
}

/// One of the finder's examples (area_examples.jsonl): the recording, the area's features, its kind.
#[derive(Serialize, Deserialize)]
struct Example {
    rec: String,
    feat: Value,
    kind: Value,
    #[serde(flatten)]
    rest: Map<String, Value>,
}

/// KovOBS's layout with each area's kind by name (python/review.py: OVERLAY_SHARES).
fn overlay_shares() -> Vec<Value> {
    aimview::geometry::overlay_shares().iter().zip(OVERLAY_KINDS).map(|(b, kind)| json!([b[0], b[1], b[2], b[3], kind])).collect()
}

/// KovOBS's layout as the review takes it (kinds by id), for a review made outside the app.
pub fn kovobs_areas() -> Vec<AreaBox> {
    aimview::geometry::overlay_shares()
        .iter()
        .zip(OVERLAY_KINDS)
        .map(|(b, kind)| (b[0], b[1], b[2], b[3], slug(kind)))
        .collect()
}

/// python/server.py: `_slug`.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_matches('_').to_string();
    if out.is_empty() { "type".into() } else { out }
}

/// python/server.py: `_new_id`: the name's slug, with a number when another kind has it.
fn new_id(name: &str, kinds: &[Kind]) -> String {
    let base = slug(name);
    let (mut out, mut n) = (base.clone(), 2);
    while kinds.iter().any(|k| k.id == out) {
        out = format!("{base}_{n}");
        n += 1;
    }
    out
}

/// Python's `str(v or "")`.
fn text(v: &Value) -> String {
    match v {
        Value::Null | Value::Bool(false) => String::new(),
        Value::Bool(true) => "True".into(),
        Value::String(s) => s.clone(),
        Value::Number(n) if n.as_f64() == Some(0.0) => String::new(),
        Value::Number(n) => n.as_i64().map_or_else(|| pyjson::float_repr(n.as_f64().unwrap_or(0.0)), |i| i.to_string()),
        Value::Array(a) if a.is_empty() => String::new(),
        Value::Object(o) if o.is_empty() => String::new(),
        v => v.to_string(),
    }
}

/// A box's first four values as numbers.
fn rect(b: &Value) -> Option<[f64; 4]> {
    let a = b.as_array()?;
    let v = |i: usize| a.get(i)?.as_f64();
    Some([v(0)?, v(1)?, v(2)?, v(3)?])
}

/// Whether two sets of areas are the same (as JSON gives them back: a last place can differ).
fn same_rects(a: &[[f64; 4]], b: &[[f64; 4]]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.iter().zip(y).all(|(p, q)| (p - q).abs() < 1e-9))
}

/// The areas a review in `dir` was tracked with (tracks.json's `areas`; a review made before it kept them was
/// tracked with KovOBS's layout); None without tracks.
pub fn tracked_areas(dir: &Path) -> Option<Vec<[f64; 4]>> {
    let tracks: Value = serde_json::from_slice(&crate::disk::read(dir.join("tracks.json")).ok()?).ok()?;
    Some(match tracks["areas"].as_array() {
        Some(boxes) => boxes.iter().filter_map(rect).collect(),
        None => aimview::geometry::overlay_shares().to_vec(),
    })
}

impl Library {
    /// The area kinds, built-in ones first (python/server.py: kinds). The first use, or a list kept before kinds had
    /// ids, writes the list with ids.
    pub fn kinds(&self) -> Answer<Vec<Kind>> {
        let p = self.file(AREA_KINDS);
        let data: Vec<OldKind> = match pyjson::load(&p) {
            Some(v) => serde_json::from_value(v).map_err(|e| format!("{AREA_KINDS}: {e}"))?,
            None if crate::disk::exists(&p) => return Err(format!("{AREA_KINDS} is not JSON").into()),
            None => Vec::new(),
        };
        if !data.is_empty() && data.iter().all(|k| k.id.is_some()) {
            return Ok(data
                .into_iter()
                .map(|k| Kind { id: k.id.unwrap_or_default(), name: k.name, about: text(&k.about) })
                .collect());
        }
        // the user's own kinds by name (a later one of the same name takes the earlier one's place)
        let mut own: Vec<(String, OldKind)> = Vec::new();
        for k in data {
            match own.iter_mut().find(|(n, _)| *n == k.name.to_lowercase()) {
                Some(slot) => slot.1 = k,
                None => own.push((k.name.to_lowercase(), k)),
            }
        }
        let mut out: Vec<Kind> = Vec::new();
        for (name, about) in BUILT_IN {
            let mine = own.iter().position(|(n, _)| *n == name.to_lowercase()).map(|i| own.remove(i).1);
            let theirs = mine.map(|k| text(&k.about)).filter(|a| !a.is_empty());
            out.push(Kind { id: slug(name), name: name.into(), about: theirs.unwrap_or_else(|| about.into()) });
        }
        for (_, k) in own {
            let id = new_id(&k.name, &out);
            out.push(Kind { id, name: k.name, about: if k.about.is_null() { String::new() } else { text(&k.about) } });
        }
        pyjson::dump(&p, &out, true)?;
        Ok(out)
    }

    /// A kind's id from its id or its name (areas saved before kinds had ids hold names); unknown: "other".
    fn kind_id(v: &Value, kinds: &[Kind]) -> String {
        if let Some(s) = v.as_str()
            && kinds.iter().any(|k| k.id == s)
        {
            return s.to_string();
        }
        let name = match v {
            Value::String(s) => s.to_lowercase(),
            v => text(v).to_lowercase(),
        };
        kinds.iter().find(|k| k.name.to_lowercase() == name).map_or_else(|| "other".into(), |k| k.id.clone())
    }

    /// A new kind (no id), or a kind's new name and description; answers every kind.
    pub fn save_kind(&self, body: &Value) -> Answer<Value> {
        let name: String = text(&body["name"]).trim().chars().take(40).collect();
        let about: String = text(&body["about"]).trim().chars().take(200).collect();
        if name.is_empty() {
            return Err(Failure::bad("a type needs a name"));
        }
        let id = text(&body["id"]);
        let mut kinds = self.kinds()?;
        if kinds.iter().any(|k| k.name.to_lowercase() == name.to_lowercase() && k.id != id) {
            return Err(Failure::bad(format!("there is a type called {name} already")));
        }
        if id.is_empty() {
            let id = new_id(&name, &kinds);
            kinds.push(Kind { id, name, about });
        } else {
            let k = kinds.iter_mut().find(|k| k.id == id).ok_or_else(|| Failure::bad(format!("no type with the id {id}")))?;
            (k.name, k.about) = (name, about);
        }
        pyjson::dump(&self.file(AREA_KINDS), &kinds, true)?;
        Ok(json!(kinds))
    }

    /// Areas with their kind as an id ([x0, y0, x1, y1, id]).
    fn with_ids(&self, boxes: &[Value]) -> Answer<Vec<Value>> {
        let kinds = self.kinds()?;
        Ok(boxes
            .iter()
            .map(|b| {
                let a = b.as_array().cloned().unwrap_or_default();
                let mut out: Vec<Value> = a.iter().take(4).cloned().collect();
                out.push(json!(Library::kind_id(a.get(4).unwrap_or(&json!("other")), &kinds)));
                Value::Array(out)
            })
            .collect())
    }

    /// The areas a review of the recording leaves out, and where they come from: saved for it, else for an added
    /// recording the ones last saved for one, else KovOBS's layout.
    pub fn exclude(&self, id: &str) -> Answer<Value> {
        let read = |p: &Path| -> Answer<Vec<Value>> {
            let v = pyjson::load(p).ok_or_else(|| format!("{} is not JSON", p.display()))?;
            Ok(v.as_array().cloned().unwrap_or_default())
        };
        let saved = self.review_dir(id).join(EXCLUDE);
        let uploads = self.file(EXCLUDE_UPLOADS);
        let (boxes, source) = if crate::disk::exists(&saved) {
            (read(&saved)?, "saved")
        } else if id.starts_with("uploads/") && crate::disk::exists(&uploads) {
            (read(&uploads)?, "last upload")
        } else {
            (overlay_shares(), "kovobs")
        };
        Ok(json!({ "boxes": self.with_ids(&boxes)?, "source": source }))
    }

    /// The recording's areas as the review takes them.
    pub fn exclude_boxes(&self, id: &str) -> Answer<Vec<AreaBox>> {
        let ex = self.exclude(id)?;
        Ok(ex["boxes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|b| rect(b).map(|r| (r[0], r[1], r[2], r[3], b[4].as_str().unwrap_or("other").to_string())))
            .collect())
    }

    /// The recording's areas, shares of the frame [x0, y0, x1, y1]; KovOBS's layout when they cannot be read.
    pub fn exclude_areas(&self, id: &str) -> Vec<[f64; 4]> {
        match self.exclude_boxes(id) {
            Ok(boxes) => boxes.iter().map(|b| [b.0, b.1, b.2, b.3]).collect(),
            Err(_) => aimview::geometry::overlay_shares().to_vec(),
        }
    }

    /// Whether the review in `dir` was tracked with the recording's areas.
    pub fn tracked_with_areas(&self, id: &str, dir: &Path) -> bool {
        tracked_areas(dir).is_some_and(|a| same_rects(&a, &self.exclude_areas(id)))
    }

    /// GET /api/exclude: the kinds, and the recording's areas (or with `kovobs`, KovOBS's layout, kinds by name).
    pub fn exclude_answer(&self, id: Option<&str>, kovobs: bool) -> Answer<Value> {
        let kinds = self.kinds()?;
        let mut out = if kovobs {
            json!({ "boxes": overlay_shares(), "source": "kovobs" })
        } else {
            self.exclude(id.ok_or_else(|| Failure::bad("id= is missing"))?)?
        };
        out["kinds"] = json!(kinds);
        Ok(out)
    }

    /// Keeps the recording's areas (the page's JSON: [[x0, y0, x1, y1, kind id], ...], shares of the frame); for an
    /// added recording they are also the next one's default. The finder learns from them, and a review tracked with
    /// other areas is made again (`job`).
    pub fn set_exclude(self: &Arc<Self>, id: &str, body: &[u8]) -> Answer<Value> {
        self.resolve(id)?;
        let body = pyjson::parse(if body.is_empty() { b"null" } else { body }).map_err(Failure::bad)?;
        let fine = |b: &Value| {
            let Some(a) = b.as_array() else { return false };
            let Some(r) = rect(b) else { return false };
            (a.len() == 4 || (a.len() == 5 && a[4].is_string()))
                && a[..4].iter().all(Value::is_number)
                && 0.0 <= r[0]
                && r[0] < r[2]
                && r[2] <= 1.0
                && 0.0 <= r[1]
                && r[1] < r[3]
                && r[3] <= 1.0
        };
        let boxes = match body.as_array() {
            Some(list) if list.iter().all(fine) => self.with_ids(list)?,
            _ => return Err(Failure::bad("boxes: a list of [x0, y0, x1, y1, type id] (shares of the frame)")),
        };
        pyjson::dump(&self.review_dir(id).join(EXCLUDE), &boxes, false)?;
        if id.starts_with("uploads/") {
            pyjson::dump(&self.file(EXCLUDE_UPLOADS), &boxes, false)?;
        }
        // the finder learns in the background: finding the areas reads the recording when they are not kept yet
        #[cfg(feature = "native")]
        {
            let (lib, rec, saved) = (self.clone(), id.to_string(), boxes.clone());
            std::thread::spawn(move || {
                if let Err(e) = lib.learn(&rec, &saved) {
                    eprintln!("the area finder could not learn from {rec}: {}", e.message);
                }
            });
        }
        // the browser build learns now, from the found areas the page sent (/api/found); with none kept it only saves
        #[cfg(not(feature = "native"))]
        let learned = self.learn(id, &boxes).is_ok();
        let (model, dir) = self.shown(id);
        let job = if !crate::disk::is_file(dir.join("tracks.json")) {
            json!({ "stage": "none" })
        } else if self.tracked_with_areas(id, &dir) {
            json!({ "stage": "done", "done": 0, "total": 1, "model": model })
        } else {
            self.analyse(id, true)?
        };
        let mut out = self.exclude(id)?;
        out["job"] = job;
        #[cfg(not(feature = "native"))]
        {
            out["learned"] = json!(learned);
        }
        Ok(out)
    }

    /// The recordings the user saved areas for: (its folder's name, its found areas, its saved areas), leaving out
    /// `but` and other games (python/server.py: labelled).
    pub fn labelled(&self, but: Option<&str>) -> Vec<(String, Value, Value)> {
        let skip: Vec<PathBuf> = self.not_aim().iter().map(|id| self.review_dir(id)).chain(but.map(|id| self.review_dir(id))).collect();
        let mut out = Vec::new();
        for e in crate::disk::read_dir(&self.folders().recordings).into_iter().flatten().flatten() {
            let d = e.path();
            if !e.is_dir() || skip.contains(&d) {
                continue;
            }
            if let (Some(found), Some(saved)) = (pyjson::load(&d.join(FOUND)), pyjson::load(&d.join(EXCLUDE))) {
                out.push((e.file_name().to_string_lossy().into_owned(), found, saved));
            }
        }
        out
    }

    /// The areas to propose (python/server.py: find_areas): the user's own areas from a recording with the same layout
    /// (unless `copy` is off), else the areas found in this one, named by what was learned or by rules.
    pub fn find_areas(&self, id: &str, copy: bool) -> Answer<Value> {
        let video = self.resolve(id)?;
        let found = self.found_areas(id, &video, false)?.0;
        let labelled: Vec<Labelled> = if copy {
            self.labelled(Some(id))
                .into_iter()
                .filter_map(|(rec, f, s)| Some(Labelled { rec, found: serde_json::from_value(f).ok()?, saved: serde_json::from_value(s).ok()? }))
                .collect()
        } else {
            Vec::new()
        };
        let examples = Examples::Lines(crate::disk::read_to_string(self.examples_path()).unwrap_or_default()).list();
        let proposal = finder_areas::find(&found, &examples, &labelled);
        let boxes: Vec<Value> = proposal.boxes.iter().map(|b| json!(b)).collect();
        let recordings: BTreeSet<&str> = examples.iter().map(|e| e.rec.as_str()).filter(|r| !r.starts_with("kovobs:")).collect();
        Ok(json!({
            "boxes": self.with_ids(&boxes)?, "examples": examples.len(), "recordings": recordings.len(),
            "copied": proposal.copied, "by": proposal.by,
        }))
    }

    /// The areas the finder found in the recording, and with `with_maps` its maps: as kept in its folder, else found
    /// now and kept (python/areas.py: find, maps). In the browser build the page finds them: with none kept the
    /// answer is `FOUND_NEEDED` (409).
    fn found_areas(&self, id: &str, video: &Path, with_maps: bool) -> Answer<(Vec<Area>, Option<Maps>)> {
        let dir = self.review_dir(id);
        let (found, maps) = (finder::found(&dir), if with_maps { finder::maps(&dir) } else { None });
        if let Some(found) = &found
            && (!with_maps || maps.is_some())
        {
            return Ok((found.clone(), maps));
        }
        #[cfg(not(feature = "native"))]
        {
            let _ = video;
            Err(Failure { status: crate::library::FOUND_NEEDED, message: "the recording's areas are not found yet".into() })
        }
        #[cfg(feature = "native")]
        {
            finder::keep(&dir, &finder::analyse(video)?)?;
            let missing = || Failure::from("the found areas could not be kept".to_string());
            Ok((finder::found(&dir).ok_or_else(missing)?, if with_maps { Some(finder::maps(&dir).ok_or_else(missing)?) } else { None }))
        }
    }

    /// The finder learns from the areas saved for a recording (python/areas.py: learn): each saved area is an example
    /// of its kind, each found area in none of them an example of "not an area"; they replace the recording's earlier
    /// examples in area_examples.jsonl.
    fn learn(&self, id: &str, saved: &[Value]) -> Answer<()> {
        let video = self.resolve(id)?;
        let (found, maps) = self.found_areas(id, &video, true)?;
        let saved: Vec<SavedBox> = saved.iter().filter_map(|b| serde_json::from_value(b.clone()).ok()).collect();
        let new = finder_areas::learn(id, &found, &saved, maps.as_ref());
        let _one_at_a_time = EXAMPLES.lock().unwrap_or_else(|e| e.into_inner());
        let p = self.examples_path();
        let lines = crate::disk::read_to_string(&p).unwrap_or_default();
        Ok(pyjson::write_text(&p, finder_areas::merge(&lines, id, &new).as_bytes())?)
    }

    /// The area finder's examples (area_examples.jsonl).
    pub fn examples_path(&self) -> PathBuf {
        self.file(AREA_EXAMPLES)
    }

    /// GET /api/area_examples: area_examples.jsonl as it is kept; empty when there is none.
    pub fn examples_text(&self) -> Answer<Vec<u8>> {
        match crate::disk::read(self.examples_path()) {
            Ok(bytes) => Ok(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(format!("{AREA_EXAMPLES}: {e}").into()),
        }
    }

    /// POST /api/area_examples: area_examples.jsonl replaced by `body` (its text), once each of its lines reads as an
    /// example; else the first line that does not is named (400) and nothing changes. Kinds kept by name become ids
    /// (`fix_examples`). Answers how many examples it holds.
    pub fn set_examples(&self, body: &[u8]) -> Answer<Value> {
        let text = std::str::from_utf8(body).map_err(|_| Failure::bad(format!("{AREA_EXAMPLES}: not UTF-8 text")))?;
        let mut out = Vec::with_capacity(body.len());
        let mut count = 0;
        for (i, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let bad = |e: String| Failure::bad(format!("{AREA_EXAMPLES}, line {}: {e}", i + 1));
            let v = pyjson::parse(line.as_bytes()).map_err(bad)?;
            serde_json::from_value::<Example>(v).map_err(|e| bad(e.to_string()))?;
            out.extend_from_slice(line.as_bytes());
            out.push(b'\n');
            count += 1;
        }
        {
            let _one_at_a_time = EXAMPLES.lock().unwrap_or_else(|e| e.into_inner());
            pyjson::write_text(&self.examples_path(), &out)?;
        }
        self.fix_examples()?;
        Ok(json!({ "examples": count }))
    }

    /// Examples kept before kinds had ids hold the kind's name: their kinds as ids (python/server.py: Library's
    /// start). Nothing changes when they have ids.
    pub fn fix_examples(&self) -> Answer<()> {
        let p = self.examples_path();
        let Ok(text) = crate::disk::read(&p) else { return Ok(()) };
        let mut examples: Vec<Example> = Vec::new();
        for line in text.split(|&b| b == b'\n') {
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let v = pyjson::parse(line).map_err(|e| format!("{AREA_EXAMPLES}: {e}"))?;
            examples.push(serde_json::from_value(v).map_err(|e| format!("{AREA_EXAMPLES}: {e}"))?);
        }
        let kinds = self.kinds()?;
        let mut changed = false;
        for e in &mut examples {
            if e.kind != NONE {
                let id = json!(Library::kind_id(&e.kind, &kinds));
                changed |= id != e.kind;
                e.kind = id;
            }
        }
        if changed {
            let mut out = Vec::new();
            for e in &examples {
                out.extend(pyjson::to_vec(e, false));
                out.push(b'\n');
            }
            pyjson::write_text(&p, &out)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_as_python_makes_them() {
        assert_eq!(slug("Session stats"), "session_stats");
        assert_eq!(slug("  Zoomed  crosshair!"), "zoomed_crosshair");
        assert_eq!(slug("Ünï"), "n");
        assert_eq!(slug("!!"), "type");
        let kinds = vec![Kind { id: "timer".into(), name: "Timer".into(), about: String::new() }];
        assert_eq!(new_id("Timer", &kinds), "timer_2");
        assert_eq!(Library::kind_id(&json!("Timer"), &kinds), "timer");
        assert_eq!(Library::kind_id(&json!("timer"), &kinds), "timer");
        assert_eq!(Library::kind_id(&json!("Handcam"), &kinds), "other");
    }
}
