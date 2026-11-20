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
    /// Priority is bounded: a priority marker older than this, or a waiter that has waited this long, no longer yields to it
    /// (ADR 0102). Default 1800 s: twice the 15-minute run budget of the push gate that is the priority holder, so a healthy
    /// priority run is always inside it and a hung one cannot hold everybody for good.
    pub priority_max: Duration,
    /// The same bound for the marker of a CI job on this host (`heavy mark ci-...`, ADR 0102): a job runs for up to its
    /// `timeout-minutes` (the longest host job's is 120), so the bound is 7200 s, not the push gate's 1800 s.
    pub ci_priority_max: Duration,
    /// A container run that has waited this long for the machine to be free of other steps makes everyone else wait for it
    /// (a drain marker), so a stream of other steps cannot keep it out for good. Default: a sixth of `priority_max`.
    pub drain_after: Duration,
    /// The most heavy steps that may run together (the "Auto" default is `gate::auto_max_slots`, at most 5). How many do run
    /// together at a given moment is `gate::allowed_slots`: this is only its upper bound; 1 means one at a time, always.
    pub slots: u32,
    /// The number of CPU cores the slot rule counts against the load (`HEAVY_CPUS`, else what the OS says).
    pub cpus: u32,
    /// The troubleshooting log file (None: the default under `root`).
    pub log_file: Option<PathBuf>,
    /// Echo every log event to stderr.
    pub verbose: bool,
    /// Keep the tail of the command's stderr for the failure report (stderr then is a pipe).
    pub stderr_tail: bool,
    /// The command inherits heavy's standard input (`HEAVY_STDIN=inherit`, an explicit opt-in); by default it reads `/dev/null`.
    pub stdin_inherit: bool,
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
        let priority_max = secs(get, &["HEAVY_PRIORITY_MAX"], 1800.0, 1.0, &mut w);
        let ci_priority_max = secs(get, &["HEAVY_CI_MAX"], 7200.0, 1.0, &mut w);
        let cpus = match first(get, &["HEAVY_CPUS"]) {
            None => std::thread::available_parallelism().map_or(1, |n| u32::try_from(n.get()).unwrap_or(u32::MAX)),
            Some(v) => match v.trim().parse::<u32>() {
                Ok(n) if n >= 1 => n,
                _ => {
                    w.push(format!("HEAVY_CPUS={v:?} is not a number >= 1; using the OS's count"));
                    std::thread::available_parallelism().map_or(1, |n| u32::try_from(n.get()).unwrap_or(u32::MAX))
                }
            },
        };
        // HEAVY_SLOTS is the most steps that may run together: `auto` (the default) or 1..64. How many run at a moment follows the
        // load (gate::allowed_slots); the shell plock that kept one lock is retired, so no other variable has to opt in.
        let slots = match first(get, &["HEAVY_SLOTS"]) {
            None => crate::gate::auto_max_slots(cpus),
            Some(v) if v.trim().eq_ignore_ascii_case("auto") => crate::gate::auto_max_slots(cpus),
            Some(v) => match v.trim().parse::<u32>() {
                Ok(n) if (1..=64).contains(&n) => n,
                _ => {
                    w.push(format!("HEAVY_SLOTS={v:?} is not `auto` or 1..64; using auto"));
                    crate::gate::auto_max_slots(cpus)
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
            priority_max: Duration::from_secs_f64(priority_max),
            ci_priority_max: Duration::from_secs_f64(ci_priority_max),
            slots,
            drain_after: Duration::from_secs_f64((priority_max / 6.0).max(1.0)),
            cpus,
            log_file: first(get, &["HEAVY_LOG_FILE"]).map(PathBuf::from),
            verbose: flag("HEAVY_VERBOSE"),
            stderr_tail: get("HEAVY_STDERR_TAIL").is_none_or(|v| v != "0"),
            stdin_inherit: get("HEAVY_STDIN").is_some_and(|v| v == "inherit"),
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
    fn stdin_inherit_is_an_explicit_opt_in() {
        assert!(!cfg(&[]).0.stdin_inherit);
        assert!(!cfg(&[("HEAVY_STDIN", "1")]).0.stdin_inherit);
        assert!(cfg(&[("HEAVY_STDIN", "inherit")]).0.stdin_inherit);
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
    fn heavy_sh_slot_folder_is_ignored_with_a_warning() {
        let (c, w) = cfg(&[("PLOCK_DIR", "/a"), ("HEAVY_LOCK_DIR", "/slots")]);
        assert_eq!(c.root, PathBuf::from("/a"));
        assert!(w.iter().any(|m| m.contains("HEAVY_LOCK_DIR")), "{w:?}");
    }

    #[test]
    fn bad_values_fall_back_with_a_warning() {
        let (c, w) = cfg(&[
            ("HEAVY_POLL", "abc"),
            ("HEAVY_SLOTS", "0"),
            ("HEAVY_GATE_LOAD", "-1"),
            ("HEAVY_CPUS", "4"),
        ]);
        assert_eq!((c.poll, c.slots, c.gate_load), (Duration::from_secs(5), 2, 30.0));
        assert_eq!(w.len(), 3, "{w:?}");
    }

    #[test]
    fn the_priority_bound_has_a_default_and_a_setting() {
        let (c, w) = cfg(&[]);
        assert_eq!(c.priority_max, Duration::from_secs(1800));
        assert_eq!(c.ci_priority_max, Duration::from_secs(7200));
        assert!(w.is_empty());
        let (c, _) = cfg(&[("HEAVY_PRIORITY_MAX", "90")]);
        assert_eq!(c.priority_max, Duration::from_secs(90));
        let (c, w) = cfg(&[("HEAVY_PRIORITY_MAX", "x")]);
        assert_eq!((c.priority_max, w.len()), (Duration::from_secs(1800), 1));
    }

    #[test]
    fn defaults() {
        let (c, _) = Config::from_env(&|k| (k == "HEAVY_CPUS").then(|| "14".to_string()), true);
        assert_eq!(c.root, PathBuf::from(LEGACY_ROOT));
        assert_eq!((c.gate_load, c.slots, c.wait_log), (30.0, 5, Duration::from_secs(60)));
        assert!(c.log_path("2026-10-31").ends_with("heavy-logs/heavy.2026-10-31.log"));
    }
}
