//! The Crops page's files: the check folders of detector crops (python/model/crop_check/make_page.py writes each: its
//! crops.json, sets.json and crops/<id>.png), the user's answer to each crop (answers/checks/<id>.json, the file the
//! claude.ai check pages' answers were saved to), and the answers moved between modes as one document (browser mode
//! exports it, the review server imports it). The check folders are the data folder's crops/ in the app's layout
//! (browser mode, the desktop app) and test_out/vod_model/check_* in Python's (the review server).
//!
//! In: the page's requests (api.rs /api/crop_*). Out: those files, read by python/model/crop_check/labels.py and
//! aimview-tool crop-labels, which turn a scene into training labels with the core (src/shapes.rs).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use aimview::shapes::{self, Scene};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::library::{Answer, Failure, Library};

/// Where a check folder keeps its answers: the page's own, and older folders the claude.ai pages' answers were saved
/// to (answers*/checks), read too.
const ANSWERS: &str = "answers";
/// The folder of answer files in each answers folder.
const CHECKS: &str = "checks";
/// Where an answer goes when its crop is answered again (beside answers/checks/): nothing the user said is lost.
const REPLACED: &str = "replaced";
/// A crop's side in pixels (make_page.py's crops are 256 x 256).
pub const CROP_PX: usize = 256;
/// The numbers of an added box: [cx, cy, w, h].
const BOX_VALUES: usize = 4;
/// The numbers of a tapped point: [x, y].
const POINT_VALUES: usize = 2;

/// What the user said of a crop: its boxes are right, wrong (and fixed), or it cannot tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum CropVerdict {
    /// The model's boxes are right (with the crossed-out ones crossed out).
    Right,
    /// The boxes are wrong; the answer's marks or scene fix them.
    Wrong,
    /// The user cannot tell.
    Unsure,
}

/// A crop's answer: the verdict, its set and file, when (ms since 1970), the model boxes crossed out, moved or resized,
/// the boxes drawn and points tapped (the claude.ai pages' fields), whether a suggestion was taken as offered, and the
/// scene the Crops page drew (src/shapes.rs), when it drew one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CropAnswer {
    /// Right, wrong or can't tell.
    pub verdict: CropVerdict,
    /// The crop's set; it must be the crop's.
    pub set: String,
    /// The crop's file in the training data (as train/a.npz); it must be the crop's.
    pub file: String,
    /// When it was answered, in ms since 1970: of two answers to a crop the later wins.
    pub at: f64,
    /// The model's boxes crossed out, by their index in the crop's `boxes`.
    #[serde(default)]
    pub remove: Vec<usize>,
    /// The model's boxes moved or resized, by their index: [cx, cy, w, h] in crop pixels.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(as = "BTreeMap<String, aimview::typescript::CropBox>"))]
    pub edit: BTreeMap<String, [f64; 4]>,
    /// The boxes drawn ([cx, cy, w, h]) and the points tapped ([x, y]), in crop pixels.
    #[serde(default)]
    pub add: Vec<Vec<f64>>,
    /// Whether the page's suggestion was taken as offered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub suggested: Option<bool>,
    /// The scene the Crops page drew (src/shapes.rs), when it drew one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub scene: Option<Scene>,
}

/// A crop as make_page.py lists it: its id, set and file, the recording's folder and kind, why it was picked, the rule
/// that mined it, the model's boxes ([cx, cy, w, h], crop pixels) and scores, and the boxes it starts crossed out.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CropEntry {
    /// The crop's id, made from its set and file, which names its picture (crops/<id>.png) and its answer file.
    pub id: String,
    /// The set it belongs to (its tab on the page).
    pub set: String,
    /// Its file in the training data, relative to the set's source (as train/a.npz).
    pub file: String,
    /// The recording's folder it came from; empty when not known.
    #[serde(default)]
    pub folder: String,
    /// The recording's scenario kind; empty when not known.
    #[serde(default)]
    pub kind: String,
    /// Why it was picked.
    #[serde(default)]
    pub why: Vec<String>,
    /// The rule that mined it, when one did.
    #[serde(default)]
    pub rule: Option<String>,
    /// The model's boxes, [cx, cy, w, h] in crop pixels.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(as = "Vec<aimview::typescript::CropBox>"))]
    pub boxes: Vec<[f64; 4]>,
    /// The boxes' scores, 0 to 1, in the boxes' order.
    #[serde(default)]
    pub scores: Vec<f64>,
    /// The boxes it starts with crossed out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub preset: Option<CropPreset>,
}

/// The model boxes a crop starts with crossed out (a mined false box: Right agrees it is no target).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CropPreset {
    /// Their indexes in the crop's `boxes`.
    pub remove: Vec<usize>,
}

/// A set of a check folder: its tab's title, the note over a crossed-out box, whether its answers teach the page's
/// suggestions, and how many crops it has and how many are answered.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct CropSet {
    /// The set's name, as the crops name it.
    pub set: String,
    /// Its tab's title: sets.json's, else the name.
    pub title: String,
    /// The note shown over a crop with a crossed-out box.
    pub crossed_out: Option<String>,
    /// Whether its answers teach the page's suggestions (true unless sets.json says no).
    pub learn: bool,
    /// Its crops.
    pub count: usize,
    /// Its crops with an answer.
    pub answered: usize,
}

/// A check folder (the page names it by its folder's name) and its sets.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CropPage {
    /// The check folder's name.
    pub page: String,
    /// Its sets, in the order their first crops come in crops.json.
    pub sets: Vec<CropSet>,
}

/// A check folder's answers as one document, to move them between modes: browser mode exports it, the review server
/// imports it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CropAnswers {
    /// The check folder's name.
    pub page: String,
    /// The newest answer of each crop, by crop id.
    pub answers: BTreeMap<String, CropAnswer>,
}

/// A set's entry in sets.json.
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetInfo {
    /// Its tab's title (make_page.py's --title).
    title: Option<String>,
    /// The note over a crossed-out box (--crossed-out).
    crossed_out: Option<String>,
    /// False when its answers must not teach the suggestions (--no-learn).
    learn: Option<bool>,
}

/// Whether a folder name can be a check folder's: plain characters only, so it never leaves the crops folder.
fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

/// An answer file's answer: the page's own, or as the claude.ai pages' were saved ({data: answer}); None when it is
/// neither.
fn read_answer(path: &Path) -> Option<CropAnswer> {
    let value: Value = serde_json::from_slice(&crate::disk::read(path).ok()?).ok()?;
    let answer = if value.get("data").is_some_and(Value::is_object) { value["data"].clone() } else { value };
    serde_json::from_value(answer).ok()
}

/// Why an answer cannot be kept, or Ok: its marks are boxes or points of finite numbers, its scene is sound
/// (shapes::check), and it is for this crop.
fn check_answer(answer: &CropAnswer, crop: &CropEntry) -> Answer<()> {
    if answer.set != crop.set || answer.file != crop.file {
        return Err(Failure::bad(format!("the answer is for {}, not {}", answer.file, crop.file)));
    }
    let numbers = answer.edit.values().flatten().chain(answer.add.iter().flatten());
    if numbers.into_iter().any(|value| !value.is_finite()) || !answer.at.is_finite() {
        return Err(Failure::bad("the answer has a number that is not finite"));
    }
    if answer.add.iter().any(|mark| mark.len() != BOX_VALUES && mark.len() != POINT_VALUES) {
        return Err(Failure::bad("an added mark is neither a box nor a point"));
    }
    if let Some(index) = answer.remove.iter().find(|&&index| index >= crop.boxes.len()) {
        return Err(Failure::bad(format!("the crop has no box {index}")));
    }
    match &answer.scene {
        Some(scene) => shapes::check(scene).map_err(Failure::bad),
        None => Ok(()),
    }
}

impl Library {
    /// A check folder by its name, when it is one: a folder of the crops folder holding crops.json, its name plain and
    /// with the layout's start.
    fn crop_folder(&self, page: &str) -> Answer<PathBuf> {
        let folders = self.folders();
        let folder = folders.crops.join(page);
        let listed = crate::disk::is_file(folder.join("crops.json"));
        if !plain_name(page) || !page.starts_with(folders.crop_prefix) || !listed {
            return Err(Failure::missing(format!("no check folder {page}")));
        }
        Ok(folder)
    }

    /// A check folder's crops, in the order make_page.py listed them.
    fn crop_entries(&self, folder: &Path) -> Answer<Vec<CropEntry>> {
        let bytes = crate::disk::read(folder.join("crops.json")).map_err(|error| error.to_string())?;
        serde_json::from_slice(&bytes).map_err(|error| Failure::from(format!("crops.json: {error}")))
    }

    /// One crop of a check folder, by its id.
    fn crop_entry(&self, folder: &Path, id: &str) -> Answer<CropEntry> {
        let entries = self.crop_entries(folder)?;
        entries.into_iter().find(|entry| entry.id == id).ok_or_else(|| Failure::missing(format!("no crop {id}")))
    }

    /// A check folder's answers by crop id: the newest of each (by `at`) over its answers folders.
    fn folder_answers(&self, folder: &Path) -> BTreeMap<String, CropAnswer> {
        let mut answers: BTreeMap<String, CropAnswer> = BTreeMap::new();
        let dirs = crate::disk::read_dir(folder).into_iter().flatten().flatten();
        let checks = dirs.filter(|dir| dir.file_name().to_string_lossy().starts_with(ANSWERS)).map(|dir| dir.path());
        for file in checks.flat_map(|dir| crate::disk::read_dir(dir.join(CHECKS)).into_iter().flatten().flatten()) {
            let path = file.path();
            let Some(id) = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()) else { continue };
            if path.extension().is_some_and(|ext| ext == "json")
                && let Some(answer) = read_answer(&path)
                && answers.get(&id).is_none_or(|kept| kept.at < answer.at)
            {
                answers.insert(id, answer);
            }
        }
        answers
    }

    /// Every check folder with its sets (GET /api/crop_pages), by name.
    pub fn crop_pages(&self) -> Answer<Value> {
        let folders = self.folders();
        let mut names: Vec<String> = crate::disk::read_dir(&folders.crops)
            .into_iter()
            .flatten()
            .flatten()
            .map(|dir| dir.file_name().to_string_lossy().into_owned())
            .filter(|name| self.crop_folder(name).is_ok())
            .collect();
        names.sort();
        let pages: Answer<Vec<CropPage>> = names.iter().map(|name| self.crop_page(name)).collect();
        Ok(json!(pages?))
    }

    /// One check folder's sets: each set's title from sets.json, its crops and answers counted.
    fn crop_page(&self, page: &str) -> Answer<CropPage> {
        let folder = self.crop_folder(page)?;
        let entries = self.crop_entries(&folder)?;
        let answers = self.folder_answers(&folder);
        let infos: BTreeMap<String, SetInfo> = crate::library::read_json(&folder.join("sets.json")).unwrap_or_default();
        let mut sets: Vec<CropSet> = Vec::new();
        for entry in &entries {
            if !sets.iter().any(|set| set.set == entry.set) {
                let info = infos.get(&entry.set);
                sets.push(CropSet {
                    set: entry.set.clone(),
                    title: info.and_then(|info| info.title.clone()).unwrap_or_else(|| entry.set.clone()),
                    crossed_out: info.and_then(|info| info.crossed_out.clone()),
                    learn: info.and_then(|info| info.learn).unwrap_or(true),
                    count: 0,
                    answered: 0,
                });
            }
            let set = sets.iter_mut().find(|set| set.set == entry.set).expect("the set was just added");
            set.count += 1;
            set.answered += usize::from(answers.contains_key(&entry.id));
        }
        Ok(CropPage { page: page.to_string(), sets })
    }

    /// A set's crops (GET /api/crops).
    pub fn crops(&self, page: &str, set: &str) -> Answer<Value> {
        let entries = self.crop_entries(&self.crop_folder(page)?)?;
        Ok(json!(entries.into_iter().filter(|entry| entry.set == set).collect::<Vec<_>>()))
    }

    /// A set's answers by crop id (GET /api/crop_answers).
    pub fn crop_answers(&self, page: &str, set: &str) -> Answer<Value> {
        let answers = self.folder_answers(&self.crop_folder(page)?);
        Ok(json!(answers.into_iter().filter(|(_, answer)| answer.set == set).collect::<BTreeMap<_, _>>()))
    }

    /// A crop's picture, PNG (GET /api/crop_image).
    pub fn crop_image(&self, page: &str, id: &str) -> Answer<Vec<u8>> {
        let folder = self.crop_folder(page)?;
        let crop = self.crop_entry(&folder, id)?;
        crate::disk::read(folder.join("crops").join(format!("{}.png", crop.id)))
            .map_err(|error| Failure::missing(format!("the picture of {id}: {error}")))
    }

    /// Keeps a crop's answer (POST /api/crop_answer), written to a file of its own and then moved into place, so a
    /// reader never sees half of it.
    pub fn save_crop_answer(&self, page: &str, id: &str, body: &[u8]) -> Answer<Value> {
        let folder = self.crop_folder(page)?;
        let crop = self.crop_entry(&folder, id)?;
        let answer: CropAnswer =
            serde_json::from_slice(body).map_err(|error| Failure::bad(format!("the answer: {error}")))?;
        check_answer(&answer, &crop)?;
        write_answer(&folder, &crop.id, &answer)?;
        Ok(json!(answer))
    }

    /// Every answer of a check folder as one document (GET /api/crop_export).
    pub fn export_crop_answers(&self, page: &str) -> Answer<Value> {
        let answers = self.folder_answers(&self.crop_folder(page)?);
        Ok(json!(CropAnswers { page: page.to_string(), answers }))
    }

    /// A document of answers (POST /api/crop_import): each answer of a crop of the folder is kept unless the folder
    /// holds a newer one of it (by `at`); answers the counts written, kept and of crops it does not have.
    pub fn import_crop_answers(&self, page: &str, body: &[u8]) -> Answer<Value> {
        let folder = self.crop_folder(page)?;
        let document: CropAnswers =
            serde_json::from_slice(body).map_err(|error| Failure::bad(format!("the answers: {error}")))?;
        let entries = self.crop_entries(&folder)?;
        let held = self.folder_answers(&folder);
        let (mut written, mut kept, mut unknown) = (0, 0, 0);
        for (id, answer) in &document.answers {
            let Some(crop) = entries.iter().find(|entry| &entry.id == id) else {
                unknown += 1;
                continue;
            };
            if held.get(id).is_some_and(|newer| newer.at >= answer.at) {
                kept += 1;
                continue;
            }
            check_answer(answer, crop)?;
            write_answer(&folder, id, answer)?;
            written += 1;
        }
        Ok(json!({ "written": written, "kept": kept, "unknown": unknown }))
    }
}

/// A scene answer's training label: its verdict, set, file and time; per target its visible box, the box round all its
/// shapes, whether it is hidden entirely and its shapes (kinds, roles); the boxes of the targets that show, the boxes
/// of those hidden entirely (training's ignore boxes), and the pixels of every target (training's tmask) as run
/// lengths.
fn scene_label(answer: &CropAnswer, scene: &Scene) -> Value {
    let view = shapes::visible(scene, CROP_PX, CROP_PX);
    let targets: Vec<Value> = view
        .targets
        .iter()
        .map(|target| {
            let members: Vec<&shapes::Shape> =
                scene.shapes.iter().filter(|shape| target.shapes.contains(&shape.id)).collect();
            json!({ "box": target.frame, "whole": target.whole, "hidden": target.hidden, "shapes": members })
        })
        .collect();
    let boxes: Vec<[f64; 4]> = view.targets.iter().filter_map(|target| target.frame).collect();
    let ignore: Vec<[f64; 4]> = view.targets.iter().filter(|target| target.hidden).map(|target| target.whole).collect();
    json!({
        "verdict": answer.verdict, "set": answer.set, "file": answer.file, "at": answer.at, "targets": targets,
        "boxes": boxes,
        "ignore": ignore, "mask": view.mask,
    })
}

impl Library {
    /// The training labels of a check folder's scene answers (aimview-tool crop-labels): by crop id, those of the
    /// given sets (every set when none is given); answers with no scene are left to labels.py, as before.
    pub fn crop_labels(&self, page: &str, sets: &[String]) -> Answer<Value> {
        let answers = self.folder_answers(&self.crop_folder(page)?);
        let labels: BTreeMap<String, Value> = answers
            .iter()
            .filter(|(_, answer)| sets.is_empty() || sets.contains(&answer.set))
            .filter_map(|(id, answer)| Some((id.clone(), scene_label(answer, answer.scene.as_ref()?))))
            .collect();
        Ok(json!({ "page": page, "size": CROP_PX, "labels": labels }))
    }
}

/// Writes a crop's answer into the folder's answers/checks/, through a temporary file renamed into place. The answer
/// it replaces moves to answers/replaced/ first.
fn write_answer(folder: &Path, id: &str, answer: &CropAnswer) -> Answer<()> {
    let dir = folder.join(ANSWERS).join(CHECKS);
    crate::disk::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let bytes = serde_json::to_vec_pretty(answer).map_err(|error| error.to_string())?;
    let (temporary, path) = (dir.join(format!("{id}.json.part")), dir.join(format!("{id}.json")));
    crate::disk::write(&temporary, bytes).map_err(|error| error.to_string())?;
    keep_replaced(folder, id, &path)?;
    crate::disk::rename(&temporary, &path).map_err(|error| error.to_string().into())
}

/// Moves the answer at `path`, when there is one, to answers/replaced/<id>.<its time in ms>.json (a number after it
/// when that name is taken), where the page and the labels do not read it.
fn keep_replaced(folder: &Path, id: &str, path: &Path) -> Answer<()> {
    if !crate::disk::is_file(path) {
        return Ok(());
    }
    let dir = folder.join(ANSWERS).join(REPLACED);
    crate::disk::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let at = read_answer(path).map_or(0.0, |answer| answer.at.round());
    let name = |copy: usize| if copy == 0 { format!("{id}.{at}.json") } else { format!("{id}.{at}.{copy}.json") };
    let free = (0..).map(|copy| dir.join(name(copy))).find(|place| !crate::disk::exists(place));
    let place = free.ok_or_else(|| "no free name in answers/replaced".to_string())?;
    crate::disk::rename(path, place).map_err(|error| error.to_string().into())
}

/// The Crops page's routes on a check folder made for each test.
#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use serde_json::json;

    use crate::config::{Config, Layout};
    use crate::library::Library;

    /// The test check folder's name.
    const PAGE: &str = "check_bars";

    /// A library in Python's layout with one check folder: two crops of set "bars", one picture, and an answer saved by
    /// a claude.ai page ({data: answer}) in an older answers folder.
    fn library(name: &str) -> (Arc<Library>, PathBuf) {
        let dir = std::env::temp_dir().join(format!("aimview-crops-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let folder = dir.join("vod_model").join(PAGE);
        std::fs::create_dir_all(folder.join("crops")).unwrap();
        let crops = json!([
            { "id": "bars.train.a", "set": "bars", "file": "train/a.npz", "boxes": [[10, 10, 4, 4], [20, 20, 6, 6]],
              "scores": [0.9, 0.3], "preset": { "remove": [1] } },
            { "id": "bars.train.b", "set": "bars", "file": "train/b.npz" },
        ]);
        std::fs::write(folder.join("crops.json"), crops.to_string()).unwrap();
        std::fs::write(folder.join("sets.json"), json!({ "bars": { "title": "Health bars" } }).to_string()).unwrap();
        std::fs::write(folder.join("crops").join("bars.train.a.png"), b"\x89PNG").unwrap();
        let old = folder.join("answers_old").join("checks");
        std::fs::create_dir_all(&old).unwrap();
        let saved = json!({ "data": { "verdict": "right", "set": "bars", "file": "train/b.npz", "at": 5.0 } });
        std::fs::write(old.join("bars.train.b.json"), saved.to_string()).unwrap();
        std::fs::create_dir_all(dir.join("vod_model").join("data_v3")).unwrap();
        let config = Config::new(dir.clone(), Layout::Python, dir.join("models"));
        (Library::open(config).unwrap(), folder)
    }

    /// A Wrong answer to crop a, given at `at`: box 1 crossed out, a box added, and a scene of a head and a body joined
    /// into one target.
    fn answer(at: f64) -> serde_json::Value {
        json!({
            "verdict": "wrong", "set": "bars", "file": "train/a.npz", "at": at, "remove": [1], "add": [[30, 30, 5, 5]],
            "scene": { "shapes": [
                { "id": "s0", "kind": "pill", "box": [10, 10, 4, 4], "model": 0, "role": "head" },
                { "id": "s1", "kind": "box", "box": [10, 16, 6, 8], "angle": 10, "face": [2, -2], "role": "body" },
            ], "targets": [["s0", "s1"]] },
        })
    }

    /// Only check_* folders are listed, with their sets' titles and counts, crops, answers (a claude.ai page's too)
    /// and pictures; a name that climbs out or is no check folder is refused.
    #[test]
    fn the_check_folders_their_sets_and_answers_are_listed() {
        let (library, folder) = library("list");
        let pages = library.crop_pages().unwrap();
        assert_eq!(pages[0]["page"], PAGE, "check_* only: data_v3 is no check folder");
        assert_eq!(pages.as_array().unwrap().len(), 1);
        let set = &pages[0]["sets"][0];
        let counts = (set["title"].as_str(), set["count"].as_u64(), set["answered"].as_u64());
        assert_eq!(counts, (Some("Health bars"), Some(2), Some(1)));
        assert_eq!(library.crops(PAGE, "bars").unwrap().as_array().unwrap().len(), 2);
        assert_eq!(library.crop_answers(PAGE, "bars").unwrap()["bars.train.b"]["at"], 5.0, "a claude.ai page's answer");
        assert_eq!(library.crop_image(PAGE, "bars.train.a").unwrap(), b"\x89PNG");
        assert!(library.crop_image(PAGE, "bars.train.x").is_err());
        assert!(library.crops("..", "bars").is_err() && library.crops("data_v3", "bars").is_err());
        let _ = std::fs::remove_dir_all(folder.parent().unwrap().parent().unwrap());
    }

    /// An answer is written whole (no .part left), with its scene; a scene naming a missing shape, an answer for
    /// another crop, or a missing box is refused.
    #[test]
    fn an_answer_is_kept_as_its_crop_said_and_a_slip_is_refused() {
        let (library, folder) = library("save");
        let saved = library.save_crop_answer(PAGE, "bars.train.a", answer(7.0).to_string().as_bytes()).unwrap();
        assert_eq!(saved["scene"]["targets"], json!([["s0", "s1"]]));
        let file = folder.join("answers").join("checks").join("bars.train.a.json");
        let kept: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(kept["scene"]["shapes"][1]["face"], json!([2.0, -2.0]));
        assert!(!file.with_extension("json.part").exists(), "the temporary file is renamed into place");
        let mut slip = answer(8.0);
        slip["scene"]["targets"] = json!([["s0", "s9"]]);
        assert!(library.save_crop_answer(PAGE, "bars.train.a", slip.to_string().as_bytes()).is_err());
        let mut other = answer(8.0);
        other["file"] = json!("train/b.npz");
        assert!(library.save_crop_answer(PAGE, "bars.train.a", other.to_string().as_bytes()).is_err());
        let mut no_box = answer(8.0);
        no_box["remove"] = json!([2]);
        assert!(library.save_crop_answer(PAGE, "bars.train.a", no_box.to_string().as_bytes()).is_err());
        let _ = std::fs::remove_dir_all(folder.parent().unwrap().parent().unwrap());
    }

    /// Each answer given again moves the one it replaces to answers/replaced/<id>.<its time>.json, and only the newest
    /// is read.
    #[test]
    fn an_answer_given_again_keeps_the_one_it_replaces() {
        let (library, folder) = library("again");
        library.save_crop_answer(PAGE, "bars.train.a", answer(7.0).to_string().as_bytes()).unwrap();
        library.save_crop_answer(PAGE, "bars.train.a", answer(8.0).to_string().as_bytes()).unwrap();
        library.save_crop_answer(PAGE, "bars.train.a", answer(8.0).to_string().as_bytes()).unwrap();
        let replaced = folder.join("answers").join("replaced");
        let mut names: Vec<String> = std::fs::read_dir(&replaced)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["bars.train.a.7.json", "bars.train.a.8.json"]);
        assert_eq!(
            library.crop_answers(PAGE, "bars").unwrap()["bars.train.a"]["at"],
            8.0,
            "replaced ones are not read"
        );
        let _ = std::fs::remove_dir_all(folder.parent().unwrap().parent().unwrap());
    }

    /// A scene answer gives one label per target (head and body joined), with its mask; answers without a scene and
    /// sets not asked for give none.
    #[test]
    fn a_scene_answer_gives_one_label_per_target() {
        let (library, folder) = library("labels");
        library.save_crop_answer(PAGE, "bars.train.a", answer(7.0).to_string().as_bytes()).unwrap();
        let labels = library.crop_labels(PAGE, &[]).unwrap();
        assert_eq!(labels["labels"].as_object().unwrap().len(), 1, "the claude.ai answer has no scene: labels.py's");
        let label = &labels["labels"]["bars.train.a"];
        assert_eq!(label["boxes"].as_array().unwrap().len(), 1, "the head and body are one target");
        let shapes = label["targets"][0]["shapes"].as_array().unwrap();
        let roles: Vec<&str> = shapes.iter().map(|shape| shape["role"].as_str().unwrap()).collect();
        assert_eq!(roles, ["head", "body"]);
        assert!(label["mask"].as_array().unwrap().len() > 1 && label["ignore"].as_array().unwrap().is_empty());
        assert!(library.crop_labels(PAGE, &["other".into()]).unwrap()["labels"].as_object().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(folder.parent().unwrap().parent().unwrap());
    }

    /// An exported document imports with the newer answer of each crop winning and unknown crops counted; importing
    /// it again writes nothing.
    #[test]
    fn answers_move_between_modes_and_the_newer_one_wins() {
        let (library, folder) = library("move");
        library.save_crop_answer(PAGE, "bars.train.a", answer(7.0).to_string().as_bytes()).unwrap();
        let exported = library.export_crop_answers(PAGE).unwrap();
        assert_eq!(exported["answers"].as_object().unwrap().len(), 2);
        let mut document = exported.clone();
        document["answers"]["bars.train.a"]["at"] = json!(9.0);
        document["answers"]["bars.train.a"]["verdict"] = json!("unsure");
        document["answers"]["bars.train.b"]["at"] = json!(1.0);
        document["answers"]["bars.train.z"] = document["answers"]["bars.train.b"].clone();
        let counts = library.import_crop_answers(PAGE, document.to_string().as_bytes()).unwrap();
        assert_eq!(counts, json!({ "written": 1, "kept": 1, "unknown": 1 }));
        assert_eq!(library.crop_answers(PAGE, "bars").unwrap()["bars.train.a"]["verdict"], "unsure");
        let again = library.import_crop_answers(PAGE, document.to_string().as_bytes()).unwrap();
        assert_eq!(again["written"], 0, "importing again writes nothing");
        let _ = std::fs::remove_dir_all(folder.parent().unwrap().parent().unwrap());
    }
}
