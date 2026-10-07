//! KovaaK's stats files and each recording's pairing with one (python/retired/server.py: stats_index, stats_for,
//! stats_of, stats_info, set_stats): the user's choice (stats.json in the recording's folder), else one uploaded beside
//! it, else the stats file of the same scenario whose time is nearest the recording's.
//!
//! In: KovaaK's stats folder (listed again once a minute; a file's run read from its last few kB), the stats files
//! uploaded for a recording, and the page's choice. Out: each recording's stats file for its report and the list, the
//! stats files to pair a recording with (/api/stats), and a scenario's past runs (/api/history).

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::PoisonError;

use aimview::stats_file::StatsFile;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::names::{local_stamp, parse_stats_name, parse_video, stamp_seconds};
use super::recordings::{STATS_UPLOADS, is_upload};
use super::reviews::Job;
use super::{Answer, Failure, Library, keep_json, modified, read_kept};
use crate::disk::Instant;
use crate::store::{Item, Mark};

/// Stats files offered to pair with a recording.
const CANDIDATES: usize = 40;
/// Seconds before the stats folder is listed again (runs played while the library is open are found).
const INDEX_AGE_S: u64 = 60;
/// The most a stats file's time can be from the recording's, in seconds, to pair them by time.
const NEAR_S: f64 = 5.0;
/// The end of a stats file read for its "Key:,value" lines, in bytes (they take about 1.5 kB).
const FOOTER_BYTES: u64 = 4096;
/// Where a picked stats file is kept (`Pick`'s source): KovaaK's stats folder.
const KOVAAK_SOURCE: &str = "kovaak";
/// Where a picked stats file is kept: the uploads' stats folder.
pub(super) const UPLOAD_SOURCE: &str = "upload";
/// The most threads that read stats files at once natively.
#[cfg(feature = "native")]
const MAX_READ_THREADS: usize = 16;
/// The threads that read stats files at once when the computer's count is not known.
#[cfg(feature = "native")]
const READ_THREADS_UNKNOWN: usize = 4;

/// A stats file in KovaaK's folder: when its run ended (seconds, see `stamp_seconds`), its name, its time stamp and
/// when it was last changed (seconds since 1970).
#[derive(Clone)]
pub(super) struct StatsEntry {
    /// When its run ended: its name's stamp in seconds from 2000-01-01.
    end_s: f64,
    /// Its file name in the stats folder.
    name: String,
    /// Its name's time stamp (yyyy.mm.dd-hh.mm.ss).
    stamp: String,
    /// Its time of change in seconds since 1970 (0 in the browser until `history` reads it).
    modified: f64,
}

/// A stats file's run as read, kept with the file's time of change then (None: the file has no score).
struct ReadRun {
    /// The file's time of change when it was read: a newer one reads it again.
    modified: f64,
    /// The run; None when the file has no score.
    run: Option<PastRun>,
}

/// A past run of a scenario, from its stats file: when it ended (the file name's time stamp), its score, its kills,
/// and its accuracy (hits over shots).
#[derive(Clone, Serialize)]
struct PastRun {
    /// The file name's time stamp.
    stamp: String,
    /// KovaaK's score.
    score: f64,
    /// The kills, when the file gives them.
    kills: Option<f64>,
    /// Hits over shots, 0 to 1; None without hits and misses.
    accuracy: Option<f64>,
}

/// KovaaK's stats files by scenario name, and when the folder was listed; and the runs read from them, by file name.
#[derive(Default)]
pub(super) struct StatsIndex {
    /// The stats files by scenario name.
    by_scenario: HashMap<String, Vec<StatsEntry>>,
    /// When the folder was last listed; None: not yet, or forgotten.
    listed: Option<Instant>,
    /// The runs read for `history`, by file name.
    runs: HashMap<String, ReadRun>,
}

/// A number from a stats file's "Key:,value" line.
fn number(meta: &HashMap<String, String>, key: &str) -> Option<f64> {
    meta.get(key)?.trim().parse().ok().filter(|value: &f64| value.is_finite())
}

/// A stats file's run, read from its last few kB (the whole file when they hold no score). None when it has no score.
fn past_run(path: &Path, stamp: &str) -> Option<PastRun> {
    let mut file = crate::disk::File::open(path).ok()?;
    let from = file.metadata().ok()?.len().saturating_sub(FOOTER_BYTES);
    let mut bytes = Vec::new();
    file.seek(SeekFrom::Start(from)).ok()?;
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    // the first line is cut unless the file starts there
    let tail = if from > 0 { text.split_once('\n').map_or("", |(_, rest)| rest) } else { &text };
    let mut meta = StatsFile::parse(tail).meta;
    if from > 0 && !meta.contains_key("Score") {
        meta = StatsFile::parse(&String::from_utf8_lossy(&crate::disk::read(path).ok()?)).meta;
    }
    let (hits, misses) = (number(&meta, "Hit Count"), number(&meta, "Miss Count"));
    let accuracy = match (hits, misses) {
        (Some(hits), Some(misses)) if hits + misses > 0.0 => Some(hits / (hits + misses)),
        _ => None,
    };
    Some(PastRun { stamp: stamp.to_string(), score: number(&meta, "Score")?, kills: number(&meta, "Kills"), accuracy })
}

/// The run of a stats file in `folder`.
fn read_past_run(folder: &Path, file: &StatsEntry) -> Option<PastRun> {
    past_run(&folder.join(&file.name), &file.stamp)
}

/// The runs of the stats files `unread` (indexes into `files`), each with its index, read on several threads.
#[cfg(feature = "native")]
fn read_past_runs(folder: &Path, files: &[StatsEntry], unread: &[usize]) -> Vec<(usize, Option<PastRun>)> {
    let threads = std::thread::available_parallelism()
        .map_or(READ_THREADS_UNKNOWN, std::num::NonZeroUsize::get)
        .min(MAX_READ_THREADS);
    std::thread::scope(|scope| {
        let jobs: Vec<_> = unread
            .chunks(unread.len().div_ceil(threads))
            .map(|part| {
                scope.spawn(move || part.iter().map(|&i| (i, read_past_run(folder, &files[i]))).collect::<Vec<_>>())
            })
            .collect();
        jobs.into_iter().flat_map(|job| job.join().unwrap_or_default()).collect()
    })
}

/// The same in the browser build, which has one thread.
#[cfg(not(feature = "native"))]
fn read_past_runs(folder: &Path, files: &[StatsEntry], unread: &[usize]) -> Vec<(usize, Option<PastRun>)> {
    unread.iter().map(|&i| (i, read_past_run(folder, &files[i]))).collect()
}

/// A time in seconds rounded to tenths, as the page shows a candidate's distance from the recording.
fn tenths(seconds: f64) -> f64 {
    (seconds * 10.0).round() / 10.0
}

/// A recording's scenario and when its run ended (seconds, see `stamp_seconds`): from its name (KovOBS's), else its
/// file name and its time of change.
fn scenario_and_end(video: &Path) -> (String, f64) {
    let parsed = parse_video(video);
    let scenario = parsed.as_ref().map_or_else(
        || video.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default(),
        |(scenario, _, _)| scenario.clone(),
    );
    let end_s = parsed
        .and_then(|(_, _, stamp)| stamp_seconds(&stamp))
        .unwrap_or_else(|| stamp_seconds(&local_stamp(modified(video))).unwrap_or(0.0));
    (scenario, end_s)
}

/// How a recording came to its stats file `file` (stats_info's `how`): the user's pick (none, gone, upload or
/// picked), else missing, beside (an upload's, by its name) or found (by name and time).
fn pairing_how(pick: Option<&Pick>, file: Option<&Path>, id: &str, video: &Path) -> &'static str {
    match pick {
        Some(pick) if pick.file.is_none() => "none",
        Some(_) if file.is_none() => "gone",
        Some(pick) if pick.source == UPLOAD_SOURCE => "upload",
        Some(_) => "picked",
        None if file.is_none() => "missing",
        None if is_upload(id) && file.is_some_and(|file| file.parent() == video.parent()) => "beside",
        None => "found",
    }
}

/// The user's choice of stats file for a recording (stats.json): a file and where it is, or no file.
#[derive(Serialize, Deserialize)]
pub(super) struct Pick {
    /// The stats file's name; None: the user chose no stats file.
    file: Option<String>,
    /// Where it is: `KOVAAK_SOURCE` or `UPLOAD_SOURCE`.
    source: String,
}

impl Library {
    /// KovaaK's stats folder.
    pub fn stats_folder(&self) -> PathBuf {
        self.config.stats.clone()
    }

    /// KovaaK's stats files, listed again once the listing is a minute old.
    fn with_stats<T>(&self, use_index: impl FnOnce(&HashMap<String, Vec<StatsEntry>>) -> T) -> T {
        let mut index = self.stats.lock().unwrap_or_else(PoisonError::into_inner);
        if index.listed.is_none_or(|listed| listed.elapsed().as_secs() > INDEX_AGE_S) {
            index.by_scenario = self.list_stats();
            index.listed = Some(Instant::now());
        }
        use_index(&index.by_scenario)
    }

    /// KovaaK's stats folder, listed: its stats files by scenario name.
    fn list_stats(&self) -> HashMap<String, Vec<StatsEntry>> {
        let mut by_scenario: HashMap<String, Vec<StatsEntry>> = HashMap::new();
        for entry in crate::disk::read_dir(self.stats_folder()).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some((scenario, stamp)) = parse_stats_name(&name) else { continue };
            let end_s = stamp_seconds(&stamp).unwrap_or(0.0);
            // cheap on Windows: the listing gives it. In the browser each file's would be a call to the page
            // (KovaaK's folder holds tens of thousands), so `history` reads it for its own files only.
            let modified = if cfg!(feature = "native") {
                entry.metadata().ok().and_then(|metadata| metadata.modified()).unwrap_or(0.0)
            } else {
                0.0
            };
            by_scenario.entry(scenario).or_default().push(StatsEntry { end_s, name, stamp, modified });
        }
        by_scenario
    }

    /// Forgets the stats files listed: they are listed again when next needed (the runs read from them are kept by
    /// their time of change).
    #[cfg(not(feature = "native"))]
    pub(super) fn forget_stats(&self) {
        let mut index = self.stats.lock().unwrap_or_else(PoisonError::into_inner);
        (index.by_scenario, index.listed) = (HashMap::new(), None);
    }

    /// The stats file of a scenario's run that ended at `stamp` (a file-name time stamp): the one of that scenario
    /// nearest it, within 5 s (python/retired/server.py: stats_for).
    pub fn stats_for(&self, scenario: &str, stamp: &str) -> Option<PathBuf> {
        let end_s = stamp_seconds(stamp)?;
        let distance_s = |entry: &StatsEntry| (entry.end_s - end_s).abs();
        let name = self.with_stats(|index| {
            index
                .get(scenario)?
                .iter()
                .filter(|entry| distance_s(entry) <= NEAR_S)
                .min_by(|a, b| distance_s(a).total_cmp(&distance_s(b)).then(a.name.cmp(&b.name)))
                .map(|entry| entry.name.clone())
        })?;
        Some(self.stats_folder().join(name))
    }

    /// Every run of a scenario in KovaaK's stats folder, oldest first: [{stamp, score, kills, accuracy}]. Each file is
    /// read once (its last few kB), then kept by name and time of change; files are read on several threads.
    pub fn history(&self, scenario: &str) -> Answer<Value> {
        let mut files: Vec<StatsEntry> = self.with_stats(|index| index.get(scenario).cloned().unwrap_or_default());
        files.sort_by(|a, b| a.end_s.total_cmp(&b.end_s).then(a.name.cmp(&b.name)));
        if !cfg!(feature = "native") {
            let folder = self.stats_folder();
            for file in &mut files {
                file.modified = modified(&folder.join(&file.name));
            }
        }
        let mut runs: Vec<Option<Option<PastRun>>> = {
            let index = self.stats.lock().unwrap_or_else(PoisonError::into_inner);
            let kept = |file: &StatsEntry| index.runs.get(&file.name).filter(|read| read.modified == file.modified);
            files.iter().map(|file| kept(file).map(|read| read.run.clone())).collect()
        };
        let unread: Vec<usize> = (0..files.len()).filter(|&i| runs[i].is_none()).collect();
        if !unread.is_empty() {
            let read = read_past_runs(&self.stats_folder(), &files, &unread);
            let mut index = self.stats.lock().unwrap_or_else(PoisonError::into_inner);
            for (i, run) in read {
                index.runs.insert(files[i].name.clone(), ReadRun { modified: files[i].modified, run: run.clone() });
                runs[i] = Some(run);
            }
        }
        let past_runs: Vec<PastRun> = runs.into_iter().flatten().flatten().collect();
        Ok(serde_json::to_value(past_runs).map_err(|error| error.to_string())?)
    }

    /// The user's choice of stats file for the recording (stats.json); None when none is kept.
    pub(super) fn pairing(&self, id: &str) -> Option<Pick> {
        read_kept(self.store(), Item::Mark(id, Mark::StatsPick))
    }

    /// The path of a stats file by its name and source; an error for a name that is not a plain .csv file name, or an
    /// unknown source.
    fn stats_file(&self, name: &str, source: &str) -> Answer<PathBuf> {
        let plain = Path::new(name).file_name().is_some_and(|file_name| file_name == name)
            && name.to_lowercase().ends_with(".csv");
        if !plain {
            return Err(Failure::bad(format!("not a stats file: {name}")));
        }
        match source {
            KOVAAK_SOURCE => Ok(self.stats_folder().join(name)),
            UPLOAD_SOURCE => Ok(self.uploads().join(STATS_UPLOADS).join(name)),
            _ => Err(Failure::bad(format!("no stats files kept in {source}"))),
        }
    }

    /// The recording's stats file: the user's choice (None when it is gone), else one uploaded beside it (same name,
    /// .csv), else by name and time.
    pub(crate) fn stats_of(&self, id: &str, video: &Path) -> Option<PathBuf> {
        self.stats_with(self.pairing(id), id, video)
    }

    /// `stats_of` with the recording's pairing already read (None: no choice made). The recordings list reads it only
    /// for a recording that has a folder in the data folder, where a choice is kept.
    pub(super) fn stats_with(&self, pick: Option<Pick>, id: &str, video: &Path) -> Option<PathBuf> {
        if let Some(pick) = pick {
            let picked = pick.file.and_then(|file| self.stats_file(&file, &pick.source).ok());
            return picked.filter(|path| crate::disk::is_file(path));
        }
        let beside = video.with_extension("csv");
        if is_upload(id) && crate::disk::is_file(&beside) {
            return Some(beside);
        }
        let (scenario, _, stamp) = parse_video(video)?;
        self.stats_for(&scenario, &stamp)
    }

    /// The recording's stats file (see `stats_of`); None when it has none or the recording is not there.
    pub fn stats_path(&self, id: &str) -> Option<PathBuf> {
        let video = self.resolve(id).ok()?;
        self.stats_of(id, &video)
    }

    /// The stats file a recording uses and how it came to it, and the stats files to pair it with: its scenario's, or
    /// with `query` those of every scenario whose name holds it, nearest the recording's time first.
    pub fn stats_info(&self, id: &str, query: Option<&str>) -> Answer<Value> {
        let video = self.resolve(id)?;
        let (pick, file) = (self.pairing(id), self.stats_of(id, &video));
        let how = pairing_how(pick.as_ref(), file.as_deref(), id, &video);
        let (scenario, end_s) = scenario_and_end(&video);
        let text = query.unwrap_or(&scenario).trim().to_lowercase();
        let candidates = self.candidates(&text, query.is_some(), end_s);
        let file = file
            .and_then(|file| file.file_name().map(|name| name.to_string_lossy().into_owned()))
            .or_else(|| pick.and_then(|pick| pick.file));
        Ok(json!({ "file": file, "how": how, "scenario": scenario, "candidates": candidates }))
    }

    /// The stats files to pair a recording whose run ended at `end_s` with, nearest it first (then the earlier one,
    /// then by scenario): those of the scenario named `text` (lower case), or with `containing` those of every scenario
    /// whose name holds it.
    fn candidates(&self, text: &str, containing: bool, end_s: f64) -> Vec<Value> {
        let named = |scenario: &str| {
            if containing { scenario.to_lowercase().contains(text) } else { scenario.to_lowercase() == text }
        };
        self.with_stats(|index| {
            let mut near: Vec<(f64, &str, &StatsEntry)> = index
                .iter()
                .filter(|(scenario, _)| named(scenario))
                .flat_map(|(scenario, entries)| {
                    entries.iter().map(move |entry| ((entry.end_s - end_s).abs(), scenario.as_str(), entry))
                })
                .collect();
            near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.2.end_s.total_cmp(&b.2.end_s)).then(a.1.cmp(b.1)));
            near.into_iter()
                .take(CANDIDATES)
                .map(|(_, scenario, entry)| {
                    let off_s = tenths(entry.end_s - end_s);
                    json!({ "name": entry.name, "scenario": scenario, "stamp": entry.stamp, "off": off_s })
                })
                .collect()
        })
    }

    /// The user's choice of stats file ({file, source}, file null for none) or {auto: true} (by name and time again).
    /// The report is worked out when it is shown, so nothing is measured again here.
    pub fn set_stats(&self, id: &str, body: &Value) -> Answer<Value> {
        let video = self.resolve(id)?;
        let pick = Item::Mark(id, Mark::StatsPick);
        if body["auto"].as_bool() == Some(true) {
            let _ = self.store().remove(pick);
        } else {
            let file = body["file"].as_str().map(str::to_string);
            let source = body["source"].as_str().unwrap_or(KOVAAK_SOURCE).to_string();
            if let Some(name) = &file
                && !crate::disk::is_file(self.stats_file(name, &source)?)
            {
                return Err(Failure::missing(name.clone()));
            }
            keep_json(self.store(), pick, &Pick { file, source })?;
        }
        let job = if self.reviewed(id) {
            Job::new("done", &self.shown(id).0.unwrap_or_default())
        } else {
            Job::new("none", "")
        };
        Ok(json!({ "job": job, "stats": self.stats_of(id, &video).is_some() }))
    }
}

/// Reading past runs.
#[cfg(test)]
mod tests {
    use super::*;

    /// A run's score, kills and accuracy come from the file's end, or the whole file when the end has no score; a file
    /// with no score gives no run.
    #[test]
    fn a_run_is_read_from_the_footer_or_the_whole_file() {
        let dir = std::env::temp_dir().join(format!("aimview-history-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let footer = "Kills:,129\nHit Count:,129\nMiss Count:,13\nScore:,1171.9\nScenario:,a\n";
        // a long kill table first, so the footer is all the end holds
        let long = format!("Kill #,Timestamp\n{}\n{footer}", "1,17:09:19.328,target\n".repeat(400));
        std::fs::write(dir.join("long.csv"), &long).unwrap();
        let run = past_run(&dir.join("long.csv"), "s").unwrap();
        assert_eq!((run.score, run.kills), (1171.9, Some(129.0)));
        assert!((run.accuracy.unwrap() - 129.0 / 142.0).abs() < 1e-12);
        // the score near the start, a long table after it: the whole file is read
        std::fs::write(dir.join("early.csv"), format!("Score:,5\n{}", "x\n".repeat(3000))).unwrap();
        let early = past_run(&dir.join("early.csv"), "s").unwrap();
        assert_eq!((early.score, early.kills, early.accuracy), (5.0, None, None));
        std::fs::write(dir.join("none.csv"), "Kills:,1\n").unwrap();
        assert!(past_run(&dir.join("none.csv"), "s").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
