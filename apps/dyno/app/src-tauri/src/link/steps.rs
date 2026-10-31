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
}
