//! The service side of an update: a supervised process restarts itself when its own executable changed on disk (the
//! app was updated or reinstalled), but only when nothing is in flight. It exits cleanly and launchd (`KeepAlive`),
//! systemd (`Restart=always`) or the Windows service wrapper starts the new program. A process started by hand is
//! left alone (nobody would start it again). From Pitcrew's `self_update.rs` and Pitwall's `service/watch.rs`.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, UNIX_EPOCH};

/// How often the executable is looked at.
pub const CHECK_EVERY: Duration = Duration::from_secs(60);

/// A cheap fingerprint of an executable file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BinaryStamp {
    /// Size in bytes.
    pub len: u64,
    /// Modification time, milliseconds since the Unix epoch.
    pub modified_ms: u128,
}

/// The fingerprint of `path` (`None` while it cannot be read, e.g. while an installer replaces it).
pub fn stamp(path: &Path) -> Option<BinaryStamp> {
    let m = std::fs::metadata(path).ok()?;
    let modified_ms = m
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis();
    Some(BinaryStamp {
        len: m.len(),
        modified_ms,
    })
}

/// What to do about the executable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartDecision {
    /// Unchanged, unreadable right now, or not supervised: keep running.
    Stay,
    /// Changed, but work is in flight: look again later.
    WaitForIdle,
    /// Changed and idle: exit so the supervisor starts the new program.
    Restart,
}

/// Decides from the stamp at start, the stamp now and whether work is in flight. `busy` is only asked when the file
/// changed (the question may cost a request).
pub fn decide(
    started: Option<BinaryStamp>,
    now: Option<BinaryStamp>,
    supervised: bool,
    busy: impl FnOnce() -> bool,
) -> RestartDecision {
    match (started, now) {
        (Some(a), Some(b)) if supervised && a != b => {
            if busy() {
                RestartDecision::WaitForIdle
            } else {
                RestartDecision::Restart
            }
        }
        _ => RestartDecision::Stay,
    }
}

/// Whether this process is supervised: `var` (the spec's `supervised_env`) is set and not empty.
pub fn is_supervised(var: &str) -> bool {
    std::env::var_os(var).is_some_and(|v| !v.is_empty())
}

/// Stops a running [`ExeWatch`] (dropping it stops it too, without waiting).
#[derive(Debug)]
pub struct WatchStop(Sender<()>, std::thread::JoinHandle<()>);

impl WatchStop {
    /// Stops the watch and waits for its thread (it wakes at once).
    pub fn stop(self) {
        let _ = self.0.send(());
        let _ = self.1.join();
    }
}

/// The waits between looks; the real one sleeps until the period ends or the watch is stopped.
pub trait Ticker: Send {
    /// Waits for `period`; `false` when the watch was stopped.
    fn wait(&mut self, period: Duration) -> bool;
}

/// Sleeps on a channel, so a stop ends the wait at once.
pub struct ChannelTicker(Receiver<()>);

impl Ticker for ChannelTicker {
    fn wait(&mut self, period: Duration) -> bool {
        matches!(self.0.recv_timeout(period), Err(RecvTimeoutError::Timeout))
    }
}

/// Watches an executable.
pub struct ExeWatch {
    exe: PathBuf,
    started: Option<BinaryStamp>,
    supervised: bool,
    every: Duration,
}

impl ExeWatch {
    /// Watches `exe` (stamped now); `supervised` from [`is_supervised`].
    pub fn new(exe: PathBuf, supervised: bool) -> Self {
        let started = stamp(&exe);
        Self {
            exe,
            started,
            supervised,
            every: CHECK_EVERY,
        }
    }

    /// Another period.
    #[must_use]
    pub fn every(mut self, every: Duration) -> Self {
        self.every = every;
        self
    }

    /// One look.
    pub fn check(&self, busy: impl FnOnce() -> bool) -> RestartDecision {
        decide(self.started, stamp(&self.exe), self.supervised, busy)
    }

    /// Looks every period until the program changed while idle, then calls `on_restart` (which exits the process, after
    /// removing its discovery file) and returns. Returns at once when not supervised or the file was unreadable at
    /// start; returns when the ticker says stop.
    pub fn run(
        self,
        ticker: &mut dyn Ticker,
        busy: &mut dyn FnMut() -> bool,
        on_restart: impl FnOnce(),
    ) {
        if !self.supervised || self.started.is_none() {
            return;
        }
        while ticker.wait(self.every) {
            if self.check(&mut *busy) == RestartDecision::Restart {
                on_restart();
                return;
            }
        }
    }

    /// Runs the watch on its own thread; `busy` and `on_restart` run there.
    pub fn spawn(
        self,
        mut busy: impl FnMut() -> bool + Send + 'static,
        on_restart: impl FnOnce() + Send + 'static,
    ) -> std::io::Result<WatchStop> {
        let (tx, rx) = channel();
        let thread = std::thread::Builder::new()
            .name("paddock-exe-watch".into())
            .spawn(move || self.run(&mut ChannelTicker(rx), &mut busy, on_restart))?;
        Ok(WatchStop(tx, thread))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn s(len: u64, ms: u128) -> Option<BinaryStamp> {
        Some(BinaryStamp {
            len,
            modified_ms: ms,
        })
    }

    #[test]
    fn a_changed_program_restarts_only_an_idle_supervised_process() {
        assert_eq!(
            decide(s(1, 1), s(2, 1), true, || false),
            RestartDecision::Restart
        );
        assert_eq!(
            decide(s(1, 1), s(1, 9), true, || false),
            RestartDecision::Restart
        );
        assert_eq!(
            decide(s(1, 1), s(2, 1), true, || true),
            RestartDecision::WaitForIdle
        );
        assert_eq!(
            decide(s(1, 1), s(2, 1), false, || false),
            RestartDecision::Stay
        );
        assert_eq!(
            decide(s(1, 1), s(1, 1), true, || panic!("not asked")),
            RestartDecision::Stay
        );
        assert_eq!(
            decide(s(1, 1), None, true, || false),
            RestartDecision::Stay,
            "being replaced"
        );
        assert_eq!(decide(None, s(1, 1), true, || false), RestartDecision::Stay);
    }

    /// Ticks `n` times without waiting, running `between` before each tick.
    struct Ticks<F: FnMut(u32) + Send>(u32, u32, F);
    impl<F: FnMut(u32) + Send> Ticker for Ticks<F> {
        fn wait(&mut self, _: Duration) -> bool {
            (self.2)(self.1);
            self.1 += 1;
            self.1 <= self.0
        }
    }

    #[test]
    fn the_watch_waits_while_busy_then_restarts_once() {
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("agent");
        std::fs::write(&exe, b"v1").unwrap();
        let w = ExeWatch::new(exe.clone(), true).every(Duration::from_secs(1));
        assert_eq!(w.check(|| false), RestartDecision::Stay);
        let mut asked = 0;
        let mut restarted = 0;
        let path = exe.clone();
        let mut ticker = Ticks(10, 0, move |tick| {
            if tick == 2 {
                std::fs::write(&path, b"version 2").unwrap();
            }
        });
        w.run(
            &mut ticker,
            &mut || {
                asked += 1;
                asked < 3
            },
            || restarted += 1,
        );
        assert_eq!(
            (asked, restarted),
            (3, 1),
            "asked only after the change, restarted once when idle"
        );
        assert!(ticker.1 < 10, "stopped looking after the restart");
    }

    #[test]
    fn an_unsupervised_or_unreadable_program_is_never_watched_and_a_stop_ends_the_watch() {
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("agent");
        std::fs::write(&exe, b"v1").unwrap();
        let mut ticker = Ticks(3, 0, |_| {});
        ExeWatch::new(exe.clone(), false).run(&mut ticker, &mut || false, || panic!("no restart"));
        assert_eq!(ticker.1, 0, "returns at once");
        ExeWatch::new(d.path().join("missing"), true)
            .run(&mut ticker, &mut || false, || panic!("no restart"));
        assert_eq!(ticker.1, 0);
        ExeWatch::new(exe.clone(), true).run(&mut ticker, &mut || false, || panic!("unchanged"));
        assert_eq!(ticker.1, 4, "ran until the ticker stopped");
        // The real thread: stopped before its first look, joined, no restart.
        let restarted = Arc::new(AtomicBool::new(false));
        let r = restarted.clone();
        let stop = ExeWatch::new(exe, true)
            .spawn(|| false, move || r.store(true, Ordering::SeqCst))
            .unwrap();
        stop.stop();
        assert!(!restarted.load(Ordering::SeqCst));
        let mut t = ChannelTicker(channel::<()>().1);
        assert!(
            !t.wait(Duration::from_secs(30)),
            "a dropped stop handle ends the wait at once"
        );
    }

    #[test]
    fn supervision_is_read_from_the_variable() {
        assert!(!is_supervised("PADDOCK_SERVICE_TEST_SURELY_UNSET_VARIABLE"));
    }
}
