//! KovaaK's stats files and each recording's pairing with one: the user's choice (stats.json in the recording's
//! folder), else one uploaded beside it, else the stats file of the same scenario whose time is nearest the
//! recording's (python/server.py: stats_index, stats_for, stats_of, stats_info, set_stats).

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use aimview::stats_file::StatsFile;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::names::{local_stamp, parse_stats_name, parse_video, stamp_seconds};
use super::reviews::Job;
use super::{Answer, Failure, Library, modified, read_json, write_json};
use crate::disk::Instant;

/// Stats files offered to pair with a recording.
const CANDIDATES: usize = 40;
/// Seconds before the stats folder is listed again (runs played while the library is open are found).
const INDEX_AGE: u64 = 60;
/// The most a stats file's time can be from the recording's, in seconds, to pair them by time.
const NEAR: f64 = 5.0;
/// The end of a stats file read for its "Key:,value" lines, in bytes (they take about 1.5 kB).
const FOOTER: u64 = 4096;

/// A stats file in KovaaK's folder: when its run ended (seconds, see `stamp_seconds`), its name, its time stamp and
/// when it was last changed (seconds since 1970).
pub(super) struct StatsEntry {
    t: f64,
    name: String,
    stamp: String,
    modified: f64,
}

/// A past run of a scenario, from its stats file: when it ended (the file name's time stamp), its score, its kills,
/// and its accuracy (hits over shots).
#[derive(Clone, Serialize)]
struct PastRun {
    stamp: String,
    score: f64,
    kills: Option<f64>,
    accuracy: Option<f64>,
}

/// KovaaK's stats files by scenario name, and when the folder was listed; and the runs read from them, by file name
/// with the file's time of change (None: the file has no score).
#[derive(Default)]
pub(super) struct StatsIndex {
    by_scenario: HashMap<String, Vec<StatsEntry>>,
    listed: Option<Instant>,
    runs: HashMap<String, (f64, Option<PastRun>)>,
}

/// A number from a stats file's "Key:,value" line.
fn number(meta: &HashMap<String, String>, key: &str) -> Option<f64> {
    meta.get(key)?.trim().parse().ok().filter(|v: &f64| v.is_finite())
}

/// A stats file's run, read from its last few kB (the whole file when they hold no score). None when it has no score.
fn past_run(path: &Path, stamp: &str) -> Option<PastRun> {
    let mut f = crate::disk::File::open(path).ok()?;
    let from = f.metadata().ok()?.len().saturating_sub(FOOTER);
    let mut bytes = Vec::new();
    f.seek(SeekFrom::Start(from)).ok()?;
    f.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    // the first line is cut unless the file starts there
    let tail = if from > 0 { text.split_once('\n').map_or("", |(_, rest)| rest) } else { &text };
    let mut meta = StatsFile::parse(tail).meta;
    if from > 0 && !meta.contains_key("Score") {
        meta = StatsFile::parse(&String::from_utf8_lossy(&crate::disk::read(path).ok()?)).meta;
    }
    let (hits, misses) = (number(&meta, "Hit Count"), number(&meta, "Miss Count"));
    let accuracy = match (hits, misses) {
        (Some(h), Some(m)) if h + m > 0.0 => Some(h / (h + m)),
        _ => None,
    };
    Some(PastRun { stamp: stamp.to_string(), score: number(&meta, "Score")?, kills: number(&meta, "Kills"), accuracy })
}

/// The user's choice of stats file for a recording (stats.json): a file and where it is, or no file.
#[derive(Serialize, Deserialize)]
pub(super) struct Pick {
    file: Option<String>,
    source: String,
}

impl Library {
    /// KovaaK's stats folder.
    pub fn stats_folder(&self) -> PathBuf {
        self.config.stats.clone()
    }

    /// KovaaK's stats files, listed again once the listing is a minute old.
    fn with_stats<T>(&self, use_index: impl FnOnce(&HashMap<String, Vec<StatsEntry>>) -> T) -> T {
        let mut index = self.stats.lock().unwrap_or_else(|e| e.into_inner());
        if index.listed.is_none_or(|t| t.elapsed().as_secs() > INDEX_AGE) {
            let mut by_scenario: HashMap<String, Vec<StatsEntry>> = HashMap::new();
            for e in crate::disk::read_dir(self.stats_folder()).into_iter().flatten().flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if let Some((scenario, stamp)) = parse_stats_name(&name) {
                    let t = stamp_seconds(&stamp).unwrap_or(0.0);
                    // cheap on Windows: the listing gives it. In the browser each file's would be a call to the
                    // page (KovaaK's folder holds tens of thousands), so `history` reads it for its own files only.
                    let modified = if cfg!(feature = "native") {
                        e.metadata().ok().and_then(|m| m.modified()).unwrap_or(0.0)
                    } else {
                        0.0
                    };
                    by_scenario.entry(scenario).or_default().push(StatsEntry { t, name, stamp, modified });
                }
            }
            index.by_scenario = by_scenario;
            index.listed = Some(Instant::now());
        }
        use_index(&index.by_scenario)
    }

    /// Forgets the stats files listed: they are listed again when next needed (the runs read from them are kept by
    /// their time of change).
    #[cfg(not(feature = "native"))]
    pub(super) fn forget_stats(&self) {
        let mut index = self.stats.lock().unwrap_or_else(|e| e.into_inner());
        (index.by_scenario, index.listed) = (HashMap::new(), None);
    }

    /// The stats file of a scenario's run that ended at `stamp` (a file-name time stamp): the one of that scenario
    /// nearest it, within 5 s (python/server.py: stats_for).
    pub fn stats_for(&self, scenario: &str, stamp: &str) -> Option<PathBuf> {
        let t = stamp_seconds(stamp)?;
        let name = self.with_stats(|index| {
            index
                .get(scenario)?
                .iter()
                .filter(|e| (e.t - t).abs() <= NEAR)
                .min_by(|a, b| (a.t - t).abs().total_cmp(&(b.t - t).abs()).then(a.name.cmp(&b.name)))
                .map(|e| e.name.clone())
        })?;
        Some(self.stats_folder().join(name))
    }

    /// Every run of a scenario in KovaaK's stats folder, oldest first: [{stamp, score, kills, accuracy}]. Each file is
    /// read once (its last few kB), then kept by name and time of change; files are read on several threads.
    pub fn history(&self, scenario: &str) -> Answer<Value> {
        let mut files: Vec<(f64, String, String, f64)> = self.with_stats(|index| {
            index.get(scenario).map_or_else(Vec::new, |list| {
                list.iter().map(|e| (e.t, e.name.clone(), e.stamp.clone(), e.modified)).collect()
            })
        });
        files.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        if !cfg!(feature = "native") {
            let folder = self.stats_folder();
            for f in &mut files {
                f.3 = modified(&folder.join(&f.1));
            }
        }
        let mut runs: Vec<Option<Option<PastRun>>> = {
            let index = self.stats.lock().unwrap_or_else(|e| e.into_inner());
            files
                .iter()
                .map(|(_, name, _, modified)| index.runs.get(name).filter(|r| r.0 == *modified).map(|r| r.1.clone()))
                .collect()
        };
        let unread: Vec<usize> = (0..files.len()).filter(|&i| runs[i].is_none()).collect();
        if !unread.is_empty() {
            let folder = self.stats_folder();
            #[cfg(feature = "native")]
            let read: Vec<(usize, Option<PastRun>)> = {
                let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(16);
                std::thread::scope(|s| {
                    let jobs: Vec<_> = unread
                        .chunks(unread.len().div_ceil(threads))
                        .map(|part| {
                            let (files, folder) = (&files, &folder);
                            s.spawn(move || {
                                part.iter().map(|&i| (i, past_run(&folder.join(&files[i].1), &files[i].2))).collect::<Vec<_>>()
                            })
                        })
                        .collect();
                    jobs.into_iter().flat_map(|j| j.join().unwrap_or_default()).collect()
                })
            };
            // the browser build has one thread
            #[cfg(not(feature = "native"))]
            let read: Vec<(usize, Option<PastRun>)> = unread.iter().map(|&i| (i, past_run(&folder.join(&files[i].1), &files[i].2))).collect();
            let mut index = self.stats.lock().unwrap_or_else(|e| e.into_inner());
            for (i, run) in read {
                index.runs.insert(files[i].1.clone(), (files[i].3, run.clone()));
                runs[i] = Some(run);
            }
        }
        Ok(serde_json::to_value(runs.into_iter().flatten().flatten().collect::<Vec<_>>()).map_err(|e| e.to_string())?)
    }

    pub(super) fn pairing(&self, id: &str) -> Option<Pick> {
        read_json(&self.review_dir(id).join("stats.json"))
    }

    fn stats_file(&self, name: &str, source: &str) -> Answer<PathBuf> {
        let plain = Path::new(name).file_name().is_some_and(|f| f == name) && name.to_lowercase().ends_with(".csv");
        if !plain {
            return Err(Failure::bad(format!("not a stats file: {name}")));
        }
        match source {
            "kovaak" => Ok(self.stats_folder().join(name)),
            "upload" => Ok(self.uploads().join("stats").join(name)),
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
            return pick.file.and_then(|f| self.stats_file(&f, &pick.source).ok()).filter(|p| crate::disk::is_file(p));
        }
        let beside = video.with_extension("csv");
        if id.starts_with("uploads/") && crate::disk::is_file(&beside) {
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
    /// with q those of every scenario whose name holds q, nearest the recording's time first.
    pub fn stats_info(&self, id: &str, q: Option<&str>) -> Answer<Value> {
        let video = self.resolve(id)?;
        let (pick, file) = (self.pairing(id), self.stats_of(id, &video));
        let how = match &pick {
            Some(p) if p.file.is_none() => "none",
            Some(_) if file.is_none() => "gone",
            Some(p) if p.source == "upload" => "upload",
            Some(_) => "picked",
            None if file.is_none() => "missing",
            None if id.starts_with("uploads/") && file.as_ref().is_some_and(|f| f.parent() == video.parent()) => "beside",
            None => "found",
        };
        let parsed = parse_video(&video);
        let scenario = parsed.as_ref().map_or_else(
            || video.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            |(s, _, _)| s.clone(),
        );
        let t = parsed.and_then(|(_, _, stamp)| stamp_seconds(&stamp)).unwrap_or_else(|| {
            stamp_seconds(&local_stamp(modified(&video))).unwrap_or(0.0)
        });
        let text = q.unwrap_or(&scenario).trim().to_lowercase();
        let candidates = self.with_stats(|index| {
            let mut near: Vec<(f64, &str, &StatsEntry)> = index
                .iter()
                .filter(|(s, _)| if q.is_none() { s.to_lowercase() == text } else { s.to_lowercase().contains(&text) })
                .flat_map(|(s, entries)| entries.iter().map(move |e| ((e.t - t).abs(), s.as_str(), e)))
                .collect();
            near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.2.t.total_cmp(&b.2.t)).then(a.1.cmp(b.1)));
            near.into_iter()
                .take(CANDIDATES)
                .map(|(_, s, e)| json!({ "name": e.name, "scenario": s, "stamp": e.stamp, "off": ((e.t - t) * 10.0).round() / 10.0 }))
                .collect::<Vec<_>>()
        });
        let file = file
            .and_then(|f| f.file_name().map(|n| n.to_string_lossy().into_owned()))
            .or_else(|| pick.and_then(|p| p.file));
        Ok(json!({ "file": file, "how": how, "scenario": scenario, "candidates": candidates }))
    }

    /// The user's choice of stats file ({file, source}, file null for none) or {auto: true} (by name and time again).
    /// The report is worked out when it is shown, so nothing is measured again here.
    pub fn set_stats(&self, id: &str, body: &Value) -> Answer<Value> {
        let video = self.resolve(id)?;
        let path = self.review_dir(id).join("stats.json");
        if body["auto"].as_bool() == Some(true) {
            let _ = crate::disk::remove_file(&path);
        } else {
            let file = body["file"].as_str().map(str::to_string);
            let source = body["source"].as_str().unwrap_or("kovaak").to_string();
            if let Some(f) = &file
                && !crate::disk::is_file(self.stats_file(f, &source)?)
            {
                return Err(Failure::missing(f.clone()));
            }
            write_json(&path, &Pick { file, source })?;
        }
        let job = if self.reviewed(id) { Job::new("done", &self.shown(id).0.unwrap_or_default()) } else { Job::new("none", "") };
        Ok(json!({ "job": job, "stats": self.stats_of(id, &video).is_some() }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
