//! What the link tells the log, as pure functions so the lines are tested without a log or a clock: the phase changes (never one
//! per tick), a failed request (the variant's name, never its fields: a request can carry a path) and the start of an agent
//! that exited. Names, codes and millisecond counts only (the logging rules: no user content).

use super::machine::Snapshot;
use std::time::Instant;

/// One change of the link's phase, for one log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhaseChange {
    /// The phase left (a variant name).
    pub from: &'static str,
    /// The phase entered.
    pub to: &'static str,
    /// How long the phase left lasted.
    pub in_phase_ms: u64,
    /// How long since the link began.
    pub since_begin_ms: u64,
}

/// Remembers the last phase and says when it changed: a tick that changes nothing (a look's number counts up in a wait) is `None`.
#[derive(Debug)]
pub struct PhaseLog {
    begun: Instant,
    entered: Instant,
    last: &'static str,
}

impl PhaseLog {
    /// A log that starts in `Idle` at `now`.
    pub fn new(now: Instant) -> PhaseLog {
        PhaseLog {
            begun: now,
            entered: now,
            last: "Idle",
        }
    }

    /// The change `snapshot` shows against the last phase, if any.
    pub fn observe(&mut self, snapshot: &Snapshot, now: Instant) -> Option<PhaseChange> {
        let to = snapshot.phase.name();
        if to == self.last {
            return None;
        }
        let change = PhaseChange {
            from: self.last,
            to,
            in_phase_ms: ms(now.saturating_duration_since(self.entered)),
            since_begin_ms: ms(now.saturating_duration_since(self.begun)),
        };
        self.last = to;
        self.entered = now;
        Some(change)
    }
}

/// More phase changes than this within [`FLAP_WINDOW`] is a flapping link: its lines are replaced by one summary per window.
pub const FLAP_LIMIT: usize = 8;
/// The window a flapping link is judged over.
pub const FLAP_WINDOW: std::time::Duration = std::time::Duration::from_secs(10);

/// What the link tells the log about its phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseLine {
    /// One change of phase.
    Change(PhaseChange),
    /// The phase changed more than [`FLAP_LIMIT`] times in [`FLAP_WINDOW`]: this many changes so far, no more lines until the next window.
    Flapping {
        /// Changes in the window so far.
        changes: usize,
    },
}

/// [`PhaseLog`] with a guard against a link that flaps (a look that keeps connecting and dropping would write a line per change,
/// for ever): the first [`FLAP_LIMIT`] changes of a window are told, then one summary, then nothing until the window has passed.
#[derive(Debug)]
pub struct GuardedPhaseLog {
    log: PhaseLog,
    recent: std::collections::VecDeque<Instant>,
    summarised: Option<Instant>,
}

impl GuardedPhaseLog {
    /// A log that starts in `Idle` at `now`.
    pub fn new(now: Instant) -> GuardedPhaseLog {
        GuardedPhaseLog {
            log: PhaseLog::new(now),
            recent: std::collections::VecDeque::new(),
            summarised: None,
        }
    }

    /// The line `snapshot` calls for, if any.
    pub fn observe(&mut self, snapshot: &Snapshot, now: Instant) -> Option<PhaseLine> {
        let change = self.log.observe(snapshot, now)?;
        while self.recent.front().is_some_and(|t| now.saturating_duration_since(*t) > FLAP_WINDOW) {
            self.recent.pop_front();
        }
        self.recent.push_back(now);
        if self.recent.len() <= FLAP_LIMIT {
            self.summarised = None;
            return Some(PhaseLine::Change(change));
        }
        match self.summarised {
            Some(at) if now.saturating_duration_since(at) < FLAP_WINDOW => None,
            _ => {
                self.summarised = Some(now);
                Some(PhaseLine::Flapping {
                    changes: self.recent.len(),
                })
            }
        }
    }
}

/// Connect attempts of one wait: how many failed in a row, since when, and which reasons were told. The failure line is told when the
/// reason changes (with the attempt number and the time since the link began), the success line carries the attempts it took.
#[derive(Debug)]
pub struct ConnectAttempts {
    begun: Instant,
    failures: u32,
    last_reason: Option<String>,
}

/// A failed connect attempt to tell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectFailure {
    /// Why it failed (a fixed sentence or an error kind).
    pub reason: String,
    /// Which attempt of this wait it was.
    pub attempt: u32,
    /// How long since the link began.
    pub since_begin_ms: u64,
}

/// A successful connect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectSuccess {
    /// How many attempts it took, this one included.
    pub attempts: u32,
    /// How long since the link began.
    pub since_begin_ms: u64,
}

impl ConnectAttempts {
    /// A counter for a link that began at `begun`.
    pub fn new(begun: Instant) -> ConnectAttempts {
        ConnectAttempts {
            begun,
            failures: 0,
            last_reason: None,
        }
    }

    /// An attempt failed for `reason`: `Some` when the reason differs from the last one told (the count goes on either way).
    pub fn failed(&mut self, reason: &str, now: Instant) -> Option<ConnectFailure> {
        self.failures += 1;
        if self.last_reason.as_deref() == Some(reason) {
            return None;
        }
        self.last_reason = Some(reason.to_string());
        Some(ConnectFailure {
            reason: reason.to_string(),
            attempt: self.failures,
            since_begin_ms: ms(now.saturating_duration_since(self.begun)),
        })
    }

    /// An attempt succeeded: the attempts it took, and the counter starts again.
    pub fn succeeded(&mut self, now: Instant) -> ConnectSuccess {
        let done = ConnectSuccess {
            attempts: self.failures + 1,
            since_begin_ms: ms(now.saturating_duration_since(self.begun)),
        };
        self.failures = 0;
        self.last_reason = None;
        done
    }
}

/// A [`ChangeGate`] per key (per request name): two polls that alternate, each failing the same way, are two states, not a flap.
#[derive(Debug, Default)]
pub struct KeyedGate(std::collections::HashMap<String, String>);

impl KeyedGate {
    /// Whether `state` differs from the last one that passed for `key`; remembers it when it does.
    pub fn changed(&mut self, key: &str, state: &str) -> bool {
        if self.0.get(key).map(String::as_str) == Some(state) {
            false
        } else {
            self.0.insert(key.to_string(), state.to_string());
            true
        }
    }

    /// `key` worked: its next failure is a change again.
    pub fn reset(&mut self, key: &str) {
        self.0.remove(key);
    }
}

fn ms(d: std::time::Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// The name of a request variant from its `Debug` text: everything before the first field or bracket. Only this is logged for a
/// failed request, because a request's fields can hold a path the person chose.
pub fn variant_name(debug: &str) -> &str {
    let end = debug
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(debug.len());
    if end == 0 {
        "Unknown"
    } else {
        &debug[..end]
    }
}

/// Lets a line through only when its text (without the numbers that change every time) differs from the last one that passed:
/// a polling loop that sees the same state again and again writes one line, not one per tick (the logging rules: log on change).
#[derive(Debug, Default)]
pub struct ChangeGate(Option<String>);

impl ChangeGate {
    /// Forgets the last key (the state ended: the next one, even the same, is a change).
    pub fn reset(&mut self) {
        self.0 = None;
    }

    /// Whether `key` differs from the last key that passed; remembers it when it does.
    pub fn changed(&mut self, key: &str) -> bool {
        if self.0.as_deref() == Some(key) {
            false
        } else {
            self.0 = Some(key.to_string());
            true
        }
    }
}

/// A writer that takes at most 48 bytes and then refuses: the name of a request is its first word, and a request can carry a whole
/// settings object, which must not be formatted only to be cut.
struct Cap(String);

impl std::fmt::Write for Cap {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        if self.0.len() + s.len() > 48 {
            let room = 48 - self.0.len();
            self.0.push_str(s.get(..room).unwrap_or(""));
            return Err(std::fmt::Error);
        }
        self.0.push_str(s);
        Ok(())
    }
}

/// The variant name of any `Debug` value, formatting no more than its first bytes.
pub fn debug_variant_name(value: &dyn std::fmt::Debug) -> String {
    use std::fmt::Write;
    let mut cap = Cap(String::new());
    let _ = write!(cap, "{value:?}");
    variant_name(&cap.0).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::machine::{Phase, Snapshot};
    use std::time::{Duration, Instant};

    fn snap(phase: Phase) -> Snapshot {
        Snapshot {
            phase,
            stall: None,
            ever_connected: false,
            child: None,
            looks: 0,
            changes_answered: 0,
        }
    }

    /// ADR (logging for root cause) D3: the link's phases are the story of a slow login connect. A change of phase is one line
    /// with the phase left, the phase entered and how long the left one lasted; the 100 ms ticks that change nothing write nothing.
    #[test]
    fn a_phase_change_is_one_line_and_a_run_of_ticks_writes_none() {
        let t0 = Instant::now();
        let mut log = PhaseLog::new(t0);
        let first = log.observe(&snap(Phase::Looking { seq: 1 }), t0).expect("the first phase is told");
        assert_eq!(
            (first.from, first.to, first.in_phase_ms, first.since_begin_ms),
            ("Idle", "Looking", 0, 0)
        );
        for k in 1..=50 {
            // The look's number changes with every tick of a wait; the phase does not.
            let s = snap(Phase::Looking { seq: 1 + k });
            assert_eq!(log.observe(&s, t0 + Duration::from_millis(100 * k)), None, "tick {k}");
        }
        let c = log
            .observe(&snap(Phase::WaitingForService), t0 + Duration::from_millis(5_100))
            .unwrap();
        assert_eq!(
            (c.from, c.to, c.in_phase_ms, c.since_begin_ms),
            ("Looking", "WaitingForService", 5_100, 5_100)
        );
        let c = log
            .observe(
                &snap(Phase::Connected {
                    peer: 1,
                    owner: crate::link::machine::Owner::Other,
                }),
                t0 + Duration::from_millis(30_000),
            )
            .unwrap();
        assert_eq!(
            (c.from, c.to, c.in_phase_ms, c.since_begin_ms),
            ("WaitingForService", "Connected", 24_900, 30_000)
        );
    }

    #[test]
    fn every_phase_has_a_stable_name() {
        use crate::link::machine::Owner;
        let named = [
            (Phase::Idle, "Idle"),
            (Phase::Looking { seq: 1 }, "Looking"),
            (Phase::Starting { child: 1 }, "Starting"),
            (Phase::WaitingForService, "WaitingForService"),
            (Phase::WaitingForHolder, "WaitingForHolder"),
            (Phase::Replacing { peer: 1 }, "Replacing"),
            (Phase::TakingBack { peer: 1 }, "TakingBack"),
            (Phase::Ending { child: 1 }, "Ending"),
            (
                Phase::BackingOff {
                    failures: 1,
                    ticks_left: 1,
                },
                "BackingOff",
            ),
            (
                Phase::Connected {
                    peer: 1,
                    owner: Owner::Other,
                },
                "Connected",
            ),
        ];
        for (phase, name) in named {
            assert_eq!(phase.name(), name);
        }
    }

    /// D7: one place logs every failed request. A request's Debug text can hold a path the person chose; only its name is written.
    #[test]
    fn a_failed_requests_log_line_names_the_variant_and_never_its_fields() {
        assert_eq!(variant_name("SaveReport { path: \"/Users/someone/secret.pdf\" }"), "SaveReport");
        assert_eq!(variant_name("Settings"), "Settings");
        assert_eq!(variant_name("History(Range { from: 1 })"), "History");
        assert_eq!(variant_name(""), "Unknown");
    }

    #[test]
    fn a_repeated_state_passes_the_gate_once_until_it_changes() {
        let mut g = ChangeGate::default();
        assert!(g.changed("no file"));
        for _ in 0..1_000 {
            assert!(!g.changed("no file"));
        }
        assert!(g.changed("file of this build"));
        assert!(g.changed("no file"), "back to an earlier state is a change");
    }

    #[test]
    fn the_name_of_a_big_request_is_read_without_formatting_all_of_it() {
        #[derive(Debug)]
        #[allow(dead_code)]
        enum Req {
            SaveSettings { everything: String },
            Settings,
        }
        assert_eq!(
            debug_variant_name(&Req::SaveSettings {
                everything: "x".repeat(1_000_000)
            }),
            "SaveSettings"
        );
        assert_eq!(debug_variant_name(&Req::Settings), "Settings");
    }

    /// ADR 0036 D3: a link that flaps (connects and drops every second) must not write a line per change for ever; more than 8 phase
    /// changes in 10 s is flapping.
    #[test]
    fn a_flapping_link_is_summarised_once_per_window_instead_of_a_line_per_change() {
        assert_eq!((FLAP_LIMIT, FLAP_WINDOW), (8, Duration::from_secs(10)), "ADR 0036: 8 changes per 10 s");
        let t0 = Instant::now();
        let mut log = GuardedPhaseLog::new(t0);
        let mut lines = Vec::new();
        for k in 0..20u64 {
            let phase = if k % 2 == 0 {
                Phase::Looking { seq: k }
            } else {
                Phase::WaitingForService
            };
            lines.extend(log.observe(&snap(phase), t0 + Duration::from_millis(400 * k)));
        }
        let changes = lines.iter().filter(|l| matches!(l, PhaseLine::Change(_))).count();
        let summaries = lines.iter().filter(|l| matches!(l, PhaseLine::Flapping { .. })).count();
        assert_eq!((changes, summaries), (FLAP_LIMIT, 1), "{lines:?}");
        // After the window has passed another summary is allowed, and a calm link tells its changes again.
        let later = t0 + Duration::from_secs(200);
        assert!(matches!(
            log.observe(&snap(Phase::Looking { seq: 1000 }), later),
            Some(PhaseLine::Change(_))
        ));
    }

    #[test]
    fn connect_attempts_tell_a_new_reason_with_its_attempt_number_and_a_success_with_the_attempts_it_took() {
        let t0 = Instant::now();
        let mut c = ConnectAttempts::new(t0);
        let f = c.failed("there is no discovery file", t0 + Duration::from_millis(100)).unwrap();
        assert_eq!((f.attempt, f.since_begin_ms), (1, 100));
        assert_eq!(
            c.failed("there is no discovery file", t0 + Duration::from_millis(200)),
            None,
            "the same reason is not told again"
        );
        let f = c.failed("the agent did not answer", t0 + Duration::from_millis(300)).unwrap();
        assert_eq!(f.attempt, 3, "the count goes on while the reason is not told");
        let ok = c.succeeded(t0 + Duration::from_millis(400));
        assert_eq!((ok.attempts, ok.since_begin_ms), (4, 400));
        assert_eq!(
            c.failed("there is no discovery file", t0 + Duration::from_millis(500))
                .unwrap()
                .attempt,
            1,
            "a new wait counts from one"
        );
    }

    /// The 15 s throttling poll and the 60 s baselines poll fail the same way and alternate: one gate for both would pass every line.
    #[test]
    fn alternating_polls_that_fail_the_same_way_are_told_once_each() {
        let mut g = KeyedGate::default();
        let mut told = 0;
        for _ in 0..100 {
            for poll in ["Throttling", "Baselines"] {
                if g.changed(poll, "AgentUnavailable") {
                    told += 1;
                }
            }
        }
        assert_eq!(told, 2);
        g.reset("Throttling");
        assert!(
            g.changed("Throttling", "AgentUnavailable"),
            "after a success a failure is news again"
        );
        assert!(!g.changed("Baselines", "AgentUnavailable"));
    }
}
