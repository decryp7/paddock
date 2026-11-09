//! The agent's own log for a test that fails (ER-27). An agent started in the test process (`Running::start*`) installs no
//! file log, so its lines ("refused a connection: ...", a job's phases) went nowhere and a failure showed only the client's
//! side (CI run 7763). [`on_failure`] keeps every `log` record of this process in memory (the last [`KEPT`]) and its guard
//! prints them to standard error when it is dropped while the test panics; nextest shows that output with the failure.
//!
//! One logger per process: nextest runs each test in its own process; under `cargo test` the tests of one file share it, and
//! the printed lines can include a neighbour's.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

/// The most lines kept (the oldest go first).
pub const KEPT: usize = 2000;

struct Keep(Mutex<VecDeque<String>>);

impl log::Log for Keep {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }

    fn log(&self, r: &log::Record) {
        struct Pairs(String);
        impl<'kvs> log::kv::VisitSource<'kvs> for Pairs {
            fn visit_pair(&mut self, k: log::kv::Key<'kvs>, v: log::kv::Value<'kvs>) -> Result<(), log::kv::Error> {
                self.0.push_str(&format!(" {k}={v}"));
                Ok(())
            }
        }
        let mut pairs = Pairs(String::new());
        let _ = r.key_values().visit(&mut pairs);
        let thread = std::thread::current();
        let line = format!(
            "{:?} {:<5} [{}] {}{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default(),
            r.level(),
            thread.name().unwrap_or("?"),
            r.args(),
            pairs.0
        );
        let mut lines = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if lines.len() == KEPT {
            lines.pop_front();
        }
        lines.push_back(line);
    }

    fn flush(&self) {}
}

fn keep() -> Option<&'static Keep> {
    static K: OnceLock<Option<&'static Keep>> = OnceLock::new();
    *K.get_or_init(|| {
        let k: &'static Keep = Box::leak(Box::new(Keep(Mutex::new(VecDeque::new()))));
        // Another logger already installed (a test file with its own capture) keeps its place; this guard then prints nothing.
        log::set_logger(k).ok()?;
        log::set_max_level(log::LevelFilter::Trace);
        Some(k)
    })
}

/// Prints the kept log lines when dropped during a panic.
#[must_use = "the lines are printed when the guard is dropped"]
pub struct OnFailure;

impl Drop for OnFailure {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            return;
        }
        let Some(k) = keep() else { return };
        let lines = k.0.lock().unwrap_or_else(|p| p.into_inner());
        eprintln!(
            "---- the agent's log ({} lines, the last {KEPT} at most; {}) ----",
            lines.len(),
            descriptors()
        );
        for l in lines.iter() {
            eprintln!("{l}");
        }
        eprintln!("---- end of the agent's log ----");
    }
}

/// How many files this process has open and may open: running out of them ends connections (CI run 7763).
fn descriptors() -> String {
    #[cfg(unix)]
    {
        let dir = if cfg!(target_os = "linux") { "/proc/self/fd" } else { "/dev/fd" };
        let open = std::fs::read_dir(dir).map_or_else(|e| e.to_string(), |d| d.count().to_string());
        let mut limit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        // SAFETY: a valid pointer to a local struct.
        #[allow(unsafe_code)]
        let read = unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } == 0;
        let limit = if read {
            format!("{} (hard {})", limit.rlim_cur, limit.rlim_max)
        } else {
            "unknown".into()
        };
        format!("{open} files open, limit {limit}")
    }
    #[cfg(not(unix))]
    {
        "open files not counted here".to_string()
    }
}

/// Starts keeping this process's log lines; the guard prints them if the test fails. Take it before the agent starts.
pub fn on_failure() -> OnFailure {
    let _ = keep();
    OnFailure
}

/// The kept lines (a test that asserts on what the agent logged).
#[allow(dead_code)]
pub fn lines() -> Vec<String> {
    keep()
        .map(|k| k.0.lock().unwrap_or_else(|p| p.into_inner()).iter().cloned().collect())
        .unwrap_or_default()
}
