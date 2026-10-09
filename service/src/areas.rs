//! The areas a review leaves out (python/retired/server.py: exclude, set_exclude, kinds, save_kind, find_areas,
//! labelled): a webcam, another player's overlay.
//!
//! In: the page's areas for a recording, its new and renamed area kinds, and an area_examples.jsonl it loads. Kept
//! (store.rs): each recording's areas (exclude.json: [x0, y0, x1, y1, kind id], shares of the frame); for an added
//! recording without its own, the ones last saved for one (exclude_uploads.json), else KovOBS's layout. The kinds an
//! area can be are area_kinds.json; the area finder (finder.rs, the core's src/areas.rs) learns from the saved areas
//! into area_examples.jsonl (python/areas.py). Out: the areas the review tracks with (a review tracked with other
//! areas is made again), and the areas the finder proposes.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use aimview::areas::{self as finder_areas, Area, Examples, Labelled, Maps, SavedBox};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::finder;
use crate::library::slug as recording_slug;
use crate::library::{Answer, Failure, Library, is_upload};
use crate::pyjson;
use crate::review::AreaBox;
use crate::store::{Item, Mark, Part, ReviewBy};

/// The name messages give the area kinds.
const AREA_KINDS: &str = Item::AreaKinds.file_name();
/// The name messages give the area finder's examples.
const AREA_EXAMPLES: &str = Item::AreaExamples.file_name();
/// An area the user removed: an example of "not an area" (python/areas.py: NONE).
const NONE: &str = "none";
/// The kind of an area whose kind is not known.
const OTHER_KIND: &str = "other";
/// The longest kind name kept, in characters (python/retired/server.py cut them there).
const MAX_KIND_NAME_CHARS: usize = 40;
/// The longest kind description kept, in characters.
const MAX_KIND_ABOUT_CHARS: usize = 200;
/// How far apart two areas' edges (shares of the frame) can be and still be the same: JSON can give an edge back a
/// last place off.
const SAME_EDGE_TOLERANCE: f64 = 1e-9;
/// What an example learned from KovOBS's layout, not from one of the user's recordings, has as its recording.
const KOVOBS_EXAMPLE_PREFIX: &str = "kovobs:";

/// The kinds an area can be, built in (python/retired/review.py: EXCLUDE_KINDS), and what each is
/// (python/retired/server.py: KIND_ABOUT).
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

/// What each of KovOBS's areas is (python/retired/review.py: OVERLAY_KINDS), in the order of
/// aimview::geometry::OVERLAY.
const OVERLAY_KINDS: [&str; 8] =
    ["Session stats", "Timer", "Clock", "Settings", "Weapon", "Scenario name", "Webcam", "Version"];

/// A kind of area: its id never changes; its name and what it is can.
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "AreaKind"))]
pub struct Kind {
    /// The kind's id, which saved areas and examples hold ("session_stats").
    id: String,
    /// The name the page shows.
    name: String,
    /// What it is, in a few words.
    about: String,
}

/// A kind as area_kinds.json kept it before kinds had ids.
#[derive(Deserialize)]
struct OldKind {
    /// Its id; None in a list kept before kinds had ids.
    id: Option<String>,
    /// Its name.
    name: String,
    /// What it is: any JSON value, read as Python's `str` reads it.
    #[serde(default)]
    about: Value,
}

/// One of the finder's examples (area_examples.jsonl): the recording, the area's features, its kind.
#[derive(Serialize, Deserialize)]
struct Example {
    /// The recording it came from, or "kovobs:..." for one learned from KovOBS's layout.
    rec: String,
    /// The area's features, as the core's finder reads them.
    feat: Value,
    /// The kind's id, a name in an example kept before kinds had ids, or "none" for not an area.
    kind: Value,
    /// Any other fields, kept as they are.
    #[serde(flatten)]
    rest: Map<String, Value>,
}

/// KovOBS's layout with each area's kind by name (python/retired/review.py: OVERLAY_SHARES).
fn overlay_shares() -> Vec<Value> {
    aimview::geometry::overlay_shares()
        .iter()
        .zip(OVERLAY_KINDS)
        .map(|(edges, kind)| json!([edges[0], edges[1], edges[2], edges[3], kind]))
        .collect()
}

/// KovOBS's layout as the review takes it (kinds by id), for a review made outside the app.
pub fn kovobs_areas() -> Vec<AreaBox> {
    aimview::geometry::overlay_shares()
        .iter()
        .zip(OVERLAY_KINDS)
        .map(|(edges, kind)| (edges[0], edges[1], edges[2], edges[3], slug(kind)))
        .collect()
}

/// A kind's id from its name (python/retired/server.py: `_slug`): its ASCII letters and digits in lower case, each run
/// of other characters as one "_", none at the ends; "type" when nothing is left.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for character in name.to_lowercase().chars() {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            out.push(character);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_matches('_').to_string();
    if out.is_empty() { "type".into() } else { out }
}

/// A new kind's id (python/retired/server.py: `_new_id`): the name's slug, with a number when another kind has it.
fn new_id(name: &str, kinds: &[Kind]) -> String {
    let base = slug(name);
    let (mut out, mut number) = (base.clone(), 2);
    while kinds.iter().any(|kind| kind.id == out) {
        out = format!("{base}_{number}");
        number += 1;
    }
    out
}

/// Python's `str(value or "")`.
fn text(value: &Value) -> String {
    match value {
        Value::Null | Value::Bool(false) => String::new(),
        Value::Bool(true) => "True".into(),
        Value::String(string) => string.clone(),
        Value::Number(number) if number.as_f64() == Some(0.0) => String::new(),
        Value::Number(number) => number
            .as_i64()
            .map_or_else(|| pyjson::float_repr(number.as_f64().unwrap_or(0.0)), |integer| integer.to_string()),
        Value::Array(items) if items.is_empty() => String::new(),
        Value::Object(fields) if fields.is_empty() => String::new(),
        other => other.to_string(),
    }
}

/// A box's first four values (x0, y0, x1, y1) as numbers.
fn rect(area: &Value) -> Option<[f64; 4]> {
    let values = area.as_array()?;
    let edge = |i: usize| values.get(i)?.as_f64();
    Some([edge(0)?, edge(1)?, edge(2)?, edge(3)?])
}

/// Whether two sets of areas are the same (as JSON gives them back: a last place can differ).
fn same_rects(a: &[[f64; 4]], b: &[[f64; 4]]) -> bool {
    let same_rect = |(rect_a, rect_b): (&[f64; 4], &[f64; 4])| {
        rect_a.iter().zip(rect_b).all(|(edge_a, edge_b)| (edge_a - edge_b).abs() < SAME_EDGE_TOLERANCE)
    };
    a.len() == b.len() && a.iter().zip(b).all(same_rect)
}

/// Whether a saved area is [x0, y0, x1, y1] or [x0, y0, x1, y1, kind id] with numbers for its edges, as shares of the
/// frame with x0 < x1 and y0 < y1.
fn valid_box(area: &Value) -> bool {
    let Some(values) = area.as_array() else { return false };
    let Some([x0, y0, x1, y1]) = rect(area) else { return false };
    (values.len() == 4 || (values.len() == 5 && values[4].is_string()))
        && values[..4].iter().all(Value::is_number)
        && 0.0 <= x0
        && x0 < x1
        && x1 <= 1.0
        && 0.0 <= y0
        && y0 < y1
        && y1 <= 1.0
}

/// The areas a review was tracked with, from its tracks (tracks.json's `areas`; a review made before it kept them was
/// tracked with KovOBS's layout); None when they are not tracks.
pub fn tracked_areas(tracks: &[u8]) -> Option<Vec<[f64; 4]>> {
    let tracks: Value = serde_json::from_slice(tracks).ok()?;
    Some(match tracks["areas"].as_array() {
        Some(boxes) => boxes.iter().filter_map(rect).collect(),
        None => aimview::geometry::overlay_shares().to_vec(),
    })
}

/// area_kinds.json kept before kinds had ids, given ids (python/retired/server.py: kinds): the built-in kinds first,
/// with the user's description of one where they gave one, then the user's own kinds (a later one of a name takes the
/// earlier one's place).
fn kinds_with_ids(kept: Vec<OldKind>) -> Vec<Kind> {
    let mut own: Vec<(String, OldKind)> = Vec::new();
    for kind in kept {
        let name = kind.name.to_lowercase();
        match own.iter_mut().find(|(own_name, _)| *own_name == name) {
            Some(slot) => slot.1 = kind,
            None => own.push((name, kind)),
        }
    }
    let mut out: Vec<Kind> = Vec::new();
    for (name, about) in BUILT_IN {
        let mine = own.iter().position(|(own_name, _)| *own_name == name.to_lowercase()).map(|i| own.remove(i).1);
        let theirs = mine.map(|kind| text(&kind.about)).filter(|their_about| !their_about.is_empty());
        out.push(Kind { id: slug(name), name: name.into(), about: theirs.unwrap_or_else(|| about.into()) });
    }
    for (_, kind) in own {
        let id = new_id(&kind.name, &out);
        let about = if kind.about.is_null() { String::new() } else { text(&kind.about) };
        out.push(Kind { id, name: kind.name, about });
    }
    out
}

impl Library {
    /// The area kinds, built-in ones first (python/retired/server.py: kinds). The first use, or a list kept before
    /// kinds had ids, writes the list with ids.
    pub fn kinds(&self) -> Answer<Vec<Kind>> {
        let kept: Vec<OldKind> = match pyjson::load(self.store(), Item::AreaKinds) {
            Some(value) => serde_json::from_value(value).map_err(|error| format!("{AREA_KINDS}: {error}"))?,
            None if self.store().has(Item::AreaKinds) => return Err(format!("{AREA_KINDS} is not JSON").into()),
            None => Vec::new(),
        };
        if !kept.is_empty() && kept.iter().all(|kind| kind.id.is_some()) {
            return Ok(kept
                .into_iter()
                .map(|kind| Kind { id: kind.id.unwrap_or_default(), name: kind.name, about: text(&kind.about) })
                .collect());
        }
        let kinds = kinds_with_ids(kept);
        pyjson::dump(self.store(), Item::AreaKinds, &kinds, true)?;
        Ok(kinds)
    }

    /// A kind's id from its id or its name (areas saved before kinds had ids hold names); unknown: "other".
    fn kind_id(given: &Value, kinds: &[Kind]) -> String {
        if let Some(id) = given.as_str()
            && kinds.iter().any(|kind| kind.id == id)
        {
            return id.to_string();
        }
        let name = match given {
            Value::String(name) => name.to_lowercase(),
            other => text(other).to_lowercase(),
        };
        kinds
            .iter()
            .find(|kind| kind.name.to_lowercase() == name)
            .map_or_else(|| OTHER_KIND.into(), |kind| kind.id.clone())
    }

    /// A new kind (no id), or a kind's new name and description; answers every kind.
    pub fn save_kind(&self, body: &Value) -> Answer<Value> {
        let name: String = text(&body["name"]).trim().chars().take(MAX_KIND_NAME_CHARS).collect();
        let about: String = text(&body["about"]).trim().chars().take(MAX_KIND_ABOUT_CHARS).collect();
        if name.is_empty() {
            return Err(Failure::bad("a type needs a name"));
        }
        let id = text(&body["id"]);
        let mut kinds = self.kinds()?;
        if kinds.iter().any(|kind| kind.name.to_lowercase() == name.to_lowercase() && kind.id != id) {
            return Err(Failure::bad(format!("there is a type called {name} already")));
        }
        if id.is_empty() {
            let id = new_id(&name, &kinds);
            kinds.push(Kind { id, name, about });
        } else {
            let kind = kinds
                .iter_mut()
                .find(|kind| kind.id == id)
                .ok_or_else(|| Failure::bad(format!("no type with the id {id}")))?;
            (kind.name, kind.about) = (name, about);
        }
        pyjson::dump(self.store(), Item::AreaKinds, &kinds, true)?;
        Ok(json!(kinds))
    }

    /// Areas with their kind as an id ([x0, y0, x1, y1, id]).
    fn with_ids(&self, boxes: &[Value]) -> Answer<Vec<Value>> {
        let kinds = self.kinds()?;
        Ok(boxes
            .iter()
            .map(|area| {
                let values = area.as_array().cloned().unwrap_or_default();
                let mut out: Vec<Value> = values.iter().take(4).cloned().collect();
                out.push(json!(Library::kind_id(values.get(4).unwrap_or(&json!(OTHER_KIND)), &kinds)));
                Value::Array(out)
            })
            .collect())
    }

    /// The areas a review of the recording leaves out, and where they come from: saved for it, else for an added
    /// recording the ones last saved for one, else KovOBS's layout.
    pub fn exclude(&self, id: &str) -> Answer<Value> {
        let read = |item: Item<'_>| -> Answer<Vec<Value>> {
            let value =
                pyjson::load(self.store(), item).ok_or_else(|| format!("{} is not JSON", self.store().name(item)))?;
            Ok(value.as_array().cloned().unwrap_or_default())
        };
        let (saved, uploads) = (Item::Mark(id, Mark::SavedAreas), Item::UploadAreas);
        let (boxes, source) = if self.store().has(saved) {
            (read(saved)?, "saved")
        } else if is_upload(id) && self.store().has(uploads) {
            (read(uploads)?, "last upload")
        } else {
            (overlay_shares(), "kovobs")
        };
        Ok(json!({ "boxes": self.with_ids(&boxes)?, "source": source }))
    }

    /// The recording's areas as the review takes them.
    pub fn exclude_boxes(&self, id: &str) -> Answer<Vec<AreaBox>> {
        let answer = self.exclude(id)?;
        let area_box = |area: &Value| {
            let [x0, y0, x1, y1] = rect(area)?;
            Some((x0, y0, x1, y1, area[4].as_str().unwrap_or(OTHER_KIND).to_string()))
        };
        Ok(answer["boxes"].as_array().into_iter().flatten().filter_map(area_box).collect())
    }

    /// The recording's areas, shares of the frame [x0, y0, x1, y1]; KovOBS's layout when they cannot be read.
    pub fn exclude_areas(&self, id: &str) -> Vec<[f64; 4]> {
        match self.exclude_boxes(id) {
            Ok(boxes) => boxes.iter().map(|(x0, y0, x1, y1, _)| [*x0, *y0, *x1, *y1]).collect(),
            Err(_) => aimview::geometry::overlay_shares().to_vec(),
        }
    }

    /// Whether the recording's review `by` was tracked with the recording's areas.
    pub fn tracked_with_areas(&self, id: &str, by: &ReviewBy) -> bool {
        let tracked = self.review_part(id, by, Part::Tracks).and_then(|tracks| tracked_areas(&tracks));
        tracked.is_some_and(|tracked| same_rects(&tracked, &self.exclude_areas(id)))
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
        let boxes = match body.as_array() {
            Some(list) if list.iter().all(valid_box) => self.with_ids(list)?,
            _ => return Err(Failure::bad("boxes: a list of [x0, y0, x1, y1, type id] (shares of the frame)")),
        };
        pyjson::dump(self.store(), Item::Mark(id, Mark::SavedAreas), &boxes, false)?;
        if is_upload(id) {
            pyjson::dump(self.store(), Item::UploadAreas, &boxes, false)?;
        }
        // the finder learns in the background: finding the areas reads the recording when they are not kept yet
        #[cfg(feature = "native")]
        {
            let (library, recording, saved) = (self.clone(), id.to_string(), boxes.clone());
            std::thread::spawn(move || {
                if let Err(failure) = library.learn(&recording, &saved) {
                    eprintln!("the area finder could not learn from {recording}: {}", failure.message);
                }
            });
        }
        // the browser build learns now, from the found areas the page sent (/api/found); with none kept it only saves
        #[cfg(not(feature = "native"))]
        let learned = self.learn(id, &boxes).is_ok();
        let job = self.areas_job(id)?;
        let mut out = self.exclude(id)?;
        out["job"] = job;
        #[cfg(not(feature = "native"))]
        {
            out["learned"] = json!(learned);
        }
        Ok(out)
    }

    /// The review job once the recording's areas changed: none without a review, done when the review was tracked with
    /// these areas, else the review made again.
    fn areas_job(self: &Arc<Self>, id: &str) -> Answer<Value> {
        let (model, by) = self.shown(id);
        if !self.store().has(Item::ReviewPart(id, &by, Part::Tracks)) {
            Ok(json!({ "stage": "none" }))
        } else if self.tracked_with_areas(id, &by) {
            Ok(json!({ "stage": "done", "done": 0, "total": 1, "model": model }))
        } else {
            self.analyse(id, true)
        }
    }

    /// The recordings the user saved areas for: (its slug, its found areas, its saved areas), leaving out `but` and
    /// other games (python/retired/server.py: labelled, by its folder's name).
    pub fn labelled(&self, but: Option<&str>) -> Vec<(String, Value, Value)> {
        let skip: BTreeSet<String> =
            self.not_aim().iter().map(|id| recording_slug(id)).chain(but.map(recording_slug)).collect();
        let parsed = |(name, found, saved): (String, Vec<u8>, Vec<u8>)| {
            Some((name, pyjson::parse(&found).ok()?, pyjson::parse(&saved).ok()?))
        };
        self.store().labelled(&skip).into_iter().filter_map(parsed).collect()
    }

    /// The areas to propose (python/retired/server.py: find_areas): the user's own areas from a recording with the same
    /// layout (unless `copy` is off), else the areas found in this one, named by what was learned or by rules.
    pub fn find_areas(&self, id: &str, copy: bool) -> Answer<Value> {
        let video = self.resolve(id)?;
        let found = self.found_areas(id, &video, false)?.0;
        let labelled: Vec<Labelled> = if copy {
            self.labelled(Some(id))
                .into_iter()
                .filter_map(|(rec, found, saved)| {
                    let (found, saved) = (serde_json::from_value(found).ok()?, serde_json::from_value(saved).ok()?);
                    Some(Labelled { rec, found, saved })
                })
                .collect()
        } else {
            Vec::new()
        };
        let examples = Examples::Lines(self.examples_lines()).list();
        let proposal = finder_areas::find(&found, &examples, &labelled);
        let boxes: Vec<Value> = proposal.boxes.iter().map(|area| json!(area)).collect();
        let recordings: BTreeSet<&str> = examples
            .iter()
            .map(|example| example.rec.as_str())
            .filter(|recording| !recording.starts_with(KOVOBS_EXAMPLE_PREFIX))
            .collect();
        Ok(json!({
            "boxes": self.with_ids(&boxes)?, "examples": examples.len(), "recordings": recordings.len(),
            "copied": proposal.copied, "by": proposal.by,
        }))
    }

    /// The areas the finder found in the recording, and with `with_maps` its maps: as kept in its folder, else found
    /// now and kept (python/areas.py: find, maps). In the browser build the page finds them: with none kept the
    /// answer is `FOUND_NEEDED` (409).
    fn found_areas(&self, id: &str, video: &Path, with_maps: bool) -> Answer<(Vec<Area>, Option<Maps>)> {
        let store = self.store();
        let (found, maps) = (finder::found(store, id), if with_maps { finder::maps(store, id) } else { None });
        if let Some(found) = &found
            && (!with_maps || maps.is_some())
        {
            return Ok((found.clone(), maps));
        }
        #[cfg(not(feature = "native"))]
        {
            let _ = video;
            Err(Failure {
                status: crate::library::FOUND_NEEDED,
                message: "the recording's areas are not found yet".into(),
            })
        }
        #[cfg(feature = "native")]
        {
            finder::keep(store, id, &finder::analyse(video)?)?;
            let missing = || Failure::from("the found areas could not be kept".to_string());
            let found = finder::found(store, id).ok_or_else(missing)?;
            Ok((found, if with_maps { Some(finder::maps(store, id).ok_or_else(missing)?) } else { None }))
        }
    }

    /// The finder learns from the areas saved for a recording (python/areas.py: learn): each saved area is an example
    /// of its kind, each found area in none of them an example of "not an area"; they replace the recording's earlier
    /// examples in area_examples.jsonl.
    fn learn(&self, id: &str, saved: &[Value]) -> Answer<()> {
        let video = self.resolve(id)?;
        let (found, maps) = self.found_areas(id, &video, true)?;
        let saved: Vec<SavedBox> = saved.iter().filter_map(|area| serde_json::from_value(area.clone()).ok()).collect();
        let new = finder_areas::learn(id, &found, &saved, maps.as_ref());
        let _one_at_a_time = self.examples_lock();
        let lines = self.examples_lines();
        Ok(pyjson::write_text(self.store(), Item::AreaExamples, finder_areas::merge(&lines, id, &new).as_bytes())?)
    }

    /// The area finder's examples (area_examples.jsonl) as text; empty when there are none or they are not UTF-8.
    fn examples_lines(&self) -> String {
        let bytes = self.store().read(Item::AreaExamples).ok().flatten().unwrap_or_default();
        String::from_utf8(bytes).unwrap_or_default()
    }

    /// GET /api/area_examples: area_examples.jsonl as it is kept; empty when there is none.
    pub fn examples_text(&self) -> Answer<Vec<u8>> {
        match self.store().read(Item::AreaExamples) {
            Ok(bytes) => Ok(bytes.unwrap_or_default()),
            Err(error) => Err(format!("{AREA_EXAMPLES}: {error}").into()),
        }
    }

    /// GET /api/area_kinds_file: area_kinds.json as kept, for the page to download; a 404 when none is (the page
    /// offers the built-in types then).
    pub fn kinds_file(&self) -> Answer<Vec<u8>> {
        match self.store().read(Item::AreaKinds) {
            Ok(Some(bytes)) => Ok(bytes),
            Ok(None) => Err(Failure::missing(format!("no {AREA_KINDS}"))),
            Err(error) => Err(format!("{AREA_KINDS}: {error}").into()),
        }
    }

    /// POST /api/area_kinds_file: area_kinds.json replaced by `body` (the page's file of area types), once it reads
    /// as a list of them; else 400 and nothing changes. Answers how many types it holds.
    pub fn set_kinds_file(&self, body: &[u8]) -> Answer<Value> {
        let bad = |reason: String| Failure::bad(format!("{AREA_KINDS}: {reason}"));
        let kinds: Vec<OldKind> = serde_json::from_value(pyjson::parse(body).map_err(bad)?)
            .map_err(|error| Failure::bad(format!("{AREA_KINDS}: {error}")))?;
        pyjson::write_text(self.store(), Item::AreaKinds, body)?;
        Ok(json!({ "kinds": kinds.len() }))
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
            let bad = |reason: String| Failure::bad(format!("{AREA_EXAMPLES}, line {}: {reason}", i + 1));
            let value = pyjson::parse(line.as_bytes()).map_err(bad)?;
            serde_json::from_value::<Example>(value).map_err(|error| bad(error.to_string()))?;
            out.extend_from_slice(line.as_bytes());
            out.push(b'\n');
            count += 1;
        }
        {
            let _one_at_a_time = self.examples_lock();
            pyjson::write_text(self.store(), Item::AreaExamples, &out)?;
        }
        self.fix_examples()?;
        Ok(json!({ "examples": count }))
    }

    /// Examples kept before kinds had ids hold the kind's name: their kinds as ids (python/retired/server.py: Library's
    /// start). Nothing changes when they have ids.
    pub fn fix_examples(&self) -> Answer<()> {
        let Ok(Some(text)) = self.store().read(Item::AreaExamples) else { return Ok(()) };
        let mut examples: Vec<Example> = Vec::new();
        for line in text.split(|&byte| byte == b'\n') {
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let value = pyjson::parse(line).map_err(|error| format!("{AREA_EXAMPLES}: {error}"))?;
            examples.push(serde_json::from_value(value).map_err(|error| format!("{AREA_EXAMPLES}: {error}"))?);
        }
        let kinds = self.kinds()?;
        let mut changed = false;
        for example in &mut examples {
            if example.kind != NONE {
                let id = json!(Library::kind_id(&example.kind, &kinds));
                changed |= id != example.kind;
                example.kind = id;
            }
        }
        if changed {
            let mut out = Vec::new();
            for example in &examples {
                out.extend(pyjson::to_vec(example, false));
                out.push(b'\n');
            }
            pyjson::write_text(self.store(), Item::AreaExamples, &out)?;
        }
        Ok(())
    }
}

/// The kinds' ids.
#[cfg(test)]
mod tests {
    use super::*;

    /// Kind ids are made, numbered and looked up by id or name as Python made them; an unknown name is "other".
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
