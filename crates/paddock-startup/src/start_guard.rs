//! The start guard, pure: the part of an app's start before its first page (the shell plugin's set-up, the native event
//! loop starting, the tray and the main window) has no page yet, so the page [`Watchdog`](crate::Watchdog) cannot see it.
//! A start stuck there (a main thread deadlocked inside a plugin hook, ADR 0029) used to leave nothing in the log. The
//! guard names the phase the start is in so the adapter's thread can log it; it never ends or restarts anything (ER-19:
//! the start may be slow, only the user decides to give up).
//!
//! It counts ticks, not elapsed time (ADR 0031): every wake-up of the adapter's thread is one tick however late it comes,
//! so a paused process counts one tick when it resumes and a starved one simply ticks less. The caller passes the time
//! only for the closing line's duration, so every decision is a unit test.

use std::time::{Duration, Instant};

/// Where the start is. The phases follow each other; [`StartGuard::ready`] ends the guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartPhase {
    /// Tauri runs the shell plugin's set-up (with the app's own set-up inside it).
    PluginSetUp,
    /// The plugins are set up; the native event loop is starting (Tauri runs the app's `setup` once it has).
    StartingEventLoop,
    /// The shell opens the tray.
    Tray,
    /// The shell opens the main window.
    MainWindow,
}

impl StartPhase {
    /// The phase in words, for the log.
    pub fn describe(self) -> &'static str {
        match self {
            StartPhase::PluginSetUp => "the shell plugin's set-up",
            StartPhase::StartingEventLoop => "starting the event loop",
            StartPhase::Tray => "opening the tray",
            StartPhase::MainWindow => "opening the main window",
        }
    }
}

/// When the guard reports, in ticks of its thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartTiming {
    /// The ticks a start may take before the first report.
    pub first_report: u32,
    /// The ticks between later reports while the start stays stuck.
    pub repeat: u32,
    /// How long the adapter's thread sleeps between ticks.
    pub tick: Duration,
}

impl Default for StartTiming {
    /// The first report after 20 ticks of 1 s, then every 60.
    fn default() -> Self {
        StartTiming {
            first_report: 20,
            repeat: 60,
            tick: Duration::from_secs(1),
        }
    }
}

/// A report of a stuck start: the phase and the ticks the start has waited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StuckStart {
    pub phase: StartPhase,
    pub ticks: u64,
    /// Whether this is the first report (later ones repeat it).
    pub first: bool,
}

/// The start's phase and its tick count.
#[derive(Debug, Clone)]
pub struct StartGuard {
    timing: StartTiming,
    phase: StartPhase,
    began: Instant,
    ticks: u64,
    next_report: u64,
    reported: bool,
    done: bool,
}

impl StartGuard {
    /// A start that began at `now`, in [`StartPhase::PluginSetUp`].
    pub fn new(timing: StartTiming, now: Instant) -> StartGuard {
        StartGuard {
            timing,
            phase: StartPhase::PluginSetUp,
            began: now,
            ticks: 0,
            next_report: u64::from(timing.first_report.max(1)),
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

    /// The start moved on to `phase` (the count keeps running: a slow start is slow as a whole).
    pub fn enter(&mut self, phase: StartPhase) {
        self.phase = phase;
    }

    /// The first page can load: the guard is done. Returns how long the start took when it had been reported stuck,
    /// so the log can say it went on after all (once).
    pub fn ready(&mut self, now: Instant) -> Option<Duration> {
        if self.done {
            return None;
        }
        self.done = true;
        self.reported
            .then(|| now.saturating_duration_since(self.began))
    }

    /// Whether the guard is done (the thread may end).
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// One tick: a report when the start is overdue, then again every [`StartTiming::repeat`] ticks; `None` otherwise
    /// and after [`ready`](Self::ready).
    pub fn tick(&mut self) -> Option<StuckStart> {
        if self.done {
            return None;
        }
        self.ticks += 1;
        if self.ticks < self.next_report {
            return None;
        }
        let first = !self.reported;
        self.reported = true;
        self.next_report = self.ticks + u64::from(self.timing.repeat.max(1));
        Some(StuckStart {
            phase: self.phase,
            ticks: self.ticks,
            first,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticks(g: &mut StartGuard, n: u64) -> Vec<StuckStart> {
        (0..n).filter_map(|_| g.tick()).collect()
    }

    #[test]
    fn a_start_on_time_is_never_reported() {
        let t0 = Instant::now();
        let mut g = StartGuard::new(StartTiming::default(), t0);
        assert!(ticks(&mut g, 19).is_empty());
        assert_eq!(g.ready(t0 + Duration::from_secs(19)), None);
        assert!(g.is_done());
        assert!(ticks(&mut g, 500).is_empty());
    }

    #[test]
    fn a_stuck_start_is_reported_with_its_phase_then_repeated() {
        let mut g = StartGuard::new(StartTiming::default(), Instant::now());
        g.enter(StartPhase::MainWindow);
        assert_eq!(g.phase(), StartPhase::MainWindow);
        let at = |ticks, first| StuckStart {
            phase: StartPhase::MainWindow,
            ticks,
            first,
        };
        assert_eq!(
            ticks(&mut g, 140),
            vec![at(20, true), at(80, false), at(140, false)]
        );
    }

    /// However late a wake-up comes (a paused or starved process), it is one tick; the closing line follows a report.
    #[test]
    fn only_ticks_count_not_the_time_between_them() {
        let t0 = Instant::now();
        let mut g = StartGuard::new(StartTiming::default(), t0);
        assert!(ticks(&mut g, 19).is_empty());
        let r = g.tick().expect("the 20th tick reports");
        assert_eq!((r.ticks, r.first), (20, true));
        assert_eq!(
            g.ready(t0 + Duration::from_secs(3600)),
            Some(Duration::from_secs(3600))
        );
        assert_eq!(g.ready(t0 + Duration::from_secs(3601)), None, "ready once");
    }

    #[test]
    fn zero_counts_mean_every_tick() {
        let timing = StartTiming {
            first_report: 0,
            repeat: 0,
            tick: Duration::from_millis(1),
        };
        let mut g = StartGuard::new(timing, Instant::now());
        assert_eq!(ticks(&mut g, 3).len(), 3);
        assert_eq!(g.timing(), timing);
    }

    #[test]
    fn the_phases_have_words_and_the_default_waits_twenty_ticks_of_a_second() {
        for (p, w) in [
            (StartPhase::PluginSetUp, "the shell plugin's set-up"),
            (StartPhase::StartingEventLoop, "starting the event loop"),
            (StartPhase::Tray, "opening the tray"),
            (StartPhase::MainWindow, "opening the main window"),
        ] {
            assert_eq!(p.describe(), w);
        }
        let d = StartTiming::default();
        assert_eq!(
            (d.first_report, d.repeat, d.tick),
            (20, 60, Duration::from_secs(1))
        );
    }
}
