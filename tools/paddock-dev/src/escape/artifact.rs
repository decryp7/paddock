// ci-runs-reader: the plan record's artifact: finds the ci.yml runs of a commit and reads their `plan-record` artifact; judges no run
//! Where the selection records live (ADR 0117 amendment D13, user 2026-11-27): an Actions artifact of the plan job's own run, so ci.yml
//! stays read-only (no job writes to the repository). The plan job writes the record to a file ([`FileSink`]) and its upload step
//! publishes it as the artifact [`ARTIFACT_NAME`] with `retention-days` [`super::RETENTION_DAYS`]; the nightly finds the runs of a
//! commit through the Actions API (the one [`Client`]) and reads the artifact ([`ArtifactStore`]). A missing, expired or unreadable
//! record is `Missing` or `Unreadable`, which the judgement reports as not measured, never clean and never a guess (ER-33).

use super::{Load, Record, Sink, Store, valid_commit};
use crate::ci_runs::client::Client;
use std::cell::RefCell;
use std::path::PathBuf;

/// The artifact's name, fixed: the reader asks for it by name.
pub const ARTIFACT_NAME: &str = "plan-record";
/// The file inside the artifact.
pub const RECORD_FILE: &str = "record.json";
/// The workflow whose runs hold the records.
pub const WORKFLOW: &str = "ci.yml";
/// The most runs of one commit the reader looks into (a commit has a push run and a few dispatches).
pub const MAX_RUNS_PER_COMMIT: usize = 5;
/// The largest record the reader takes: a record is a few KiB; this bounds a wrong or hostile artifact.
pub const MAX_RECORD_BYTES: u64 = 1 << 20;

/// The plan job's writer: the record as JSON in a file that the upload step publishes.
pub struct FileSink {
    /// The file (its folder is made).
    pub path: PathBuf,
}

impl Sink for FileSink {
    fn put(&self, r: &Record) -> Result<(), String> {
        let text = serde_json::to_string(&r.to_json()).map_err(|e| e.to_string())?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(&self.path, text).map_err(|e| format!("{}: {e}", self.path.display()))
    }
}

/// The nightly's reader: the records of the commits' ci.yml runs, through the Actions API with its own read token.
pub struct ArtifactStore<'a> {
    client: RefCell<Client<'a>>,
}

impl<'a> ArtifactStore<'a> {
    /// A reader over `client`.
    pub fn new(client: Client<'a>) -> Self {
        ArtifactStore {
            client: RefCell::new(client),
        }
    }

    /// The record one run's artifact holds: `Ok(None)` when the run has none (no artifact, or expired), `Err` when it cannot be read.
    fn of_run(&self, run: u64, commit: &str) -> Result<Option<Record>, String> {
        let mut c = self.client.borrow_mut();
        let arts = c.run_artifacts(run, ARTIFACT_NAME)?.unwrap_or_default();
        let Some(a) = arts.iter().find(|a| a.name == ARTIFACT_NAME && !a.expired) else {
            return Ok(None);
        };
        let Some(zip) = c.artifact_zip(a.id, MAX_RECORD_BYTES)? else {
            return Ok(None);
        };
        let text = unzip_one(&zip, RECORD_FILE, MAX_RECORD_BYTES)?;
        let r = Record::parse(&String::from_utf8_lossy(&text))?;
        if r.commit == commit {
            Ok(Some(r))
        } else {
            Err(format!("the record of run {run} is for {}, not {commit}", r.commit))
        }
    }
}

impl Store for ArtifactStore<'_> {
    fn get(&self, commit: &str) -> Load {
        if !valid_commit(commit) {
            return Load::Unreadable(format!("{commit:?} is not a commit id"));
        }
        let runs = match self.client.borrow_mut().runs_page_where(1, &format!("head_sha={commit}")) {
            Ok(p) => p.runs,
            Err(e) => return Load::Unreadable(format!("the runs of {commit} could not be listed: {e}")),
        };
        // Newest first (the API's order): the first readable record wins; a bad one is reported only when no run has a good one.
        let mut bad = None;
        for run in runs.iter().filter(|r| r.workflow() == WORKFLOW).take(MAX_RUNS_PER_COMMIT) {
            match self.of_run(run.id, commit) {
                Ok(Some(r)) => return Load::Ok(Box::new(r)),
                Ok(None) => {}
                Err(e) => bad = bad.or(Some(format!("run {}: {e}", run.id))),
            }
        }
        bad.map_or(Load::Missing, Load::Unreadable)
    }
}

fn u16_at(b: &[u8], i: usize) -> Option<usize> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?) as usize)
}

fn u32_at(b: &[u8], i: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?) as usize)
}

/// The entry `name` of a zip (what an artifact download is): stored or deflated, read through the central directory (a writer may
/// leave sizes out of the local header), at most `max` bytes. Zip64 and encryption are refused. Parsers never panic: every offset is
/// checked.
pub fn unzip_one(zip: &[u8], name: &str, max: u64) -> Result<Vec<u8>, String> {
    let bad = |what: &str| format!("the artifact is not a readable zip ({what})");
    // The end-of-central-directory record: the last 22 bytes plus an optional comment of at most 65535.
    let from = zip.len().saturating_sub(22 + 65_535);
    let eocd = (from..=zip.len().saturating_sub(22))
        .rev()
        .find(|&i| zip.get(i..i + 4) == Some(&[0x50, 0x4b, 0x05, 0x06]))
        .ok_or_else(|| bad("no end record"))?;
    let (count, mut at) = (
        u16_at(zip, eocd + 10).ok_or_else(|| bad("end record"))?,
        u32_at(zip, eocd + 16).ok_or_else(|| bad("end record"))?,
    );
    for _ in 0..count {
        if zip.get(at..at + 4) != Some(&[0x50, 0x4b, 0x01, 0x02]) {
            return Err(bad("central directory"));
        }
        let field = |off: usize| u32_at(zip, at + off).ok_or_else(|| bad("central directory"));
        let (method, csize, usize_, nlen, xlen, clen, local) = (
            u16_at(zip, at + 10).ok_or_else(|| bad("central directory"))?,
            field(20)?,
            field(24)?,
            u16_at(zip, at + 28).ok_or_else(|| bad("central directory"))?,
            u16_at(zip, at + 30).ok_or_else(|| bad("central directory"))?,
            u16_at(zip, at + 32).ok_or_else(|| bad("central directory"))?,
            field(42)?,
        );
        let entry = zip.get(at + 46..at + 46 + nlen).ok_or_else(|| bad("entry name"))?;
        at += 46 + nlen + xlen + clen;
        if entry != name.as_bytes() {
            continue;
        }
        if csize == 0xFFFF_FFFF || usize_ == 0xFFFF_FFFF {
            return Err(bad("zip64 is not supported"));
        }
        if usize_ as u64 > max {
            return Err(format!("the entry {name} is larger than {max} bytes"));
        }
        if zip.get(local..local + 4) != Some(&[0x50, 0x4b, 0x03, 0x04]) {
            return Err(bad("local header"));
        }
        let start = local
            + 30
            + u16_at(zip, local + 26).ok_or_else(|| bad("local header"))?
            + u16_at(zip, local + 28).ok_or_else(|| bad("local header"))?;
        let data = zip.get(start..start + csize).ok_or_else(|| bad("entry data"))?;
        return match method {
            0 => Ok(data.to_vec()),
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(data, max as usize)
                .map_err(|e| format!("the entry {name} does not inflate: {e:?}")),
            m => Err(bad(&format!("compression method {m}"))),
        };
    }
    Err(format!("the artifact holds no {name}"))
}
