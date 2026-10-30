//! Judging a process that is starting (ADR 0031): patience, not a wall clock. The judge is pure: it is handed one
//! [`Reading`] per tick (liveness, the ready handshake, progress ticks, the tree's CPU time) and answers [`StartState`].
//! A starved or paused process is waited for; only a process that spins without becoming ready, does nothing at all, or
//! cannot be observed for a long time is called stuck. Elapsed time alone never fails a start.
//!
//! Idle and blindness are counted in ticks (observations), never in wall seconds: every observation adds one
//! [`Patience::tick`], however late it came, so a host that was paused or starved counts one tick, not the pause
//! (ADR 0031 section 2). A hung process with timer threads can keep using a little CPU and so keep resetting the idle
//! count: the judge cannot call that stuck, and the caller's own cap (its recorded product limit) is the real bound. When
//! the OS gives no CPU reading at all, [`Patience::hang_guard`] bounds the wait, so there is always a bound.
//!
//! [`StartWatch`] is the thin edge: it asks a [`StartProbe`] for the readings, feeds the judge and paces itself through a
//! [`Pace`] (a clock in production, a fake in tests). It reads the CPU time every [`Patience::cpu_every`] ticks, one pid
//! at a time ([`crate::tree_cpu_time`]), never the whole process table.

use crate::{ProcessId, StallCause, StallReport, tree_cpu_time};
use std::time::Duration;

/// When a start is stuck rather than slow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Patience {
    /// Stuck when the tree used this much CPU time without becoming ready (spinning; load does not inflate it). CPU time is
    /// summed over cores and processes, so a multi-threaded tree reaches it faster than the wall clock.
    pub spin: Duration,
    /// Stuck when the tree used no CPU and reported no progress for this long (counted in ticks) while alive.
    pub idle: Duration,
    /// Only when nothing can be observed (the OS gives no CPU reading, or the caller never reads one): stuck after this
    /// long (counted in ticks). The one time-based bound of the judge, for a process that cannot be judged any other way.
    pub hang_guard: Duration,
    /// After this many ticks alive and not ready, a [`StallCause::Slow`] note is reported (the wait goes on). 0: never.
    pub note_after: u32,
    /// The length of one tick (how often the probe is looked at).
    pub tick: Duration,
    /// The CPU time is read every this many ticks (reading it costs a system call per process of the tree).
    pub cpu_every: u32,
}

impl Default for Patience {
    fn default() -> Patience {
        Patience {
            spin: Duration::from_secs(120),
            idle: Duration::from_secs(60),
            hang_guard: Duration::from_secs(300),
            note_after: 0,
            tick: Duration::from_millis(50),
            cpu_every: 20,
        }
    }
}

/// How the process ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitInfo {
    /// The exit code (`None`: ended by a signal, or not known).
    pub status: Option<i32>,
    /// The end of its error output, when it left any.
    pub stderr_tail: Option<String>,
}

/// Whether the process is there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Liveness {
    /// Running (or starved).
    Running,
    /// Stopped by the OS or the user: no tick counts against it.
    Paused,
    /// Gone.
    Exited(ExitInfo),
}

/// One CPU reading of the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cpu {
    /// Not read this tick.
    NotRead,
    /// The OS did not say (or the process is gone).
    Unavailable,
    /// The tree's total CPU time so far.
    Used(Duration),
}

/// What one tick saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    /// The process.
    pub liveness: Liveness,
    /// The ready handshake has happened (a port opened, a file naming the process, an answer).
    pub ready: bool,
    /// An app-defined counter that grows whenever the start makes progress (a phase done, a step reported).
    pub progress: u64,
    /// The tree's CPU time, when read.
    pub cpu: Cpu,
}

/// Where the start stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartState {
    /// Alive and working or waiting: keep waiting.
    Starting,
    /// The handshake happened.
    Ready,
    /// It looks stuck, or is worth a note ([`StallCause::is_stuck`] tells which). Each kind is reported once when it
    /// begins (the note once ever, a stuck cause once per episode, until the process runs or reports progress again); the
    /// judge goes on judging and answers `Starting` while the same stall continues. The caller decides whether to end the
    /// process.
    Stalled(StallReport),
    /// The process is gone.
    Exited(ExitInfo),
}

/// The pure judge. Feed it one [`Reading`] per tick.
#[derive(Debug)]
pub struct StartJudge {
    patience: Patience,
    what: String,
    ticks: u32,
    since_cpu_read: u32,
    first_cpu: Option<Duration>,
    last_cpu: Option<Duration>,
    last_progress: u64,
    idle: Duration,
    blind: Duration,
    /// CPU used since the start or the last progress.
    spun: Duration,
    /// The stall kinds already reported in the current episode (bits of `Kind`): each is reported once.
    reported: u8,
}

impl StartJudge {
    /// A judge with nothing seen; `what` names the thing started in reports ("the browser").
    pub fn new(patience: Patience, what: &str) -> StartJudge {
        StartJudge {
            patience,
            what: what.to_string(),
            ticks: 0,
            since_cpu_read: 0,
            first_cpu: None,
            last_cpu: None,
            last_progress: 0,
            idle: Duration::ZERO,
            blind: Duration::ZERO,
            spun: Duration::ZERO,
            reported: 0,
        }
    }

    /// Takes one tick's reading.
    pub fn observe(&mut self, r: &Reading) -> StartState {
        if r.ready {
            return StartState::Ready;
        }
        match &r.liveness {
            Liveness::Exited(info) => return StartState::Exited(info.clone()),
            Liveness::Paused => return StartState::Starting,
            Liveness::Running => {}
        }
        self.ticks = self.ticks.saturating_add(1);
        self.since_cpu_read = self.since_cpu_read.saturating_add(1);
        if r.progress > self.last_progress {
            self.last_progress = r.progress;
            self.idle = Duration::ZERO;
            self.blind = Duration::ZERO;
            self.first_cpu = self.last_cpu;
        }
        let window = self.patience.tick.saturating_mul(self.since_cpu_read);
        match r.cpu {
            Cpu::NotRead => {
                // A caller that never reads the CPU time is as blind as one the OS cannot answer: past twice the read
                // interval the surplus ticks count toward the hang guard, so there is always a bound.
                if self.since_cpu_read > self.patience.cpu_every.max(1).saturating_mul(2) {
                    self.blind += self.patience.tick;
                }
            }
            Cpu::Unavailable => {
                self.blind += window;
                self.since_cpu_read = 0;
            }
            Cpu::Used(c) => {
                self.blind = Duration::ZERO;
                self.since_cpu_read = 0;
                let first = *self.first_cpu.get_or_insert(c);
                if self.last_cpu.is_none_or(|l| c > l) {
                    self.idle = Duration::ZERO;
                } else {
                    self.idle += window;
                }
                self.last_cpu = Some(c);
                self.spun = c.saturating_sub(first);
            }
        }
        // The episodes: a kind is reported once when it begins, and again only after it has ended.
        if self.spun < self.patience.spin {
            self.reported &= !SPIN;
        }
        if self.idle < self.patience.idle {
            self.reported &= !IDLE;
        }
        if self.blind < self.patience.hang_guard {
            self.reported &= !HUNG;
        }
        let note = self.patience.note_after > 0 && self.ticks >= self.patience.note_after;
        let causes = [
            (
                SPIN,
                self.spun >= self.patience.spin,
                StallCause::Spinning(self.spun),
            ),
            (
                IDLE,
                self.idle >= self.patience.idle,
                StallCause::Idle(self.idle),
            ),
            (
                HUNG,
                self.blind >= self.patience.hang_guard,
                StallCause::Hung(self.blind),
            ),
            (SLOW, note, StallCause::Slow),
        ];
        // The first kind that holds and has not been reported in this episode; one that was is not repeated, and does not
        // hide another.
        for (kind, holds, cause) in causes {
            if holds && self.reported & kind == 0 {
                self.reported |= kind;
                return StartState::Stalled(StallReport::new(cause, &self.what));
            }
        }
        StartState::Starting
    }
}

const SPIN: u8 = 1;
const IDLE: u8 = 2;
const HUNG: u8 = 4;
const SLOW: u8 = 8;

/// What a [`StartWatch`] reads from the world, once per tick.
pub trait StartProbe {
    /// Whether the process is there.
    fn liveness(&mut self) -> Liveness;
    /// Whether the ready handshake has happened. May do work (try a connection); remember its result.
    fn ready(&mut self) -> bool;
    /// The start's progress counter (see [`Reading::progress`]); 0 when the app has none.
    fn progress(&mut self) -> u64 {
        0
    }
    /// The tree's CPU time, `None` when the OS did not say.
    fn cpu_time(&mut self) -> Option<Duration>;
}

/// Waits one tick.
pub trait Pace {
    /// Waits `tick`; `false` when the wait was cancelled.
    fn wait(&mut self, tick: Duration) -> bool;
}

/// The CPU time of the tree at `root` as a [`Cpu`] reading.
pub fn tree_cpu(root: ProcessId) -> Cpu {
    tree_cpu_time(root).map_or(Cpu::Unavailable, Cpu::Used)
}

/// The end of a file's content as text, at most `max` bytes (only that much is read: the file is seeked, not loaded), cut at a
/// character boundary and trimmed; `None` when the file
/// is missing or empty. For the error output of a process that exited.
pub fn tail_of(path: &std::path::Path, max: usize) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(max as u64)))
        .ok()?;
    let mut bytes = Vec::new();
    file.take(max as u64).read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let text = text.trim_matches(|c: char| c.is_whitespace() || c == '\u{fffd}');
    (!text.is_empty()).then(|| text.to_string())
}

/// What a watch ended with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Watched {
    /// The judge's answer: `Ready`, `Stalled` or `Exited` (never `Starting`).
    State(StartState),
    /// The [`Pace`] said the wait was cancelled.
    Cancelled,
}

/// Drives a [`StartJudge`] from a [`StartProbe`].
pub struct StartWatch<P, C> {
    judge: StartJudge,
    probe: P,
    pace: C,
    patience: Patience,
    ticks: u32,
}

impl<P: StartProbe, C: Pace> StartWatch<P, C> {
    /// A watch over `probe`.
    pub fn new(patience: Patience, what: &str, probe: P, pace: C) -> StartWatch<P, C> {
        StartWatch {
            judge: StartJudge::new(patience, what),
            probe,
            pace,
            patience,
            ticks: 0,
        }
    }

    /// Looks and waits tick by tick until the start is ready, exited, stalled or cancelled. A stall is returned once
    /// ([`StartState::Stalled`]) and does not end the watch: call again to keep waiting for the next event (the same stall
    /// is not repeated; a different one, or the process becoming ready or exiting, is).
    pub fn wait(&mut self) -> Watched {
        loop {
            let reading = self.read();
            match self.judge.observe(&reading) {
                StartState::Starting => {}
                state => return Watched::State(state),
            }
            if !self.pace.wait(self.patience.tick) {
                return Watched::Cancelled;
            }
        }
    }

    fn read(&mut self) -> Reading {
        let ready = self.probe.ready();
        let liveness = self.probe.liveness();
        let every = self.patience.cpu_every.max(1);
        let cpu = if self.ticks.is_multiple_of(every) {
            self.probe.cpu_time().map_or(Cpu::Unavailable, Cpu::Used)
        } else {
            Cpu::NotRead
        };
        self.ticks = self.ticks.wrapping_add(1);
        Reading {
            liveness,
            ready,
            progress: self.probe.progress(),
            cpu,
        }
    }

    /// The probe, for what the caller keeps there (a connection made at the handshake).
    pub fn into_probe(self) -> P {
        self.probe
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    const S: fn(u64) -> Duration = Duration::from_secs;

    fn patience() -> Patience {
        Patience {
            spin: S(10),
            idle: S(5),
            hang_guard: S(8),
            note_after: 0,
            tick: S(1),
            cpu_every: 1,
        }
    }
    fn run(cpu: Cpu) -> Reading {
        Reading {
            liveness: Liveness::Running,
            ready: false,
            progress: 0,
            cpu,
        }
    }
    fn used(s: u64) -> Cpu {
        Cpu::Used(S(s))
    }
    fn cause(s: &StartState) -> Option<StallCause> {
        match s {
            StartState::Stalled(r) => Some(r.cause),
            _ => None,
        }
    }

    /// The state a watch ended with (a cancel counts as still starting).
    fn state(w: Watched) -> StartState {
        match w {
            Watched::State(s) => s,
            Watched::Cancelled => StartState::Starting,
        }
    }

    #[test]
    fn the_default_patience_waits_minutes_not_seconds() {
        let p = Patience::default();
        assert!(p.spin >= S(60) && p.idle >= S(30) && p.hang_guard >= p.idle);
        assert!(p.cpu_every > 1 && p.tick < S(1) && p.note_after == 0);
    }

    #[test]
    fn ready_wins_and_exit_reports_status_and_output() {
        let mut j = StartJudge::new(patience(), "x");
        let mut r = run(used(1));
        r.ready = true;
        r.liveness = Liveness::Exited(ExitInfo {
            status: Some(1),
            stderr_tail: None,
        });
        assert_eq!(j.observe(&r), StartState::Ready);
        let info = ExitInfo {
            status: Some(3),
            stderr_tail: Some("boom".into()),
        };
        r.ready = false;
        r.liveness = Liveness::Exited(info.clone());
        assert_eq!(j.observe(&r), StartState::Exited(info));
    }

    #[test]
    fn a_working_start_is_waited_for_however_long_it_takes() {
        let mut j = StartJudge::new(patience(), "x");
        for i in 0..1000u64 {
            let state = j.observe(&Reading {
                progress: i + 1,
                ..run(used(i / 100))
            });
            assert_eq!(state, StartState::Starting, "tick {i}");
        }
    }

    #[test]
    fn spinning_without_progress_is_stuck() {
        let mut j = StartJudge::new(patience(), "x");
        let mut c = 0;
        let mut last = StartState::Starting;
        while last == StartState::Starting {
            c += 2;
            last = j.observe(&run(used(c)));
            assert!(c < 100);
        }
        assert!(matches!(cause(&last), Some(StallCause::Spinning(d)) if d >= S(10)));
    }

    #[test]
    fn no_cpu_and_no_progress_is_idle_but_any_cpu_starts_it_again() {
        let mut j = StartJudge::new(patience(), "x");
        assert_eq!(j.observe(&run(used(1))), StartState::Starting);
        for _ in 0..4 {
            assert_eq!(j.observe(&run(used(1))), StartState::Starting);
        }
        assert_eq!(
            j.observe(&run(used(2))),
            StartState::Starting,
            "it ran: idle starts again"
        );
        for _ in 0..4 {
            assert_eq!(j.observe(&run(used(2))), StartState::Starting);
        }
        assert!(matches!(cause(&j.observe(&run(used(2)))), Some(StallCause::Idle(d)) if d == S(5)));
    }

    #[test]
    fn progress_ticks_reset_idle_and_spin() {
        let mut j = StartJudge::new(patience(), "x");
        j.observe(&run(used(1)));
        for p in 1..=20 {
            assert_eq!(
                j.observe(&Reading {
                    progress: p,
                    ..run(used(1))
                }),
                StartState::Starting
            );
        }
        let mut c = 1;
        for p in 21..=40 {
            c += 2;
            let r = Reading {
                progress: p,
                ..run(used(c))
            };
            assert_eq!(
                j.observe(&r),
                StartState::Starting,
                "spin is measured from progress"
            );
        }
    }

    #[test]
    fn a_paused_process_counts_nothing() {
        let mut j = StartJudge::new(patience(), "x");
        j.observe(&run(used(1)));
        let paused = Reading {
            liveness: Liveness::Paused,
            ..run(used(1))
        };
        for _ in 0..100 {
            assert_eq!(j.observe(&paused), StartState::Starting);
        }
        assert_eq!(j.observe(&run(used(1))), StartState::Starting);
    }

    #[test]
    fn only_an_unobservable_process_meets_the_hang_guard() {
        let mut j = StartJudge::new(patience(), "x");
        for _ in 0..7 {
            assert_eq!(j.observe(&run(Cpu::Unavailable)), StartState::Starting);
        }
        assert!(
            matches!(cause(&j.observe(&run(Cpu::Unavailable))), Some(StallCause::Hung(d)) if d == S(8))
        );
        // A reading ends the blindness.
        let mut j = StartJudge::new(patience(), "x");
        for _ in 0..7 {
            j.observe(&run(Cpu::Unavailable));
        }
        assert_eq!(j.observe(&run(used(1))), StartState::Starting);
    }

    #[test]
    fn a_slow_start_gets_a_note_after_the_chosen_ticks_and_the_wait_goes_on() {
        let mut j = StartJudge::new(
            Patience {
                note_after: 3,
                ..patience()
            },
            "the agent",
        );
        assert_eq!(
            j.observe(&Reading {
                progress: 1,
                ..run(used(0))
            }),
            StartState::Starting
        );
        assert_eq!(
            j.observe(&Reading {
                progress: 2,
                ..run(used(1))
            }),
            StartState::Starting
        );
        let note = j.observe(&Reading {
            progress: 3,
            ..run(used(2))
        });
        assert_eq!(cause(&note), Some(StallCause::Slow));
        assert!(!StallCause::Slow.is_stuck());
        assert_eq!(
            note,
            StartState::Stalled(StallReport::new(StallCause::Slow, "the agent"))
        );
    }

    #[test]
    fn cpu_not_read_between_readings_adds_its_ticks_to_the_idle_window() {
        let mut j = StartJudge::new(patience(), "x");
        j.observe(&run(used(1)));
        for _ in 0..4 {
            assert_eq!(j.observe(&run(Cpu::NotRead)), StartState::Starting);
        }
        assert!(matches!(cause(&j.observe(&run(used(1)))), Some(StallCause::Idle(d)) if d == S(5)));
    }

    struct Script {
        cpu: VecDeque<Option<Duration>>,
        exit_after: Option<u32>,
        ready_after: Option<u32>,
        calls: u32,
    }
    impl StartProbe for Script {
        fn liveness(&mut self) -> Liveness {
            self.calls += 1;
            if self.exit_after.is_some_and(|n| self.calls > n) {
                Liveness::Exited(ExitInfo {
                    status: Some(2),
                    stderr_tail: Some("bad".into()),
                })
            } else {
                Liveness::Running
            }
        }
        fn ready(&mut self) -> bool {
            self.ready_after.is_some_and(|n| self.calls >= n)
        }
        fn cpu_time(&mut self) -> Option<Duration> {
            self.cpu.pop_front().flatten()
        }
    }
    /// A pace that records its waits and is cancelled once it has waited `.1` times.
    struct Ticks<'a>(&'a RefCell<Vec<Duration>>, usize);
    impl Pace for Ticks<'_> {
        fn wait(&mut self, tick: Duration) -> bool {
            self.0.borrow_mut().push(tick);
            self.0.borrow().len() < self.1
        }
    }
    fn script(cpu: Vec<Option<Duration>>) -> Script {
        Script {
            cpu: cpu.into(),
            exit_after: None,
            ready_after: None,
            calls: 0,
        }
    }

    #[test]
    fn the_watch_waits_on_its_pace_and_returns_ready() {
        let waits = RefCell::new(Vec::new());
        let mut s = script(vec![]);
        s.ready_after = Some(3);
        let mut w = StartWatch::new(patience(), "x", s, Ticks(&waits, usize::MAX));
        assert_eq!(w.wait(), Watched::State(StartState::Ready));
        assert_eq!(waits.borrow().len(), 3);
        assert_eq!(waits.borrow()[0], S(1));
        assert_eq!(w.into_probe().calls, 4);
    }

    #[test]
    fn the_watch_returns_an_exit_a_stall_and_a_cancel_and_waits_again_after_a_stall() {
        let waits = RefCell::new(Vec::new());
        let mut s = script(vec![]);
        s.exit_after = Some(1);
        let mut w = StartWatch::new(patience(), "x", s, Ticks(&waits, usize::MAX));
        assert!(matches!(w.wait(), Watched::State(StartState::Exited(i)) if i.status == Some(2)));

        let waits = RefCell::new(Vec::new());
        let p = Patience {
            cpu_every: 2,
            ..patience()
        };
        let s = script(vec![Some(S(1)); 20]);
        let mut w = StartWatch::new(p, "x", s, Ticks(&waits, usize::MAX));
        let st = state(w.wait());
        assert!(matches!(cause(&st), Some(StallCause::Idle(_))));
        let before = waits.borrow().len();
        // Waiting again does not repeat the stall: it waits (the pace is called) until something else happens, here the
        // script running out of CPU readings, which is the OS saying nothing (the hang guard).
        let again = state(w.wait());
        assert!(
            matches!(cause(&again), Some(StallCause::Hung(_))),
            "{again:?}"
        );
        assert!(
            waits.borrow().len() > before + 3,
            "the second wait paced itself, it did not busy-poll"
        );
        let probe = w.into_probe();
        assert!(
            probe.cpu.is_empty(),
            "the CPU was read every second tick: {} readings left",
            probe.cpu.len()
        );

        let waits = RefCell::new(Vec::new());
        let s = script(vec![Some(S(1)); 20]);
        let mut w = StartWatch::new(patience(), "x", s, Ticks(&waits, 3));
        assert_eq!(w.wait(), Watched::Cancelled);
    }

    #[test]
    fn a_stall_is_reported_once_per_episode_and_the_note_once() {
        let mut j = StartJudge::new(
            Patience {
                note_after: 2,
                ..patience()
            },
            "x",
        );
        let mut stalls = 0;
        // Idle for a long time: the note, then the idle stall, each once.
        for _ in 0..30 {
            if matches!(j.observe(&run(used(1))), StartState::Stalled(_)) {
                stalls += 1;
            }
        }
        assert_eq!(stalls, 2, "the note and the idle stall, once each");
        // The process runs: the episode ends; going idle again is a new episode.
        j.observe(&run(used(2)));
        for _ in 0..4 {
            assert_eq!(j.observe(&run(used(2))), StartState::Starting);
        }
        assert!(
            matches!(cause(&j.observe(&run(used(2)))), Some(StallCause::Idle(_))),
            "a new episode is reported again"
        );
        assert_eq!(j.observe(&run(used(2))), StartState::Starting);
    }

    #[test]
    fn a_progress_or_a_reading_clears_blindness_and_spin_and_idle() {
        // Blind for 7 of 8 s, then a reading: the blindness starts again from zero.
        let mut j = StartJudge::new(patience(), "x");
        for _ in 0..7 {
            j.observe(&run(Cpu::Unavailable));
        }
        j.observe(&run(used(1)));
        for _ in 0..7 {
            assert_eq!(
                j.observe(&run(Cpu::Unavailable)),
                StartState::Starting,
                "a reading reset the blindness"
            );
        }
        // Blind for 7 s, then progress: also reset.
        let mut j = StartJudge::new(patience(), "x");
        for _ in 0..7 {
            j.observe(&run(Cpu::Unavailable));
        }
        j.observe(&Reading {
            progress: 1,
            ..run(Cpu::Unavailable)
        });
        for _ in 0..6 {
            assert_eq!(
                j.observe(&run(Cpu::Unavailable)),
                StartState::Starting,
                "progress reset the blindness"
            );
        }
        // Idle for 4 of 5 s, then progress: idle starts again.
        let mut j = StartJudge::new(patience(), "x");
        for _ in 0..5 {
            j.observe(&run(used(1)));
        }
        j.observe(&Reading {
            progress: 1,
            ..run(used(1))
        });
        for _ in 0..3 {
            assert_eq!(
                j.observe(&run(used(1))),
                StartState::Starting,
                "progress reset idle"
            );
        }
    }

    #[test]
    fn a_caller_that_never_reads_the_cpu_is_bounded_by_the_hang_guard() {
        let p = Patience {
            cpu_every: 1,
            hang_guard: S(5),
            ..patience()
        };
        let mut j = StartJudge::new(p, "x");
        let mut last = StartState::Starting;
        for _ in 0..100 {
            last = j.observe(&run(Cpu::NotRead));
            if last != StartState::Starting {
                break;
            }
        }
        assert!(
            matches!(cause(&last), Some(StallCause::Hung(_))),
            "{last:?}"
        );
    }

    #[test]
    fn the_cpu_is_read_only_every_cpu_every_ticks() {
        let waits = RefCell::new(Vec::new());
        let p = Patience {
            cpu_every: 5,
            idle: S(1000),
            spin: S(1000),
            hang_guard: S(1000),
            ..patience()
        };
        let mut s = script(vec![Some(S(1)); 100]);
        s.ready_after = Some(21);
        let mut w = StartWatch::new(p, "x", s, Ticks(&waits, usize::MAX));
        assert_eq!(w.wait(), Watched::State(StartState::Ready));
        let probe = w.into_probe();
        assert_eq!(
            100 - probe.cpu.len(),
            5,
            "21 ticks at one read in 5: ticks 0, 5, 10, 15 and 20"
        );
    }

    #[test]
    fn a_probe_without_cpu_time_is_bounded_by_the_hang_guard() {
        let waits = RefCell::new(Vec::new());
        let mut w = StartWatch::new(patience(), "x", script(vec![]), Ticks(&waits, usize::MAX));
        let st = state(w.wait());
        assert!(
            matches!(cause(&st), Some(StallCause::Hung(_))),
            "no identity, no CPU: still bounded"
        );
    }

    #[test]
    fn tree_cpu_reads_the_own_tree_and_a_gone_process_is_unavailable() {
        let me = ProcessId::of(std::process::id()).expect("own identity");
        assert!(matches!(tree_cpu(me), Cpu::Used(_)));
        let gone = ProcessId::new(std::process::id(), crate::StartTime::from_micros(1));
        assert_eq!(tree_cpu(gone), Cpu::Unavailable);
    }

    #[test]
    fn the_tail_of_an_output_file_is_bounded_and_trimmed() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("err.log");
        assert_eq!(tail_of(&f, 10), None);
        std::fs::write(&f, "  \n").unwrap();
        assert_eq!(tail_of(&f, 10), None);
        std::fs::write(&f, "first line\nsecond line\n").unwrap();
        assert_eq!(tail_of(&f, 12).as_deref(), Some("second line"));
        std::fs::write(&f, "ééé").unwrap();
        assert!(tail_of(&f, 3).is_some_and(|t| t.ends_with('é')));
    }
}
