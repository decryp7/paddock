//! Records and the store (ADR 0116 §3, §4). A record is written only for a target that ran in full and passed on the first try; reading
//! one checks the schema, the key, the target and the class again, so anything else is a miss. The [`Store`] is a port: [`FsStore`]
//! is the local one (`<git common dir>/paddock-pre-push/test-cache`, one small file per record, as [`crate::pre_push::reuse`] keeps
//! its rows), and the RustFS bucket of §4 is the CI adapter behind the same trait.

use super::key::{RunnerClass, SCHEMA};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Records older than this are evicted by the local store (the lifecycle rule of the bucket, §4).
pub const EVICT_AFTER: Duration = Duration::from_secs(30 * 24 * 3600);

/// One passed target.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// The key it was written under.
    pub key: String,
    /// The target id.
    pub target: String,
    /// The runner class.
    pub class: RunnerClass,
    /// The nextest filterset the run was narrowed to ([`super::key::normalize_filter`]); empty for a whole-package run. Part of the key.
    pub filter: String,
    /// The run (a CI run id, or the local run's token) the record came from.
    pub run: String,
    /// The CI job's display name (`crate (macos-latest)`), which the reader finds among the run's jobs; empty for a local run.
    pub job: String,
    /// The run's attempt the job ran in; 0 for a local run.
    pub attempt: u64,
    /// The ref the run was on (`refs/heads/main`, `refs/heads/train/...`, or a local branch).
    pub git_ref: String,
    /// The commit the run tested.
    pub commit: String,
    /// How many tests passed.
    pub tests: u64,
    /// How long they took, seconds.
    pub secs: f64,
    /// When it was written, in Unix seconds.
    pub at: u64,
    /// The runner's OS build as observed: reported, not keyed (decision D5).
    pub os_build: String,
}

impl Record {
    /// The record as its file holds it.
    pub fn json(&self) -> Value {
        json!({"schema": SCHEMA, "key": self.key, "target": self.target, "class": self.class.0, "verdict": "pass", "filter": self.filter,
               "run": self.run, "job": self.job, "attempt": self.attempt, "ref": self.git_ref, "commit": self.commit, "tests": self.tests, "secs": self.secs, "at": self.at,
               "os_build": self.os_build})
    }

    /// The record out of `text`, when it is a pass record of this schema for exactly this key, target and class.
    pub fn parse(text: &str, key: &str, target: &str, class: &RunnerClass) -> Option<Record> {
        let v: Value = serde_json::from_str(text).ok()?;
        let ok = v["schema"] == json!(SCHEMA)
            && v["key"] == json!(key)
            && v["target"] == json!(target)
            && v["class"] == json!(class.0)
            && v["verdict"] == json!("pass");
        let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
        ok.then(|| Record {
            key: key.to_string(),
            target: target.to_string(),
            class: class.clone(),
            filter: s("filter"),
            run: s("run"),
            job: s("job"),
            attempt: v["attempt"].as_u64().unwrap_or(0),
            git_ref: s("ref"),
            commit: s("commit"),
            tests: v["tests"].as_u64().unwrap_or(0),
            secs: v["secs"].as_f64().unwrap_or(0.0),
            at: v["at"].as_u64().unwrap_or(0),
            os_build: s("os_build"),
        })
    }

    /// The one line the plan log prints for a reused target (§5): the target, the key, the run it rests on.
    pub fn line(&self) -> String {
        let when = i64::try_from(self.at)
            .ok()
            .and_then(|s| jiff::Timestamp::from_second(s).ok())
            .map_or_else(|| format!("unix {}", self.at), |t| t.strftime("%Y-%m-%d").to_string());
        let job = if self.job.is_empty() {
            String::new()
        } else {
            format!(" job `{}` attempt {}", self.job, self.attempt)
        };
        format!(
            "test-cache: reused target={}@{} key={} from run {}{job} (commit {}, {} tests, {when})",
            self.target,
            self.class.0,
            self.key.get(..12).unwrap_or(&self.key),
            self.run,
            self.commit.get(..9).unwrap_or(&self.commit),
            self.tests,
        )
    }
}

/// What a store can say it could not do.
#[derive(Debug, PartialEq, Eq)]
pub struct StoreError(pub String);

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A key to look up: the target and class are part of where a record lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lookup {
    /// The target id.
    pub target: String,
    /// The runner class.
    pub class: RunnerClass,
    /// The key.
    pub key: String,
}

/// The store port. Both calls are bulk: N targets cost one call (ER-20); an error is for the whole call, and a caller turns reuse off
/// for the run when it gets one (fail open, §5).
pub trait Store {
    /// One answer per lookup, in order: the record, or `None` for a miss.
    fn get_many(&self, lookups: &[Lookup]) -> Result<Vec<Option<Record>>, StoreError>;
    /// Writes the records, replacing one with the same key.
    fn put_many(&self, records: &[Record]) -> Result<(), StoreError>;
    /// Deletes the records under these keys and returns each one that was there (the nightly's invalidation names the run a deleted
    /// pass came from, ADR 0116 §6). One answer per lookup, in order.
    fn delete_many(&self, lookups: &[Lookup]) -> Result<Vec<Option<Record>>, StoreError>;
}

/// The local store: `<dir>/v<schema>/<class>/<target slug>/<key>.json`.
pub struct FsStore {
    dir: PathBuf,
}

/// Where a record lives, relative to a store's root: `v<schema>/<class>/<target slug>/<key>.json` (the one layout of every store).
pub fn rel_path(target: &str, class: &RunnerClass, key: &str) -> String {
    format!("v{SCHEMA}/{}/{}/{key}.json", class.0, slug(target))
}

fn slug(target: &str) -> String {
    target
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

impl FsStore {
    /// A store in `dir` (made when a record is written).
    pub fn new(dir: &Path) -> FsStore {
        FsStore { dir: dir.to_path_buf() }
    }

    fn path(&self, target: &str, class: &RunnerClass, key: &str) -> PathBuf {
        self.dir.join(rel_path(target, class, key))
    }

    /// Removes the records older than `after` and returns how many went. Anything in the folder that is not a record file is left alone.
    pub fn evict(&self, now: u64, after: Duration) -> usize {
        let mut gone = 0;
        for f in self.files() {
            let at = std::fs::metadata(&f)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
            if at.is_some_and(|a| now.saturating_sub(a.as_secs()) > after.as_secs()) && std::fs::remove_file(&f).is_ok() {
                gone += 1;
            }
        }
        gone
    }

    /// Every record file below the schema folder.
    fn files(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut todo = vec![self.dir.join(format!("v{SCHEMA}"))];
        while let Some(d) = todo.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    todo.push(p);
                } else if p.extension().is_some_and(|x| x == "json") {
                    out.push(p);
                }
            }
        }
        out
    }

    /// The size in bytes and the oldest write time (Unix seconds) of the store, for `audit`.
    pub fn usage(&self) -> (u64, usize, Option<u64>) {
        let (mut bytes, mut n, mut oldest) = (0, 0, None::<u64>);
        for f in self.files() {
            let Ok(m) = std::fs::metadata(&f) else { continue };
            bytes += m.len();
            n += 1;
            if let Some(t) = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()) {
                oldest = Some(oldest.map_or(t.as_secs(), |o| o.min(t.as_secs())));
            }
        }
        (bytes, n, oldest)
    }
}

impl Store for FsStore {
    fn delete_many(&self, lookups: &[Lookup]) -> Result<Vec<Option<Record>>, StoreError> {
        let found = self.get_many(lookups)?;
        for l in lookups {
            let p = self.path(&l.target, &l.class, &l.key);
            match std::fs::remove_file(&p) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(StoreError(format!("{} cannot be deleted: {e}", p.display()))),
            }
        }
        Ok(found)
    }

    fn get_many(&self, lookups: &[Lookup]) -> Result<Vec<Option<Record>>, StoreError> {
        Ok(lookups
            .iter()
            .map(|l| {
                let text = std::fs::read_to_string(self.path(&l.target, &l.class, &l.key)).ok()?;
                Record::parse(&text, &l.key, &l.target, &l.class)
            })
            .collect())
    }

    fn put_many(&self, records: &[Record]) -> Result<(), StoreError> {
        for r in records {
            let p = self.path(&r.target, &r.class, &r.key);
            let err = |e: std::io::Error| StoreError(format!("{} cannot be written: {e}", p.display()));
            std::fs::create_dir_all(p.parent().unwrap_or(&self.dir)).map_err(err)?;
            // A temp file and a rename: a reader never sees half a record, and two worktrees writing one key leave one whole file.
            let tmp = p.with_extension(format!("tmp{}", std::process::id()));
            std::fs::write(&tmp, serde_json::to_string(&r.json()).unwrap_or_default()).map_err(err)?;
            std::fs::rename(&tmp, &p).map_err(err)?;
        }
        Ok(())
    }
}
