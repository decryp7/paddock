//! The run log of `throttle run`: the command's stdout and stderr, as they arrive, in `<runs>/<agent>/<start>-<short-name>.log`
//! (ADR 0090's captured tool output, its minimal part). `<runs>` is `<dev-targets>/runs` (docs/dev-setup.md, "Where things live").
//!
//! A shared cache (ER-51): **owner** throttle (only it writes or removes these files, and only names of its own shape:
//! `<8 digits>T<6 digits>.<3 digits>Z-<name>.log`); **size limit** [`MAX_LOG_BYTES`] per log and [`KEEP_PER_AGENT`] logs per agent;
//! **eviction** at the start of every run: the agent's own logs beyond the newest [`KEEP_PER_AGENT`], and any agent's logs older than
//! [`MAX_AGE`].

use std::fs::File;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

/// The newest logs kept per agent folder (the one being written is one of them).
pub const KEEP_PER_AGENT: usize = 20;
/// Logs of any agent older than this are removed.
pub const MAX_AGE: Duration = Duration::from_secs(14 * 86_400);
/// The most bytes one log keeps; the rest of the output still reaches the terminal, and the log says where it was cut.
pub const MAX_LOG_BYTES: u64 = 128 * 1024 * 1024;
/// The most entries one eviction looks at (a folder filled by something else cannot keep a run busy).
const MAX_SCAN: usize = 4096;
/// The environment variable that names the dev-targets folder (docs/dev-setup.md).
pub const DEV_TARGETS_ENV: &str = "PADDOCK_DEV_TARGETS";

/// The dev-targets folder by the rule of `tools/paddock-dev/src/places.rs` (`dev_targets_of`, the canonical one; docs/known-copies.toml
/// `dev-places`): the named folder, else the parent of the worktree root; it must be absolute, below the root and without `..`.
/// `None` when neither gives such a folder.
pub fn dev_targets_of(set: Option<&str>, worktree_root: Option<&str>) -> Option<PathBuf> {
    let set = set.map(str::trim).filter(|v| !v.is_empty());
    let path = match (set, worktree_root.map(str::trim).filter(|v| !v.is_empty())) {
        (Some(v), _) => PathBuf::from(v.trim_end_matches('/')),
        (None, Some(root)) => Path::new(root.trim_end_matches('/'))
            .parent()
            .filter(|p| p.parent().is_some())?
            .to_path_buf(),
        (None, None) => return None,
    };
    let ok = path.is_absolute() && path.parent().is_some() && !path.components().any(|c| matches!(c, Component::ParentDir));
    ok.then_some(path)
}

/// A file or folder name made of `[A-Za-z0-9._-]` only (anything else becomes `-`), at most `max` characters, never empty or a dot name.
pub fn clean_name(s: &str, max: usize) -> String {
    let t: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .take(max)
        .collect();
    let t = t.trim_matches(|c| c == '-' || c == '.').to_string();
    if t.is_empty() { "run".into() } else { t }
}

/// The short name of a command: the program's file name without its extension, and its first argument when that is a word
/// (`cargo-test`, `npm-run`, `sh`).
pub fn short_name(command: &[String]) -> String {
    let prog = command
        .first()
        .map(|p| {
            Path::new(p)
                .file_stem()
                .map_or_else(|| p.clone(), |s| s.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    let sub = command
        .get(1)
        .filter(|a| !a.starts_with('-') && a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    clean_name(&sub.map_or(prog.clone(), |s| format!("{prog}-{s}")), 40)
}

/// Whether `name` has the shape of a run log throttle wrote (`20261125T072833.123Z-<name>.log`, maybe with `-<n>` before `.log`).
pub fn is_run_log(name: &str) -> bool {
    let b = name.as_bytes();
    name.len() > 25
        && name.ends_with(".log")
        && b[..8].iter().all(u8::is_ascii_digit)
        && b[8] == b'T'
        && b[9..15].iter().all(u8::is_ascii_digit)
        && b[15] == b'.'
        && b[16..19].iter().all(u8::is_ascii_digit)
        && b[19] == b'Z'
        && b[20] == b'-'
}

/// What one eviction removed and what it could not.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Pruned {
    /// The files removed.
    pub removed: Vec<PathBuf>,
    /// The files that could not be removed, with the reason.
    pub failed: Vec<(PathBuf, String)>,
}

fn run_logs(dir: &Path) -> Vec<(String, PathBuf)> {
    std::fs::read_dir(dir)
        .map(|r| {
            r.filter_map(Result::ok)
                .take(MAX_SCAN)
                .filter_map(|e| {
                    let n = e.file_name().to_string_lossy().into_owned();
                    (is_run_log(&n) && e.file_type().is_ok_and(|t| t.is_file())).then(|| (n, e.path()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Removes `agent`'s run logs beyond the newest `keep` (by the time in their names) and every agent's run logs last written before
/// `now - max_age`. Only files of a run log's shape are touched; `keep` counts the one about to be written, so `keep - 1` stay.
pub fn prune(runs: &Path, agent: &str, keep: usize, max_age: Duration, now: SystemTime) -> Pruned {
    let mut out = Pruned::default();
    let gone = |p: PathBuf, out: &mut Pruned| match std::fs::remove_file(&p) {
        Ok(()) => out.removed.push(p),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => out.failed.push((p, e.to_string())),
    };
    let mut mine = run_logs(&runs.join(agent));
    mine.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, p) in mine.into_iter().skip(keep.saturating_sub(1)) {
        gone(p, &mut out);
    }
    let Some(cutoff) = now.checked_sub(max_age) else {
        return out;
    };
    let agents = std::fs::read_dir(runs)
        .map(|r| {
            r.filter_map(Result::ok)
                .take(MAX_SCAN)
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_else(|_| Vec::new());
    for dir in agents {
        for (_, p) in run_logs(&dir) {
            if std::fs::metadata(&p).and_then(|m| m.modified()).is_ok_and(|t| t < cutoff) {
                gone(p, &mut out);
            }
        }
    }
    out
}

/// An open run log. Shared by the output pumps; writes past [`MAX_LOG_BYTES`] are dropped after one note.
#[derive(Debug)]
pub struct RunLog {
    path: PathBuf,
    file: Mutex<File>,
    written: AtomicU64,
    max: u64,
}

impl RunLog {
    /// Creates `<runs>/<agent>/<stamp>-<short>.log` (a new file; `-2` ... `-9` when the name is taken).
    pub fn create(runs: &Path, agent: &str, stamp: &str, short: &str, max: u64) -> std::io::Result<RunLog> {
        let dir = runs.join(agent);
        std::fs::create_dir_all(&dir)?;
        let mut last = None;
        for n in 1..10 {
            let name = if n == 1 {
                format!("{stamp}-{short}.log")
            } else {
                format!("{stamp}-{short}-{n}.log")
            };
            let path = dir.join(name);
            match File::options().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    return Ok(RunLog {
                        path,
                        file: Mutex::new(file),
                        written: AtomicU64::new(0),
                        max,
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or_else(|| std::io::Error::other("no free run log name")))
    }

    /// The file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends output, up to the size limit; a write error is ignored (the terminal still has the output).
    pub fn write(&self, bytes: &[u8]) {
        let Ok(mut f) = self.file.lock() else { return };
        let before = self.written.load(Ordering::SeqCst);
        if before >= self.max {
            return;
        }
        let room = usize::try_from(self.max - before).unwrap_or(usize::MAX);
        let part = &bytes[..bytes.len().min(room)];
        let _ = f.write_all(part);
        self.written.fetch_add(part.len() as u64, Ordering::SeqCst);
        if part.len() < bytes.len() {
            let _ = writeln!(
                f,
                "\n[throttle: this log stops at {} bytes; the rest went to the terminal only]",
                self.max
            );
        }
    }

    /// Appends throttle's own closing line.
    pub fn note(&self, line: &str) {
        if let Ok(mut f) = self.file.lock() {
            let _ = writeln!(f, "{line}");
            let _ = f.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_targets_follow_the_places_rule() {
        assert_eq!(dev_targets_of(Some("/srv/dt/"), Some("/x/wt")), Some(PathBuf::from("/srv/dt")));
        assert_eq!(
            dev_targets_of(None, Some("/srv/dev-targets/wt")),
            Some(PathBuf::from("/srv/dev-targets"))
        );
        assert_eq!(
            dev_targets_of(Some("  "), Some("/srv/dev-targets/wt/")),
            Some(PathBuf::from("/srv/dev-targets"))
        );
        for bad in [Some("rel/dt"), Some("/"), Some("/a/../b")] {
            assert_eq!(dev_targets_of(bad, None), None, "{bad:?}");
        }
        assert_eq!(dev_targets_of(None, Some("/wt")), None, "the parent would be the root");
        assert_eq!(dev_targets_of(None, None), None);
    }

    #[test]
    fn names() {
        let c = |s: &str| s.split_whitespace().map(String::from).collect::<Vec<_>>();
        assert_eq!(short_name(&c("cargo test -p x")), "cargo-test");
        assert_eq!(short_name(&c("/usr/bin/sh -c x")), "sh");
        assert_eq!(short_name(&c("node.exe ./a.mjs")), "node");
        assert_eq!(clean_name("implementer:RT 3/../x", 64), "implementer-RT-3-..-x");
        assert_eq!(clean_name("../..", 64), "run");
        assert!(is_run_log("20261125T072833.123Z-cargo-test.log"));
        assert!(is_run_log("20261125T072833.123Z-cargo-test-2.log"));
        assert!(!is_run_log("notes.log") && !is_run_log("20261125T072833.123Z-cargo-test.txt"));
    }

    #[test]
    fn the_log_takes_output_up_to_its_limit_and_says_where_it_stopped() {
        let d = tempfile::tempdir().unwrap();
        let log = RunLog::create(d.path(), "A", "20261125T072833.123Z", "sh", 10).unwrap();
        let second = RunLog::create(d.path(), "A", "20261125T072833.123Z", "sh", 10).unwrap();
        assert!(second.path().to_string_lossy().ends_with("-sh-2.log"));
        log.write(b"0123456");
        log.write(b"789abc");
        log.write(b"more");
        let text = std::fs::read_to_string(log.path()).unwrap();
        assert!(text.starts_with("0123456789\n[throttle: this log stops at 10 bytes"), "{text}");
        assert!(!text.contains("abc") && !text.contains("more"));
    }

    #[test]
    fn prune_keeps_the_newest_per_agent_and_drops_old_ones_of_every_agent_only_of_its_own_shape() {
        let d = tempfile::tempdir().unwrap();
        let runs = d.path();
        std::fs::create_dir_all(runs.join("A")).unwrap();
        std::fs::create_dir_all(runs.join("B")).unwrap();
        for i in 0..5 {
            std::fs::write(runs.join(format!("A/2026112{i}T000000.000Z-x.log")), "").unwrap();
        }
        std::fs::write(runs.join("A/keep-me.log"), "").unwrap();
        std::fs::write(runs.join("B/20261101T000000.000Z-y.log"), "").unwrap();
        // Positive control first: with no age limit reached, B's log stays.
        let p = prune(runs, "A", 3, MAX_AGE, SystemTime::now());
        assert_eq!(p.removed.len(), 3, "{p:?}");
        let left: Vec<_> = run_logs(&runs.join("A")).into_iter().map(|(n, _)| n).collect();
        let mut left = left;
        left.sort();
        assert_eq!(left, ["20261123T000000.000Z-x.log", "20261124T000000.000Z-x.log"]);
        assert!(runs.join("A/keep-me.log").exists() && runs.join("B/20261101T000000.000Z-y.log").exists());
        // Now everything is older than the limit (the clock is a day past it): every run log goes, the foreign file stays.
        let later = SystemTime::now() + MAX_AGE + Duration::from_secs(86_400);
        let p = prune(runs, "A", 3, MAX_AGE, later);
        assert_eq!(p.removed.len(), 3, "{p:?}");
        assert!(runs.join("A/keep-me.log").exists());
    }
}
