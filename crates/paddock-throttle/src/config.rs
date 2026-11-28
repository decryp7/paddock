//! Settings, from the environment and the command line. Names: `THROTTLE_*` first, then the legacy `PLOCK_*` (and heavy.sh's
//! `HEAVY_LOCK_DIR`, `THROTTLE_SLOTS`). A value that does not parse is reported and the default used (fail soft, say why).

use crate::places::{self, Host};
use std::path::PathBuf;
use std::time::Duration;

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
    /// The same bound for the marker of a CI job on this host (`throttle mark ci-...`, ADR 0102): a job runs for up to its
    /// `timeout-minutes` (the longest host job's is 120), so the bound is 7200 s, not the push gate's 1800 s.
    pub ci_priority_max: Duration,
    /// A container run that has waited this long for the machine to be free of other steps makes everyone else wait for it
    /// (a drain marker), so a stream of other steps cannot keep it out for good. Default: a sixth of `priority_max`.
    pub drain_after: Duration,
    /// The most heavy steps that may run together (the "Auto" default is `gate::auto_max_slots`, at most `gate::MAX_AUTO_SLOTS`). How many do run
    /// together at a given moment is `gate::allowed_slots`: this is only its upper bound; 1 means one at a time, always.
    pub slots: u32,
    /// The number of CPU cores the slot rule counts against the load (`THROTTLE_CPUS`, else what the OS says).
    pub cpus: u32,
    /// The lock folder this one moved from ([`places::moved_from`]: `THROTTLE_MOVED_FROM`, else `<dev-targets>/throttle` or the legacy
    /// `/private/tmp/claude-501` when it holds a history). Its history file is read once ([`crate::history::History::of`]). None: nothing moved, nothing else is read.
    pub moved_from: Option<PathBuf>,
    /// The troubleshooting log file (None: the default under `root`).
    pub log_file: Option<PathBuf>,
    /// Echo every log event to stderr.
    pub verbose: bool,
    /// Keep the tail of the command's stderr for the failure report (stderr then is a pipe).
    pub stderr_tail: bool,
    /// The command inherits throttle's standard input (`THROTTLE_STDIN=inherit`, an explicit opt-in); by default it reads `/dev/null`.
    pub stdin_inherit: bool,
    /// Allow a build folder inside the home folder (ER-14 override).
    pub allow_home_target: bool,
    /// How long to wait for the command's output pipes to close after the command exited (a background process may hold them).
    pub drain: Duration,
    /// The folder of the run logs (`crate::capture`): `<dev-targets>/runs`, resolved by the program at its edge; `None` is
    /// `<root>/runs` ([`Config::runs_dir`]).
    pub runs: Option<PathBuf>,
    /// The hang detector's default (`THROTTLE_STALL`, seconds; `--stall` wins): off unless set.
    pub stall: Option<Duration>,
    /// The queue status file of the caller's row (`THROTTLE_STATUS_FILE`, [`crate::status`]): every run appends its queued, admitted
    /// and done events to it. `None`: nothing is written.
    pub status_file: Option<PathBuf>,
}

/// Problems found while reading settings (shown once, then the default is used).
pub type Warnings = Vec<String>;

fn secs(
    get: &dyn Fn(&str) -> Option<String>,
    names: &[&str],
    default: f64,
    min: f64,
    w: &mut Warnings,
) -> f64 {
    for n in names {
        if let Some(v) = get(n) {
            return match v.trim().parse::<f64>() {
                Ok(x) if x.is_finite() && x >= min => x,
                _ => {
                    w.push(format!(
                        "{n}={v:?} is not a number of seconds >= {min}; using {default}"
                    ));
                    default
                }
            };
        }
    }
    default
}

/// The legacy prefix: `HEAVY_X` is accepted for `THROTTLE_X` until the runner and every agent's binary have moved (the tool was called
/// `heavy` before 2026-11-26). `THROTTLE_X` wins when both are set.
pub fn aliased(get: &dyn Fn(&str) -> Option<String>, k: &str) -> Option<String> {
    get(k).or_else(|| {
        k.strip_prefix("THROTTLE_")
            .and_then(|rest| get(&format!("HEAVY_{rest}")))
    })
}

fn first(get: &dyn Fn(&str) -> Option<String>, names: &[&str]) -> Option<String> {
    names.iter().find_map(|n| get(n).filter(|v| !v.is_empty()))
}

impl Config {
    /// Settings from an environment lookup (`std::env::var` in the program, a map in tests).
    pub fn from_env(get: &dyn Fn(&str) -> Option<String>, host: &Host<'_>) -> (Config, Warnings) {
        let mut w = Warnings::new();
        let alias = |k: &str| aliased(get, k);
        let get: &dyn Fn(&str) -> Option<String> = &alias;
        let (root, source) = places::resolve_root(get, host, &mut w);
        if let Some(v) = first(get, &["HEAVY_LOCK_DIR"]) {
            w.push(format!(
                "HEAVY_LOCK_DIR={v:?} is heavy.sh's slot folder and is ignored: throttle uses {} (set THROTTLE_DIR to choose; heavy.sh callers must move to throttle at the same time)",
                root.display()
            ));
        }
        let poll = secs(get, &["THROTTLE_POLL", "PLOCK_POLL"], 5.0, 0.01, &mut w);
        let wait_log = secs(
            get,
            &["THROTTLE_WAIT_LOG", "PLOCK_WAIT_LOG"],
            60.0,
            0.0,
            &mut w,
        );
        let unreadable = secs(
            get,
            &["THROTTLE_UNREADABLE_LIMIT", "PLOCK_UNREADABLE_LIMIT"],
            120.0,
            0.0,
            &mut w,
        );
        let gate_load = secs(
            get,
            &["THROTTLE_GATE_LOAD", "PLOCK_GATE_LOAD"],
            30.0,
            0.0,
            &mut w,
        );
        let gate_poll = secs(
            get,
            &["THROTTLE_GATE_POLL", "PLOCK_GATE_POLL"],
            20.0,
            0.01,
            &mut w,
        );
        let priority_max = secs(get, &["THROTTLE_PRIORITY_MAX"], 1800.0, 1.0, &mut w);
        let ci_priority_max = secs(get, &["THROTTLE_CI_MAX"], 7200.0, 1.0, &mut w);
        let cpus = match first(get, &["THROTTLE_CPUS"]) {
            None => std::thread::available_parallelism()
                .map_or(1, |n| u32::try_from(n.get()).unwrap_or(u32::MAX)),
            Some(v) => match v.trim().parse::<u32>() {
                Ok(n) if n >= 1 => n,
                _ => {
                    w.push(format!(
                        "THROTTLE_CPUS={v:?} is not a number >= 1; using the OS's count"
                    ));
                    std::thread::available_parallelism()
                        .map_or(1, |n| u32::try_from(n.get()).unwrap_or(u32::MAX))
                }
            },
        };
        // THROTTLE_SLOTS is the most steps that may run together: `auto` (the default) or 1..64. How many run at a moment follows the
        // load (gate::allowed_slots); the shell plock that kept one lock is retired, so no other variable has to opt in.
        let slots = match first(get, &["THROTTLE_SLOTS"]) {
            None => crate::gate::auto_max_slots(cpus),
            Some(v) if v.trim().eq_ignore_ascii_case("auto") => crate::gate::auto_max_slots(cpus),
            Some(v) => match v.trim().parse::<u32>() {
                Ok(n) if (1..=64).contains(&n) => n,
                _ => {
                    w.push(format!(
                        "THROTTLE_SLOTS={v:?} is not `auto` or 1..64; using auto"
                    ));
                    crate::gate::auto_max_slots(cpus)
                }
            },
        };
        let flag = |n: &str| get(n).is_some_and(|v| v == "1");
        let cfg = Config {
            root: root.clone(),
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
            // The old lock folder with a history, named by places::moved_from (ER-22: no caller repeats it).
            moved_from: places::moved_from(get, host, &root, &source),
            log_file: first(get, &["THROTTLE_LOG_FILE"]).map(PathBuf::from),
            verbose: flag("THROTTLE_VERBOSE"),
            stderr_tail: get("THROTTLE_STDERR_TAIL").is_none_or(|v| v != "0"),
            stdin_inherit: get("THROTTLE_STDIN").is_some_and(|v| v == "inherit"),
            allow_home_target: flag("THROTTLE_ALLOW_HOME_TARGET"),
            drain: Duration::from_millis(
                secs(get, &["THROTTLE_DRAIN_MS"], 500.0, 0.0, &mut w) as u64
            ),
            runs: None,
            status_file: first(get, &[crate::status::STATUS_FILE_VAR]).map(PathBuf::from),
            stall: match first(get, &["THROTTLE_STALL"]) {
                None => None,
                Some(v) => match v.trim().parse::<f64>() {
                    Ok(x) if x.is_finite() && x > 0.0 => Some(Duration::from_secs_f64(x)),
                    _ => {
                        w.push(format!(
                            "THROTTLE_STALL={v:?} is not a number of seconds > 0; the hang detector stays off"
                        ));
                        None
                    }
                },
            },
        };
        (cfg, w)
    }

    /// The folder of the run logs: the one resolved at the edge, else `<root>/runs`.
    pub fn runs_dir(&self) -> PathBuf {
        self.runs.clone().unwrap_or_else(|| self.root.join("runs"))
    }

    /// The log file for a day: `--log-file`, else `<root>/throttle-logs/throttle.<day>.log`.
    pub fn log_path(&self, day: &str) -> PathBuf {
        self.log_file.clone().unwrap_or_else(|| {
            self.root
                .join("throttle-logs")
                .join(format!("throttle.{day}.log"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use paddock_testkit::path::abs;

    /// A path's text for an environment value (the invented folders are valid UTF-8).
    fn txt(p: &std::path::Path) -> &str {
        p.to_str().expect("an invented folder is UTF-8")
    }

    fn cfg(pairs: &[(&str, &str)]) -> (Config, Warnings) {
        let m: std::collections::HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect();
        Config::from_env(&|k| m.get(k).cloned(), &Host::none())
    }

    #[test]
    fn the_hang_detector_is_off_unless_set() {
        assert_eq!(cfg(&[]).0.stall, None);
        assert_eq!(
            cfg(&[("THROTTLE_STALL", "900")]).0.stall,
            Some(Duration::from_secs(900))
        );
        let (c, w) = cfg(&[("THROTTLE_STALL", "0")]);
        assert_eq!((c.stall, w.len()), (None, 1));
        let l = abs(["l"]);
        let (c, _) = cfg(&[("THROTTLE_DIR", txt(&l))]);
        assert_eq!(c.runs_dir(), l.join("runs"));
    }

    #[test]
    fn stdin_inherit_is_an_explicit_opt_in() {
        assert!(!cfg(&[]).0.stdin_inherit);
        assert!(!cfg(&[("THROTTLE_STDIN", "1")]).0.stdin_inherit);
        assert!(cfg(&[("THROTTLE_STDIN", "inherit")]).0.stdin_inherit);
    }

    #[test]
    fn legacy_and_new_names() {
        let (a, b, cc) = (abs(["a"]), abs(["b"]), abs(["c"]));
        let (c, w) = cfg(&[
            ("PLOCK_DIR", txt(&a)),
            ("PLOCK_POLL", "0.2"),
            ("PLOCK_WAIT_LOG", "1"),
            ("PLOCK_UNREADABLE_LIMIT", "3"),
        ]);
        assert!(w.is_empty());
        assert_eq!(
            (c.root, c.poll, c.unreadable_limit),
            (a.clone(), Duration::from_millis(200), 3)
        );
        let (c, _) = cfg(&[
            ("PLOCK_DIR", txt(&a)),
            ("PLOCK_ROOT", txt(&b)),
            ("THROTTLE_DIR", txt(&cc)),
        ]);
        assert_eq!(c.root, cc);
        let (c, _) = cfg(&[("PLOCK_ROOT", txt(&b))]);
        assert_eq!(c.root, b);
    }

    #[test]
    fn the_old_heavy_names_are_accepted_and_the_new_ones_win() {
        let (old, new) = (abs(["old"]), abs(["new"]));
        let (c, _) = cfg(&[
            ("HEAVY_DIR", txt(&old)),
            ("HEAVY_SLOTS", "3"),
            ("HEAVY_STALL", "70"),
        ]);
        assert_eq!(
            (c.root, c.slots, c.stall),
            (old.clone(), 3, Some(Duration::from_secs(70)))
        );
        let (c, _) = cfg(&[("HEAVY_DIR", txt(&old)), ("THROTTLE_DIR", txt(&new))]);
        assert_eq!(c.root, new);
        assert_eq!(
            aliased(
                &|k| (k == "HEAVY_HELD").then(|| "a:1".to_string()),
                "THROTTLE_HELD"
            ),
            Some("a:1".to_string())
        );
    }

    #[test]
    fn heavy_sh_slot_folder_is_ignored_with_a_warning() {
        let a = abs(["a"]);
        let (c, w) = cfg(&[("PLOCK_DIR", txt(&a)), ("HEAVY_LOCK_DIR", "/slots")]);
        assert_eq!(c.root, a);
        assert!(w.iter().any(|m| m.contains("HEAVY_LOCK_DIR")), "{w:?}");
    }

    #[test]
    fn bad_values_fall_back_with_a_warning() {
        let (c, w) = cfg(&[
            ("THROTTLE_POLL", "abc"),
            ("THROTTLE_SLOTS", "0"),
            ("THROTTLE_GATE_LOAD", "-1"),
            ("THROTTLE_CPUS", "4"),
        ]);
        assert_eq!(
            (c.poll, c.slots, c.gate_load),
            (Duration::from_secs(5), 1, 30.0)
        );
        assert_eq!(w.len(), 3, "{w:?}");
    }

    #[test]
    fn the_priority_bound_has_a_default_and_a_setting() {
        let (c, w) = cfg(&[]);
        assert_eq!(c.priority_max, Duration::from_secs(1800));
        assert_eq!(c.ci_priority_max, Duration::from_secs(7200));
        assert!(w.is_empty());
        let (c, _) = cfg(&[("THROTTLE_PRIORITY_MAX", "90")]);
        assert_eq!(c.priority_max, Duration::from_secs(90));
        let (c, w) = cfg(&[("THROTTLE_PRIORITY_MAX", "x")]);
        assert_eq!((c.priority_max, w.len()), (Duration::from_secs(1800), 1));
    }

    #[test]
    fn defaults() {
        let get = |k: &str| match k {
            "THROTTLE_CPUS" => Some("14".to_string()),
            "HOME" => Some("/Users/u".to_string()),
            _ => None,
        };
        let host = Host {
            os: paddock_env::Os::MacOs,
            ..Host::none()
        };
        let (c, _) = Config::from_env(&get, &host);
        // conformance: abs-path — the expected macOS default of a modelled macOS host (Os::MacOs, HOME=/Users/u), compared as path text
        let expected = PathBuf::from("/Users/u/Library/Application Support/throttle");
        assert_eq!(c.root, expected);
        assert_eq!(
            (c.gate_load, c.slots, c.wait_log),
            (30.0, 2, Duration::from_secs(60))
        );
        assert!(
            c.log_path("2026-10-31")
                .ends_with("throttle-logs/throttle.2026-10-31.log")
        );
    }
}
