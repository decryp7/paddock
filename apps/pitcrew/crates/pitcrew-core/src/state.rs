//! The service's own state file (`state.json`): what it remembers about each runner across restarts — whether the
//! user stopped or paused it, its history, the last update, the latest version found, the last cleanup.
//!
//! Like the configuration it is versioned, written atomically with a backup by the service, and loading never
//! fails: a damaged file falls back to the backup, then to empty state; one damaged runner entry is dropped while the
//! others load.

use crate::history::{History, HistoryEvent};
use crate::housekeeping::CleanupReport;
use crate::metrics::{MetricsStore, SizeSeries, UsageStore};
use crate::process_track::KeepRecord;
use crate::protocol::UpdateRecord;
use crate::time::Timestamp;
use crate::version::RunnerVersion;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The state format this Pitcrew writes.
pub const STATE_VERSION: u32 = 1;
/// Largest state file Pitcrew reads.
pub const MAX_STATE_BYTES: usize = 16 * 1024 * 1024;

/// What the service remembers about one runner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RunnerState {
    /// The user wants it running (false after Stop).
    pub wanted: bool,
    /// Paused for maintenance.
    pub paused: bool,
    /// Its history.
    pub history: History,
    /// The last update.
    pub last_update: Option<UpdateRecord>,
    /// The latest version found by the last check.
    pub latest: Option<RunnerVersion>,
    /// When the last check ran.
    pub checked_at: Option<Timestamp>,
    /// The last version a notification announced (announced once).
    pub announced: Option<RunnerVersion>,
    /// The last cleanup.
    pub last_cleanup: Option<CleanupReport>,
    /// The folder's size over time.
    pub folder_sizes: SizeSeries,
    /// What the runner and its processes used over time (ADR 0007).
    pub usage: UsageStore,
    /// The process id of the runner the service last started (to adopt it after a service restart).
    pub pid: Option<u32>,
    /// "Keep longer" extensions of leftover processes, so they survive a service restart. A damaged record is skipped.
    #[serde(deserialize_with = "lenient_keeps")]
    pub keeps: Vec<KeepRecord>,
}

/// Reads the records one by one: a damaged one is dropped, the rest load.
fn lenient_keeps<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<KeepRecord>, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    let Some(items) = v.as_array() else {
        return Ok(Vec::new());
    };
    Ok(items
        .iter()
        .filter_map(|i| serde_json::from_value::<KeepRecord>(i.clone()).ok())
        .take(crate::process_track::MAX_KEEP_RECORDS)
        .collect())
}

impl Default for RunnerState {
    fn default() -> Self {
        RunnerState {
            wanted: true,
            paused: false,
            history: History::new(),
            last_update: None,
            latest: None,
            checked_at: None,
            announced: None,
            last_cleanup: None,
            folder_sizes: SizeSeries::default(),
            usage: UsageStore::for_runner(),
            pid: None,
            keeps: Vec::new(),
        }
    }
}

/// The whole state.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ServiceState {
    /// Per runner, by name.
    pub runners: BTreeMap<String, RunnerState>,
    /// The machine's metrics.
    pub metrics: MetricsStore,
}

/// The file's shape.
#[derive(Serialize)]
struct Envelope<'a> {
    version: u32,
    runners: &'a BTreeMap<String, RunnerState>,
    metrics: &'a MetricsStore,
}

/// The result of [`load`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedState {
    /// The state to use.
    pub state: ServiceState,
    /// What went wrong, for the service log.
    pub problems: Vec<String>,
}

/// Loads the state from the file's text and its backup's (`None`: no file).
pub fn load(primary: Option<&str>, backup: Option<&str>) -> LoadedState {
    let Some(text) = primary else {
        return LoadedState {
            state: ServiceState::default(),
            problems: vec![],
        };
    };
    match parse(text) {
        Ok((state, problems)) => LoadedState { state, problems },
        Err(why) => {
            let mut problems = vec![format!("the state file is damaged ({why})")];
            match backup.map(parse) {
                Some(Ok((state, more))) => {
                    problems.push("the backup is used".into());
                    problems.extend(more);
                    LoadedState { state, problems }
                }
                Some(Err(why)) => {
                    problems.push(format!("the backup is damaged too ({why}); starting with empty state"));
                    LoadedState {
                        state: ServiceState::default(),
                        problems,
                    }
                }
                None => LoadedState {
                    state: ServiceState::default(),
                    problems,
                },
            }
        }
    }
}

fn parse(text: &str) -> Result<(ServiceState, Vec<String>), String> {
    if text.len() > MAX_STATE_BYTES {
        return Err(format!("it is larger than {} MB", MAX_STATE_BYTES / (1024 * 1024)));
    }
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let version = v.get("version").and_then(serde_json::Value::as_u64).ok_or("it has no version")?;
    let mut problems = vec![];
    if version > u64::from(STATE_VERSION) {
        problems.push(format!(
            "it was written by a newer Pitcrew (version {version}); it is read as far as it is understood"
        ));
    }
    let mut state = ServiceState::default();
    if let Some(m) = v.get("metrics") {
        match serde_json::from_value::<MetricsStore>(m.clone()) {
            Ok(m) => state.metrics = m,
            Err(e) => problems.push(format!("the metrics were dropped: {e}")),
        }
    }
    if let Some(runners) = v.get("runners").and_then(serde_json::Value::as_object) {
        for (name, entry) in runners {
            match serde_json::from_value::<RunnerState>(entry.clone()) {
                Ok(mut r) => {
                    // Re-bound and sort the history in case the file was edited.
                    r.history = History::from_events(r.history.events().cloned().collect::<Vec<HistoryEvent>>());
                    state.runners.insert(name.clone(), r);
                }
                Err(e) => problems.push(format!("the state of runner {name} was dropped: {e}")),
            }
        }
    }
    Ok((state, problems))
}

/// Serialises the state for saving.
pub fn to_json(state: &ServiceState) -> Result<String, String> {
    serde_json::to_string(&Envelope {
        version: STATE_VERSION,
        runners: &state.runners,
        metrics: &state.metrics,
    })
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::HistoryKind;

    #[test]
    fn round_trips() {
        let mut s = ServiceState::default();
        let mut r = RunnerState::default();
        r.history.push(Timestamp(5), HistoryKind::Started);
        r.paused = true;
        r.latest = Some(RunnerVersion::parse("5.2.0").unwrap());
        s.runners.insert("a".into(), r);
        let json = to_json(&s).unwrap();
        let back = load(Some(&json), None);
        assert!(back.problems.is_empty());
        assert_eq!(back.state, s);
        assert_eq!(load(None, None).state, ServiceState::default());
    }

    #[test]
    fn damage_falls_back_and_bad_entries_are_dropped() {
        let good = r#"{"version":1,"runners":{"a":{"wanted":false},"b":{"history":"nope"}}}"#;
        let l = load(Some(good), None);
        assert!(!l.state.runners["a"].wanted);
        assert!(!l.state.runners.contains_key("b"));
        assert_eq!(l.problems.len(), 1);
        let l = load(Some("{broken"), Some(good));
        assert!(l.state.runners.contains_key("a"));
        assert!(l.problems[0].contains("damaged"));
        let l = load(Some("{broken"), Some("also broken"));
        assert!(l.state.runners.is_empty());
        assert!(l.problems[1].contains("damaged too"));
        let l = load(Some("[]"), None);
        assert!(l.state.runners.is_empty() && !l.problems.is_empty());
        let bad_metrics = load(Some(r#"{"version":1,"metrics":7}"#), None);
        assert!(bad_metrics.problems[0].contains("metrics"));
        let newer = load(Some(r#"{"version":9}"#), None);
        assert!(newer.problems[0].contains("newer"));
        assert!(load(Some(&" ".repeat(MAX_STATE_BYTES + 1)), None).problems[0].contains("larger"));
        // A missing field gets its default: a runner is wanted unless the user stopped it.
        assert!(load(Some(r#"{"version":1,"runners":{"x":{}}}"#), None).state.runners["x"].wanted);
    }

    #[test]
    fn keep_longer_records_persist_and_damage_is_tolerated() {
        let mut s = ServiceState::default();
        let mut r = RunnerState::default();
        r.keeps = vec![KeepRecord {
            pid: 4,
            started_ms: 9,
            extra_ms: 3_600_000,
        }];
        s.runners.insert("a".into(), r);
        let back = load(Some(&to_json(&s).unwrap()), None);
        assert_eq!(back.state.runners["a"].keeps, s.runners["a"].keeps);
        // One damaged record is skipped; a non-list is ignored; an old file without the key loads.
        let text =
            r#"{"version":1,"runners":{"a":{"keeps":[{"pid":1,"startedMs":2,"extraMs":3},{"pid":"x"},7]},"b":{"keeps":"nope"},"c":{}}}"#;
        let l = load(Some(text), None);
        assert!(l.problems.is_empty(), "{:?}", l.problems);
        assert_eq!(l.state.runners["a"].keeps.len(), 1);
        assert!(l.state.runners["b"].keeps.is_empty() && l.state.runners["c"].keeps.is_empty());
    }
}
