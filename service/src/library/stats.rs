//! KovaaK's stats files and each recording's pairing with one: the user's choice (stats.json in the recording's
//! folder), else one uploaded beside it, else the stats file of the same scenario whose time is nearest the
//! recording's (python/server.py: stats_index, stats_for, stats_of, stats_info, set_stats).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::names::{local_stamp, parse_stats_name, parse_video, stamp_seconds};
use super::reviews::Job;
use super::{Answer, Failure, Library, modified, read_json, write_json};

/// Stats files offered to pair with a recording.
const CANDIDATES: usize = 40;
/// Seconds before the stats folder is listed again (runs played while the library is open are found).
const INDEX_AGE: u64 = 60;
/// The most a stats file's time can be from the recording's, in seconds, to pair them by time.
const NEAR: f64 = 5.0;

/// A stats file in KovaaK's folder: when its run ended (seconds, see `stamp_seconds`), its name and its time stamp.
pub(super) struct StatsEntry {
    t: f64,
    name: String,
    stamp: String,
}

/// KovaaK's stats files by scenario name, and when the folder was listed.
#[derive(Default)]
pub(super) struct StatsIndex {
    by_scenario: HashMap<String, Vec<StatsEntry>>,
    listed: Option<Instant>,
}

/// The user's choice of stats file for a recording (stats.json): a file and where it is, or no file.
#[derive(Serialize, Deserialize)]
struct Pick {
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
            for e in std::fs::read_dir(self.stats_folder()).into_iter().flatten().flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if let Some((scenario, stamp)) = parse_stats_name(&name) {
                    let t = stamp_seconds(&stamp).unwrap_or(0.0);
                    by_scenario.entry(scenario).or_default().push(StatsEntry { t, name, stamp });
                }
            }
            *index = StatsIndex { by_scenario, listed: Some(Instant::now()) };
        }
        use_index(&index.by_scenario)
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

    fn pairing(&self, id: &str) -> Option<Pick> {
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
        if let Some(pick) = self.pairing(id) {
            return pick.file.and_then(|f| self.stats_file(&f, &pick.source).ok()).filter(|p| p.is_file());
        }
        let beside = video.with_extension("csv");
        if id.starts_with("uploads/") && beside.is_file() {
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
            let _ = std::fs::remove_file(&path);
        } else {
            let file = body["file"].as_str().map(str::to_string);
            let source = body["source"].as_str().unwrap_or("kovaak").to_string();
            if let Some(f) = &file
                && !self.stats_file(f, &source)?.is_file()
            {
                return Err(Failure::missing(f.clone()));
            }
            write_json(&path, &Pick { file, source })?;
        }
        let job = if self.reviewed(id) { Job::new("done", &self.shown(id).0.unwrap_or_default()) } else { Job::new("none", "") };
        Ok(json!({ "job": job, "stats": self.stats_of(id, &video).is_some() }))
    }
}
