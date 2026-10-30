//! The start guard, pure: the app's start before its event loop runs (plugins' set-up, tray, main window) has no window
//! and no page yet, so the page [`Watchdog`](crate::Watchdog) cannot see it. A start that is stuck there (a main thread
//! deadlocked inside a plugin hook, ADR 0029) used to leave nothing in the log. The guard names the phase the start is in
//! and how long it has waited, so the adapter's thread can log it; it never ends or restarts anything (ER-19: the start
//! may be slow, only the user decides to give up). The caller passes the time, so every decision is a unit test.

use std::time::{Duration, Instant};

/// Where the start is. The phases follow each other; [`StartGuard::ready`] ends the guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartPhase {
    /// Tauri runs every plugin's set-up (the shell's, with the app's own set-up inside it).
    PluginSetUp,
    /// The shell opens the tray.
    Tray,
    /// The shell opens the main window.
    MainWindow,
    /// The set-up is done; the event loop has not reported ready yet.
    EventLoop,
}

impl StartPhase {
    /// The phase in words, for the log.
    pub fn describe(self) -> &'static str {
        match self {
            StartPhase::PluginSetUp => "the plugins' set-up",
            StartPhase::Tray => "opening the tray",
            StartPhase::MainWindow => "opening the main window",
            StartPhase::EventLoop => "waiting for the event loop",
        }
    }
}

/// When the guard reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartTiming {
    /// How long the start may take before the first report.
    pub first_report: Duration,
    /// How often a start still stuck is reported again.
    pub repeat: Duration,
    /// How often the adapter's thread asks [`StartGuard::due`].
    pub tick: Duration,
    /// A gap between two ticks longer than this means the process itself was paused (a CPU limiter such as App Tamer,
    /// a suspend): the clock restarts instead of reporting a stuck start.
    pub pause_gap: Duration,
}

impl Default for StartTiming {
    fn default() -> Self {
        StartTiming {
            first_report: Duration::from_secs(20),
            repeat: Duration::from_secs(60),
            tick: Duration::from_secs(1),
            pause_gap: Duration::from_secs(5),
        }
    }
}

/// A report of a stuck start: the phase and how long the start has waited in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StuckStart {
    pub phase: StartPhase,
    pub waited: Duration,
    /// Whether this is the first report (later ones repeat it).
    pub first: bool,
}

/// The start's phase and its clock.
#[derive(Debug, Clone)]
pub struct StartGuard {
    timing: StartTiming,
    phase: StartPhase,
    /// When the start (or, after a pause, its clock) began.
    since: Instant,
    last_tick: Instant,
    next_report: Instant,
    reported: bool,
    done: bool,
}

impl StartGuard {
    /// A start that began at `now`, in [`StartPhase::PluginSetUp`].
    pub fn new(timing: StartTiming, now: Instant) -> StartGuard {
        StartGuard {
            timing,
            phase: StartPhase::PluginSetUp,
            since: now,
            last_tick: now,
            next_report: now + timing.first_report,
            reported: false,
            done: false,
        }
    }

    /// The timing it was made with.
    pub fn timing(&self) -> StartTiming {
        self.timing
    }

    /// The phase the start is in.
    pub fn phase(&self) -> StartPhase {
        self.phase
    }

    /// The start moved on to `phase` (the clock keeps running: a slow start is slow as a whole).
    pub fn enter(&mut self, phase: StartPhase) {
        self.phase = phase;
    }

    /// The event loop is running: the guard is done. Returns how long the start took when it had been reported stuck,
    /// so the log can say it went on after all.
    pub fn ready(&mut self, now: Instant) -> Option<Duration> {
        if self.done {
            return None;
        }
        self.done = true;
        self.reported
            .then(|| now.saturating_duration_since(self.since))
    }

    /// Whether the guard is done (the thread may end).
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// A report when the start is overdue (then again every [`StartTiming::repeat`]); `None` otherwise, after
    /// [`ready`](Self::ready), and on the tick after a pause (the clock restarts).
    pub fn due(&mut self, now: Instant) -> Option<StuckStart> {
        let paused = now.saturating_duration_since(self.last_tick) > self.timing.pause_gap;
        self.last_tick = now;
        if self.done {
            return None;
        }
        if paused {
            self.since = now;
            self.next_report = now + self.timing.first_report;
            self.reported = false;
            return None;
        }
        if now < self.next_report {
            return None;
        }
        let first = !self.reported;
        self.reported = true;
        self.next_report = now + self.timing.repeat;
        Some(StuckStart {
            phase: self.phase,
            waited: now.saturating_duration_since(self.since),
            first,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timing() -> StartTiming {
        StartTiming {
            first_report: Duration::from_secs(20),
            repeat: Duration::from_secs(60),
            tick: Duration::from_secs(1),
            pause_gap: Duration::from_secs(5),
        }
    }

    /// Ticks every second from `from` to `to` (inclusive), collecting the reports.
    fn tick(g: &mut StartGuard, t0: Instant, from: u64, to: u64) -> Vec<(u64, StuckStart)> {
        (from..=to)
            .filter_map(|s| g.due(t0 + Duration::from_secs(s)).map(|r| (s, r)))
            .collect()
    }

    #[test]
    fn a_start_on_time_is_never_reported() {
        let t0 = Instant::now();
        let mut g = StartGuard::new(timing(), t0);
        assert!(tick(&mut g, t0, 1, 19).is_empty());
        g.enter(StartPhase::EventLoop);
        assert_eq!(g.ready(t0 + Duration::from_secs(19)), None);
        assert!(g.is_done());
        assert!(tick(&mut g, t0, 20, 200).is_empty());
    }

    #[test]
    fn a_stuck_start_is_reported_with_its_phase_then_repeated() {
        let t0 = Instant::now();
        let mut g = StartGuard::new(timing(), t0);
        g.enter(StartPhase::MainWindow);
        assert_eq!(g.phase(), StartPhase::MainWindow);
        let reports = tick(&mut g, t0, 1, 141);
        assert_eq!(
            reports,
            vec![
                (
                    20,
                    StuckStart {
                        phase: StartPhase::MainWindow,
                        waited: Duration::from_secs(20),
                        first: true
                    }
                ),
                (
                    80,
                    StuckStart {
                        phase: StartPhase::MainWindow,
                        waited: Duration::from_secs(80),
                        first: false
                    }
                ),
                (
                    140,
                    StuckStart {
                        phase: StartPhase::MainWindow,
                        waited: Duration::from_secs(140),
                        first: false
                    }
                ),
            ]
        );
    }

    #[test]
    fn a_start_that_goes_on_after_a_report_says_how_long_it_took() {
        let t0 = Instant::now();
        let mut g = StartGuard::new(timing(), t0);
        assert_eq!(tick(&mut g, t0, 1, 25).len(), 1);
        assert_eq!(
            g.ready(t0 + Duration::from_secs(25)),
            Some(Duration::from_secs(25))
        );
        assert_eq!(g.ready(t0 + Duration::from_secs(26)), None, "ready once");
    }

    #[test]
    fn a_paused_process_restarts_the_clock_instead_of_reporting() {
        let t0 = Instant::now();
        let mut g = StartGuard::new(timing(), t0);
        assert!(tick(&mut g, t0, 1, 10).is_empty());
        // Stopped by a CPU limiter for a minute: the first tick after it restarts the clock.
        assert_eq!(g.due(t0 + Duration::from_secs(70)), None);
        assert!(tick(&mut g, t0, 71, 89).is_empty());
        let r = g
            .due(t0 + Duration::from_secs(90))
            .expect("reported 20 s after the pause");
        assert_eq!((r.waited, r.first), (Duration::from_secs(20), true));
    }

    #[test]
    fn the_phases_have_words_and_the_default_timing_waits_twenty_seconds() {
        for (p, w) in [
            (StartPhase::PluginSetUp, "the plugins' set-up"),
            (StartPhase::Tray, "opening the tray"),
            (StartPhase::MainWindow, "opening the main window"),
            (StartPhase::EventLoop, "waiting for the event loop"),
        ] {
            assert_eq!(p.describe(), w);
        }
        let d = StartTiming::default();
        assert_eq!(d.first_report, Duration::from_secs(20));
        assert!(d.tick < d.pause_gap && d.pause_gap < d.first_report && d.first_report < d.repeat);
        let t0 = Instant::now();
        assert_eq!(StartGuard::new(d, t0).timing(), d);
    }
}
