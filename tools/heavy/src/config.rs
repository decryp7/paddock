//! Settings, from the environment and the command line. Names: `HEAVY_*` first, then the legacy `PLOCK_*` (and heavy.sh's
//! `HEAVY_LOCK_DIR`, `HEAVY_SLOTS`). A value that does not parse is reported and the default used (fail soft, say why).

use std::path::PathBuf;
use std::time::Duration;

/// The folder the shell plock has always used on the agents' Mac; the default when it exists.
pub const LEGACY_ROOT: &str = "/private/tmp/claude-501";

/// Everything the lock needs to know.
#[derive(Debug, Clone)]
pub struct Config {
    /// Folder holding the lock, tickets, markers and logs.
    pub root: PathBuf,
    /// Seconds between attempts.
    pub poll: Duration,
    /// Seconds between "waiting for X" lines.
    pub wait_log: Duration,
    /// An ownerless lock or ticket is broken/removed only after this many seconds.
    pub unreadable_limit: u64,
    /// The gate waits while the 1-minute load is at or above this.
    pub gate_load: f64,
    /// Seconds between gate checks.
    pub gate_poll: Duration,
    /// Number of slots (1 = the plock-compatible single lock).
    pub slots: u32,
    /// The troubleshooting log file (None: the default under `root`).
    pub log_file: Option<PathBuf>,
    /// Echo every log event to stderr.
    pub verbose: bool,
    /// Keep the tail of the command's stderr for the failure report (stderr then is a pipe).
    pub stderr_tail: bool,
    /// Allow a build folder inside the home folder (ER-14 override).
    pub allow_home_target: bool,
    /// How long to wait for the command's stderr pipe to close after the command exited (a background process may hold it).
    pub drain: Duration,
}

/// Problems found while reading settings (shown once, then the default is used).
pub type Warnings = Vec<String>;

fn secs(get: &dyn Fn(&str) -> Option<String>, names: &[&str], default: f64, min: f64, w: &mut Warnings) -> f64 {
    for n in names {
        if let Some(v) = get(n) {
            return match v.trim().parse::<f64>() {
                Ok(x) if x.is_finite() && x >= min => x,
                _ => {
                    w.push(format!("{n}={v:?} is not a number of seconds >= {min}; using {default}"));
                    default
                }
            };
        }
    }
    default
}

fn first(get: &dyn Fn(&str) -> Option<String>, names: &[&str]) -> Option<String> {
    names.iter().find_map(|n| get(n).filter(|v| !v.is_empty()))
}

impl Config {
    /// Settings from an environment lookup (`std::env::var` in the program, a map in tests).
    pub fn from_env(get: &dyn Fn(&str) -> Option<String>, legacy_exists: bool) -> (Config, Warnings) {
        let mut w = Warnings::new();
        let root = first(get, &["HEAVY_DIR", "PLOCK_DIR", "PLOCK_ROOT"])
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                if legacy_exists {
                    PathBuf::from(LEGACY_ROOT)
                } else {
                    let base = first(get, &["TMPDIR", "TEMP"]).unwrap_or_else(|| "/tmp".into());
                    PathBuf::from(base).join("heavy-lock")
                }
            });
        if let Some(v) = first(get, &["HEAVY_LOCK_DIR"]) {
            w.push(format!(
                "HEAVY_LOCK_DIR={v:?} is heavy.sh's slot folder and is ignored: heavy uses {} (set HEAVY_DIR to choose; heavy.sh callers must move to heavy at the same time)",
                root.display()
            ));
        }
        let poll = secs(get, &["HEAVY_POLL", "PLOCK_POLL"], 5.0, 0.01, &mut w);
        let wait_log = secs(get, &["HEAVY_WAIT_LOG", "PLOCK_WAIT_LOG"], 60.0, 0.0, &mut w);
        let unreadable = secs(get, &["HEAVY_UNREADABLE_LIMIT", "PLOCK_UNREADABLE_LIMIT"], 120.0, 0.0, &mut w);
        let gate_load = secs(get, &["HEAVY_GATE_LOAD", "PLOCK_GATE_LOAD"], 30.0, 0.0, &mut w);
        let gate_poll = secs(get, &["HEAVY_GATE_POLL", "PLOCK_GATE_POLL"], 20.0, 0.01, &mut w);
        // heavy.sh's HEAVY_SLOTS is often exported globally; the shell plock knows only one lock, so slots count only when
        // the folder is chosen explicitly with HEAVY_DIR (or --slots is given). Otherwise the variable is ignored, with a warning.
        let slots_var = first(get, &["HEAVY_SLOTS"]);
        if slots_var.is_some() && first(get, &["HEAVY_DIR"]).is_none() {
            w.push(
                "HEAVY_SLOTS is ignored without HEAVY_DIR (the shell plock shares one lock; use --slots or set HEAVY_DIR to opt in)".into(),
            );
        }
        let slots = match slots_var.filter(|_| first(get, &["HEAVY_DIR"]).is_some()) {
            None => 1,
            Some(v) => match v.trim().parse::<u32>() {
                Ok(n) if (1..=64).contains(&n) => n,
                _ => {
                    w.push(format!("HEAVY_SLOTS={v:?} is not 1..64; using 1"));
                    1
                }
            },
        };
        let flag = |n: &str| get(n).is_some_and(|v| v == "1");
        let cfg = Config {
            root,
            poll: Duration::from_secs_f64(poll),
            wait_log: Duration::from_secs_f64(wait_log),
            unreadable_limit: unreadable as u64,
            gate_load,
            gate_poll: Duration::from_secs_f64(gate_poll),
            slots,
            log_file: first(get, &["HEAVY_LOG_FILE"]).map(PathBuf::from),
            verbose: flag("HEAVY_VERBOSE"),
            stderr_tail: get("HEAVY_STDERR_TAIL").is_none_or(|v| v != "0"),
            allow_home_target: flag("HEAVY_ALLOW_HOME_TARGET"),
            drain: Duration::from_millis(secs(get, &["HEAVY_DRAIN_MS"], 500.0, 0.0, &mut w) as u64),
        };
        (cfg, w)
    }

    /// The log file for a day: `--log-file`, else `<root>/heavy-logs/heavy.<day>.log`.
    pub fn log_path(&self, day: &str) -> PathBuf {
        self.log_file
            .clone()
            .unwrap_or_else(|| self.root.join("heavy-logs").join(format!("heavy.{day}.log")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(pairs: &[(&str, &str)]) -> (Config, Warnings) {
        let m: std::collections::HashMap<String, String> = pairs.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect();
        Config::from_env(&|k| m.get(k).cloned(), false)
    }

    #[test]
    fn legacy_and_new_names() {
        let (c, w) = cfg(&[
            ("PLOCK_DIR", "/a"),
            ("PLOCK_POLL", "0.2"),
            ("PLOCK_WAIT_LOG", "1"),
            ("PLOCK_UNREADABLE_LIMIT", "3"),
        ]);
        assert!(w.is_empty());
        assert_eq!(
            (c.root, c.poll, c.unreadable_limit),
            (PathBuf::from("/a"), Duration::from_millis(200), 3)
        );
        let (c, _) = cfg(&[("PLOCK_DIR", "/a"), ("PLOCK_ROOT", "/b"), ("HEAVY_DIR", "/c")]);
        assert_eq!(c.root, PathBuf::from("/c"));
        let (c, _) = cfg(&[("PLOCK_ROOT", "/b")]);
        assert_eq!(c.root, PathBuf::from("/b"));
    }

    #[test]
    fn heavy_sh_variables_do_not_silently_diverge() {
        let (c, w) = cfg(&[("PLOCK_DIR", "/a"), ("HEAVY_LOCK_DIR", "/slots"), ("HEAVY_SLOTS", "5")]);
        assert_eq!((c.root, c.slots), (PathBuf::from("/a"), 1));
        assert!(
            w.iter().any(|m| m.contains("HEAVY_LOCK_DIR")) && w.iter().any(|m| m.contains("HEAVY_SLOTS is ignored")),
            "{w:?}"
        );
        let (c, w) = cfg(&[("HEAVY_DIR", "/a"), ("HEAVY_SLOTS", "5")]);
        assert_eq!(c.slots, 5);
        assert!(w.is_empty(), "{w:?}");
    }

    #[test]
    fn bad_values_fall_back_with_a_warning() {
        let (c, w) = cfg(&[("HEAVY_POLL", "abc"), ("HEAVY_SLOTS", "0"), ("HEAVY_GATE_LOAD", "-1")]);
        assert_eq!((c.poll, c.slots, c.gate_load), (Duration::from_secs(5), 1, 30.0));
        assert_eq!(w.len(), 3);
    }

    #[test]
    fn defaults() {
        let (c, _) = Config::from_env(&|_| None, true);
        assert_eq!(c.root, PathBuf::from(LEGACY_ROOT));
        assert_eq!((c.gate_load, c.slots, c.wait_log), (30.0, 1, Duration::from_secs(60)));
        assert!(c.log_path("2026-10-31").ends_with("heavy-logs/heavy.2026-10-31.log"));
    }
}
