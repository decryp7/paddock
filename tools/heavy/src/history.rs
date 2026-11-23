//! The history of how long each row ran, so a row that takes seconds is not queued behind one that takes minutes.
//!
//! Owner: `heavy` (the only writer). One file, `heavy-history.tsv`, in the lock folder: a line per finished run,
//! `<epoch seconds>\t<milliseconds>\t<row key>`. Bounded (ER-51): at most [`KEEP_PER_ROW`] lines per row and [`MAX_LINES`] in all; a write
//! drops the oldest lines first. A row's key is the caller's name for it plus its workspace (`HEAVY_ROW`, for example
//! `clippy apps/dyno`), never the run's own name, which differs every run. The file is a hint: a writer that loses a race loses a
//! line, and a missing or unreadable file means "no history", which never makes a row quick (ER-33).

use std::fs;
use std::path::{Path, PathBuf};

/// The history's file name in the lock folder.
pub const FILE: &str = "heavy-history.tsv";
/// Most lines kept in all; the oldest go first.
pub const MAX_LINES: usize = 512;
/// Most lines kept for one row, so a busy row cannot push the others out.
pub const KEEP_PER_ROW: usize = 5;
/// Most recent lines a row's expected time is taken from: its slowest, so a row that is sometimes slow (a cold build) is not quick.
pub const SAMPLES: usize = 3;
/// Longest row key kept, in bytes.
pub const KEY_MAX: usize = 160;
/// Largest file read, in bytes: a bigger one is not ours (or was damaged) and is read as no history.
const READ_MAX: u64 = 256 * 1024;

/// The history of one lock folder.
#[derive(Debug, Clone)]
pub struct History {
    path: PathBuf,
}

/// A key made safe for one line of the file: no tab or line break, bounded.
pub fn clean_key(key: &str) -> String {
    let mut out: String = key.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let mut end = out.len().min(KEY_MAX);
    while !out.is_char_boundary(end) {
        end -= 1;
    }
    out.truncate(end);
    out.trim().to_string()
}

impl History {
    /// The history in the lock folder `root`.
    pub fn new(root: &Path) -> History {
        History { path: root.join(FILE) }
    }

    /// The file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read(&self) -> Vec<(u64, u64, String)> {
        let Ok(meta) = fs::metadata(&self.path) else { return Vec::new() };
        if meta.len() > READ_MAX {
            return Vec::new();
        }
        let Ok(text) = fs::read_to_string(&self.path) else {
            return Vec::new();
        };
        text.lines()
            .filter_map(|l| {
                let mut it = l.splitn(3, '\t');
                let epoch = it.next()?.parse().ok()?;
                let ms = it.next()?.parse().ok()?;
                let key = it.next()?.to_string();
                (!key.is_empty()).then_some((epoch, ms, key))
            })
            .collect()
    }

    /// How long the row usually runs, in seconds: the slowest of its last [`SAMPLES`] runs; `None` when it has no history.
    pub fn expected_secs(&self, key: &str) -> Option<f64> {
        let key = clean_key(key);
        if key.is_empty() {
            return None;
        }
        let rows: Vec<u64> = self.read().into_iter().filter(|r| r.2 == key).map(|r| r.1).collect();
        let last = &rows[rows.len().saturating_sub(SAMPLES)..];
        last.iter().max().map(|ms| *ms as f64 / 1000.0)
    }

    /// Adds a finished run and trims the file (oldest first). Err: the file could not be written; the run is not affected.
    pub fn record(&self, key: &str, secs: f64, epoch: u64) -> std::io::Result<()> {
        let key = clean_key(key);
        if key.is_empty() || !secs.is_finite() || secs < 0.0 {
            return Ok(());
        }
        let mut lines = self.read();
        lines.push((epoch, (secs * 1000.0).round() as u64, key));
        // Per row first (newest KEEP_PER_ROW), then overall (newest MAX_LINES), both oldest-first.
        let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        let mut keep = vec![true; lines.len()];
        for (i, l) in lines.iter().enumerate().rev() {
            let n = seen.entry(l.2.clone()).or_insert(0);
            *n += 1;
            if *n > KEEP_PER_ROW {
                keep[i] = false;
            }
        }
        let mut kept: Vec<_> = lines.into_iter().zip(keep).filter_map(|(l, k)| k.then_some(l)).collect();
        if kept.len() > MAX_LINES {
            kept.drain(..kept.len() - MAX_LINES);
        }
        let body: String = kept.iter().map(|(e, ms, k)| format!("{e}\t{ms}\t{k}\n")).collect();
        let tmp = self.path.with_extension(format!("tmp{}", std::process::id()));
        fs::write(&tmp, body)?;
        fs::rename(&tmp, &self.path).inspect_err(|_| {
            let _ = fs::remove_file(&tmp);
        })
    }

    /// Lines in the file now.
    pub fn len(&self) -> usize {
        self.read().len()
    }

    /// Whether the file holds no line.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
