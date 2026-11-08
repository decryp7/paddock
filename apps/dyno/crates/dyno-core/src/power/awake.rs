//! Keep awake (ADR 0032 section 2 and 8): the request, its validation, and the pure state machine. The machine never calls
//! the OS: it takes events with the time and answers with effects the agent's power thread runs.
//!
//! ```text
//! Off --Start--> Starting --Acquired--> Holding --OsEnded(why)--> Waiting --Woke & spec applies--> Starting
//! Holding | Waiting --Stop | deadline | process exited | busy false for grace--> Off (Release)
//! Starting --AcquireFailed(why)--> Ended(why);  Off --Restarted--> Ended("Dyno restarted")
//! ```

use crate::process::ProcessRef;
use crate::time::{from_local, next_day_start, to_local, LocalZone, Timestamp, DAY, MINUTE, SECOND};
use serde::{Deserialize, Serialize};

/// The longest "for" hold: a week.
pub const MAX_FOR_MINUTES: u32 = 7 * 24 * 60;
/// How many conditions a "while busy" hold may have.
pub const MAX_CONDITIONS: usize = 8;
/// The longest program name in a condition.
pub const MAX_PROGRAM_CHARS: usize = 256;
/// The shortest and longest grace of a "while busy" hold, in seconds.
pub const GRACE_RANGE_SECS: (u32, u32) = (10, 3600);

/// What stays awake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AwakeScope {
    /// The system; the display may sleep.
    System,
    /// The system and the display.
    SystemAndDisplay,
}

/// One "busy" condition of a "while busy" hold.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum BusyCond {
    /// CPU above this percentage.
    Cpu {
        /// The threshold, 0 to 100.
        above_pct: f32,
    },
    /// Network above this many bytes per second (both directions added).
    Network {
        /// The threshold.
        above_bytes_per_s: u64,
    },
    /// Disk above this many bytes per second (reads and writes added).
    Disk {
        /// The threshold.
        above_bytes_per_s: u64,
    },
    /// A program with this name is running.
    Process {
        /// The program's file name, compared without case.
        program: String,
    },
}

/// Until when to keep awake.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AwakeUntil {
    /// Until the user stops it.
    Indefinite,
    /// For this many minutes.
    For {
        /// 1 to [`MAX_FOR_MINUTES`].
        minutes: u32,
    },
    /// Until the next time the local clock shows this minute of the day.
    At {
        /// 0 to 1439.
        minute_of_day: u16,
    },
    /// While this process runs (held by its pid and start time, never by pid alone).
    WhileProcess {
        /// The process.
        process: ProcessRef,
    },
    /// While any condition holds, and for `grace_secs` after the last one stopped holding.
    WhileBusy {
        /// At least one, at most [`MAX_CONDITIONS`].
        conds: Vec<BusyCond>,
        /// Seconds, within [`GRACE_RANGE_SECS`].
        grace_secs: u32,
    },
}

/// A keep-awake request. One is active at a time; a new one replaces it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwakeSpec {
    /// What stays awake.
    pub scope: AwakeScope,
    /// Until when.
    pub until: AwakeUntil,
}

impl AwakeSpec {
    /// Checks the request; the message says what to fix. The agent re-validates whatever the UI sent.
    pub fn validate(&self) -> Result<(), String> {
        match &self.until {
            AwakeUntil::Indefinite | AwakeUntil::WhileProcess { .. } => Ok(()),
            AwakeUntil::For { minutes } if (1..=MAX_FOR_MINUTES).contains(minutes) => Ok(()),
            AwakeUntil::For { .. } => Err(format!("Keep awake for 1 minute up to {} days.", MAX_FOR_MINUTES / 1440)),
            AwakeUntil::At { minute_of_day } if *minute_of_day < 1440 => Ok(()),
            AwakeUntil::At { .. } => Err("Pick a time of day between 00:00 and 23:59.".into()),
            AwakeUntil::WhileBusy { conds, grace_secs } => {
                if conds.is_empty() || conds.len() > MAX_CONDITIONS {
                    return Err(format!("Pick between 1 and {MAX_CONDITIONS} conditions."));
                }
                if !(GRACE_RANGE_SECS.0..=GRACE_RANGE_SECS.1).contains(grace_secs) {
                    return Err(format!(
                        "The wait after the last busy moment is {} to {} seconds.",
                        GRACE_RANGE_SECS.0, GRACE_RANGE_SECS.1
                    ));
                }
                conds.iter().try_for_each(BusyCond::validate)
            }
        }
    }

    /// When the hold ends by itself, if it does by the clock: `For` and `At` (the next local occurrence of the minute, DST
    /// gaps and folds through the zone port).
    pub fn deadline(&self, now: Timestamp, zone: &dyn LocalZone) -> Option<Timestamp> {
        match &self.until {
            AwakeUntil::For { minutes } => Some(now.plus(i64::from(*minutes) * MINUTE)),
            AwakeUntil::At { minute_of_day } => {
                let offset = i64::from(*minute_of_day) * MINUTE;
                let today = to_local(now, zone).div_euclid(DAY) * DAY;
                let candidate = from_local(today + offset, zone);
                if candidate > now {
                    return Some(candidate);
                }
                let tomorrow = to_local(next_day_start(now, zone), zone);
                Some(from_local(tomorrow + offset, zone))
            }
            _ => None,
        }
    }

    /// The words the OS shows for this hold, after the program's name "Dyno" (`pmset -g assertions`, `powercfg /requests`,
    /// `systemd-inhibit --list`): "Dyno: keeping awake for 1 h, you asked".
    pub fn reason(&self) -> String {
        let what = match &self.until {
            AwakeUntil::Indefinite => "until you stop it".to_string(),
            AwakeUntil::For { minutes } => format!("for {}", duration_words(i64::from(*minutes) * MINUTE)),
            AwakeUntil::At { minute_of_day } => format!("until {:02}:{:02}", minute_of_day / 60, minute_of_day % 60),
            AwakeUntil::WhileProcess { .. } => "while a program you chose runs".to_string(),
            AwakeUntil::WhileBusy { .. } => "while the computer is busy".to_string(),
        };
        format!("keeping awake {what}, you asked")
    }
}

impl BusyCond {
    fn validate(&self) -> Result<(), String> {
        match self {
            BusyCond::Cpu { above_pct } if above_pct.is_finite() && (0.0..=100.0).contains(above_pct) => Ok(()),
            BusyCond::Cpu { .. } => Err("CPU busy is a percentage from 0 to 100.".into()),
            BusyCond::Network { above_bytes_per_s } | BusyCond::Disk { above_bytes_per_s } if *above_bytes_per_s > 0 => Ok(()),
            BusyCond::Network { .. } | BusyCond::Disk { .. } => Err("A network or disk rate above 0 bytes per second.".into()),
            BusyCond::Process { program } if !program.trim().is_empty() && program.chars().count() <= MAX_PROGRAM_CHARS => Ok(()),
            BusyCond::Process { .. } => Err(format!("Give a program name of 1 to {MAX_PROGRAM_CHARS} characters.")),
        }
    }
}

impl BusyCond {
    /// What this condition reads now, in words, when it holds ("CPU at 72%"); `None` when it does not hold. A reading the OS
    /// withheld holds too and says so.
    fn reading(&self, i: &Inputs) -> Option<String> {
        match self {
            BusyCond::Cpu { above_pct } => match i.cpu_pct {
                Some(v) => (v > *above_pct).then(|| format!("CPU at {v:.0}%")),
                None => Some("CPU reading unavailable".into()),
            },
            BusyCond::Network { above_bytes_per_s } => match i.net_bytes_per_s {
                Some(v) => (v > *above_bytes_per_s as f64).then(|| format!("network at {:.0} KB/s", v / 1024.0)),
                None => Some("network reading unavailable".into()),
            },
            BusyCond::Disk { above_bytes_per_s } => match i.disk_bytes_per_s {
                Some(v) => (v > *above_bytes_per_s as f64).then(|| format!("disk at {:.1} MB/s", v / 1_048_576.0)),
                None => Some("disk reading unavailable".into()),
            },
            BusyCond::Process { program } => match &i.programs {
                Some(all) => all
                    .iter()
                    .any(|p| p.eq_ignore_ascii_case(program.trim()))
                    .then(|| format!("{} is running", program.trim())),
                None => Some("program list unavailable".into()),
            },
        }
    }
}

/// Where a "while busy" hold stands right now: busy (holding because of `detail`) or quiet (waiting for the computer to get busy
/// again, with the time left before it lets go).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BusyNow {
    /// Whether any condition holds now.
    pub active: bool,
    /// What holds ("CPU at 72% and network at 900 KB/s"), or "quiet".
    pub detail: String,
    /// How long it has been quiet, in seconds (0 while busy).
    pub quiet_secs: u64,
    /// Seconds until it lets go if it stays quiet (`None` while busy).
    pub release_in_secs: Option<u64>,
}

fn duration_words(ms: i64) -> String {
    let minutes = ms / MINUTE;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) if h % 24 == 0 => format!("{} d", h / 24),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// What the machine reads at each tick. `None` is a reading the OS withheld.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Inputs {
    /// CPU load, 0 to 100.
    pub cpu_pct: Option<f32>,
    /// Network bytes per second, both directions.
    pub net_bytes_per_s: Option<f64>,
    /// Disk bytes per second, reads plus writes.
    pub disk_bytes_per_s: Option<f64>,
    /// File names of the running programs.
    pub programs: Option<Vec<String>>,
    /// Whether the process of a "while process" hold still runs (pid and start time checked).
    pub process_alive: Option<bool>,
}

/// Something that happened.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The user asked for a hold; `ends` is [`AwakeSpec::deadline`].
    Start {
        /// The request.
        spec: AwakeSpec,
        /// When it ends by the clock.
        ends: Option<Timestamp>,
    },
    /// The user stopped it.
    Stop,
    /// The OS took the hold ([`Effect::Acquire`] succeeded).
    Acquired {
        /// The OS's reference, for logs and the page.
        os_ref: String,
    },
    /// The OS refused the hold.
    AcquireFailed {
        /// Plain words.
        why: String,
    },
    /// The OS ended the hold (the user put the computer to sleep, the lid closed, Windows Modern Standby on battery).
    OsEnded {
        /// Plain words.
        why: String,
    },
    /// The computer woke.
    Woke,
    /// A reading round (the sampler's values, no new polling).
    Tick(Inputs),
    /// The agent started and found it had held awake before it stopped without saying so (a crash or a restart).
    Restarted,
}

/// What the power thread must do.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Take (or replace) the OS hold with this scope and reason.
    Acquire {
        /// What stays awake.
        scope: AwakeScope,
        /// The OS's wording.
        reason: String,
    },
    /// Release the OS hold.
    Release,
    /// Write one decision to the log (no user content).
    Log(String),
}

/// What the page shows.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AwakeView {
    /// Nothing is held.
    #[default]
    Off,
    /// Held.
    Holding {
        /// The request.
        spec: AwakeSpec,
        /// Since when.
        since: Timestamp,
        /// When it ends by the clock, if it does.
        ends: Option<Timestamp>,
        /// The OS's reference.
        os_ref: String,
        /// For a "while busy" hold: busy now, or waiting for the computer to get busy (`None` for the other holds).
        busy: Option<BusyNow>,
    },
    /// The OS ended the hold; it is taken again on wake.
    Waiting {
        /// The request.
        spec: AwakeSpec,
        /// Why the OS ended it.
        why: String,
        /// Since when the hold was asked for (it keeps its start while paused).
        since: Timestamp,
        /// When it ends by the clock, if it does.
        ends: Option<Timestamp>,
    },
    /// The hold ended without the user asking: the OS refused, or Dyno restarted.
    Ended {
        /// Why, in plain words.
        why: String,
        /// When.
        at: Timestamp,
    },
}

#[derive(Debug, Clone, PartialEq)]
struct Active {
    spec: AwakeSpec,
    ends: Option<Timestamp>,
    since: Timestamp,
    last_busy: Timestamp,
    busy: Option<BusyNow>,
}

#[derive(Debug, Clone, PartialEq)]
enum State {
    Off,
    Starting(Active),
    Holding(Active, String),
    Waiting(Active, String),
    Ended(String, Timestamp),
}

/// The keep-awake state machine. Owns no clock: every event carries `now`.
#[derive(Debug, Clone, PartialEq)]
pub struct AwakeMachine {
    state: State,
}

impl Default for AwakeMachine {
    fn default() -> Self {
        Self { state: State::Off }
    }
}

impl AwakeMachine {
    /// An idle machine.
    pub fn new() -> Self {
        Self::default()
    }

    /// What the page shows now.
    pub fn view(&self) -> AwakeView {
        match &self.state {
            State::Off | State::Starting(_) => AwakeView::Off,
            State::Holding(a, os_ref) => AwakeView::Holding {
                spec: a.spec.clone(),
                since: a.since,
                ends: a.ends,
                os_ref: os_ref.clone(),
                busy: a.busy.clone(),
            },
            State::Waiting(a, why) => AwakeView::Waiting {
                spec: a.spec.clone(),
                why: why.clone(),
                since: a.since,
                ends: a.ends,
            },
            State::Ended(why, at) => AwakeView::Ended { why: why.clone(), at: *at },
        }
    }

    /// Whether the OS hold is (or should be) taken, so the agent knows to keep ticking and to mark a clean stop.
    pub fn is_active(&self) -> bool {
        matches!(self.state, State::Starting(_) | State::Holding(..) | State::Waiting(..))
    }

    /// When the machine next needs a tick even if nothing else happens: the clock deadline of the active hold, if any.
    pub fn next_deadline(&self) -> Option<Timestamp> {
        match &self.state {
            State::Starting(a) | State::Holding(a, _) | State::Waiting(a, _) => a.ends,
            _ => None,
        }
    }

    /// The process a "while process" hold follows, so the agent reads whether it still runs.
    pub fn watching_process(&self) -> Option<ProcessRef> {
        match &self.state {
            State::Holding(a, _) => match a.spec.until {
                AwakeUntil::WhileProcess { process } => Some(process),
                _ => None,
            },
            _ => None,
        }
    }

    /// Applies one event at `now`; returns the effects to run, in order.
    pub fn step(&mut self, event: Event, now: Timestamp) -> Vec<Effect> {
        let state = std::mem::replace(&mut self.state, State::Off);
        let (next, effects) = transition(state, event, now);
        self.state = next;
        effects
    }
}

fn log(text: impl Into<String>) -> Effect {
    Effect::Log(text.into())
}

fn acquire(a: &Active) -> Effect {
    Effect::Acquire {
        scope: a.spec.scope,
        reason: a.spec.reason(),
    }
}

fn transition(state: State, event: Event, now: Timestamp) -> (State, Vec<Effect>) {
    match (state, event) {
        (_, Event::Start { spec, ends }) => {
            let a = Active {
                spec,
                ends,
                since: now,
                last_busy: now,
                busy: None,
            };
            let fx = vec![log("keep-awake start requested"), acquire(&a)];
            (State::Starting(a), fx)
        }
        (State::Off | State::Ended(..), Event::Stop) => (State::Off, vec![]),
        (_, Event::Stop) => (State::Off, vec![Effect::Release, log("keep-awake stopped by the user")]),
        (State::Starting(a), Event::Acquired { os_ref }) => {
            let fx = vec![log(format!("keep-awake holding os_ref={os_ref}"))];
            (State::Holding(Active { since: now, ..a }, os_ref), fx)
        }
        (_, Event::Acquired { os_ref }) => (
            State::Off,
            vec![Effect::Release, log(format!("keep-awake late hold released os_ref={os_ref}"))],
        ),
        (State::Starting(_), Event::AcquireFailed { why }) => (
            State::Ended(why.clone(), now),
            vec![Effect::Release, log(format!("keep-awake could not start: {why}"))],
        ),
        (s, Event::AcquireFailed { .. }) => (s, vec![]),
        (State::Holding(a, _), Event::OsEnded { why }) => {
            let fx = vec![Effect::Release, log(format!("keep-awake ended by the OS: {why}; waiting for wake"))];
            (State::Waiting(a, why), fx)
        }
        (s, Event::OsEnded { .. }) => (s, vec![]),
        (State::Waiting(a, _), Event::Woke) => {
            if a.ends.is_some_and(|e| e <= now) {
                (State::Off, vec![log("keep-awake deadline passed while asleep; not taken again")])
            } else {
                let fx = vec![log("keep-awake taking the hold again after wake"), acquire(&a)];
                (State::Starting(a), fx)
            }
        }
        (s, Event::Woke) => (s, vec![]),
        (State::Off, Event::Restarted) => (
            State::Ended("Dyno restarted".into(), now),
            vec![log("keep-awake ended: Dyno restarted; not taken again")],
        ),
        (s, Event::Restarted) => (s, vec![]),
        (State::Holding(a, os_ref), Event::Tick(inputs)) => tick_holding(a, os_ref, &inputs, now),
        (State::Waiting(a, why), Event::Tick(_)) => {
            if a.ends.is_some_and(|e| e <= now) {
                (State::Off, vec![log("keep-awake deadline passed while asleep")])
            } else {
                (State::Waiting(a, why), vec![])
            }
        }
        (s, Event::Tick(_)) => (s, vec![]),
    }
}

fn tick_holding(mut a: Active, os_ref: String, i: &Inputs, now: Timestamp) -> (State, Vec<Effect>) {
    let end = |why: &str| {
        (
            State::Off,
            vec![Effect::Release, log(format!("keep-awake released: {why} os_ref={os_ref}"))],
        )
    };
    if a.ends.is_some_and(|e| e <= now) {
        return end("deadline reached");
    }
    match &a.spec.until {
        AwakeUntil::WhileProcess { .. } if i.process_alive == Some(false) => return end("the program quit"),
        AwakeUntil::WhileBusy { conds, grace_secs } => {
            let holding: Vec<String> = conds.iter().filter_map(|c| c.reading(i)).collect();
            if !holding.is_empty() {
                a.last_busy = now;
                a.busy = Some(BusyNow {
                    active: true,
                    detail: holding.join(" and "),
                    quiet_secs: 0,
                    release_in_secs: None,
                });
            } else {
                let quiet = now.since(a.last_busy).max(0);
                if quiet >= i64::from(*grace_secs) * SECOND {
                    return end("not busy for the whole grace");
                }
                a.busy = Some(BusyNow {
                    active: false,
                    detail: "quiet".into(),
                    quiet_secs: u64::try_from(quiet / SECOND).unwrap_or(0),
                    release_in_secs: Some(u64::try_from((i64::from(*grace_secs) * SECOND - quiet) / SECOND).unwrap_or(0)),
                });
            }
        }
        _ => {}
    }
    (State::Holding(a, os_ref), vec![])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::{FixedZone, HOUR};

    const T0: Timestamp = Timestamp(1_700_000_000_000);

    fn spec(until: AwakeUntil) -> AwakeSpec {
        AwakeSpec {
            scope: AwakeScope::System,
            until,
        }
    }

    fn start(m: &mut AwakeMachine, s: AwakeSpec, ends: Option<Timestamp>, now: Timestamp) -> Vec<Effect> {
        m.step(Event::Start { spec: s, ends }, now)
    }

    fn holding(until: AwakeUntil, ends: Option<Timestamp>) -> AwakeMachine {
        let mut m = AwakeMachine::new();
        start(&mut m, spec(until), ends, T0);
        m.step(Event::Acquired { os_ref: "ref".into() }, T0);
        m
    }

    fn idle_inputs() -> Inputs {
        Inputs {
            cpu_pct: Some(1.0),
            net_bytes_per_s: Some(0.0),
            disk_bytes_per_s: Some(0.0),
            programs: Some(vec![]),
            process_alive: Some(true),
        }
    }

    #[test]
    fn start_acquires_with_the_reason_then_holds() {
        let mut m = AwakeMachine::new();
        let fx = start(&mut m, spec(AwakeUntil::For { minutes: 60 }), Some(T0.plus(HOUR)), T0);
        assert!(fx.contains(&Effect::Acquire {
            scope: AwakeScope::System,
            reason: "keeping awake for 1 h, you asked".into()
        }));
        assert_eq!(m.view(), AwakeView::Off, "not held until the OS says so");
        m.step(Event::Acquired { os_ref: "x".into() }, T0.plus(5));
        assert!(matches!(m.view(), AwakeView::Holding { since, ends: Some(e), .. } if since == T0.plus(5) && e == T0.plus(HOUR)));
        assert!(m.is_active());
        assert_eq!(m.next_deadline(), Some(T0.plus(HOUR)));
    }

    #[test]
    fn refusal_ends_it_with_the_reason_and_drops_an_older_hold() {
        let mut m = holding(AwakeUntil::Indefinite, None);
        start(&mut m, spec(AwakeUntil::Indefinite), None, T0.plus(10));
        let fx = m.step(Event::AcquireFailed { why: "no logind".into() }, T0.plus(11));
        assert!(fx.contains(&Effect::Release));
        assert_eq!(
            m.view(),
            AwakeView::Ended {
                why: "no logind".into(),
                at: T0.plus(11)
            }
        );
        assert!(!m.is_active());
        // "Turn on again" works from Ended.
        let fx = start(&mut m, spec(AwakeUntil::Indefinite), None, T0.plus(20));
        assert!(fx.iter().any(|f| matches!(f, Effect::Acquire { .. })));
    }

    #[test]
    fn stop_releases_only_when_something_is_held() {
        let mut m = AwakeMachine::new();
        assert!(m.step(Event::Stop, T0).is_empty());
        let mut m = holding(AwakeUntil::Indefinite, None);
        assert!(m.step(Event::Stop, T0).contains(&Effect::Release));
        assert_eq!(m.view(), AwakeView::Off);
    }

    #[test]
    fn the_deadline_releases_on_the_first_tick_after_it() {
        let mut m = holding(AwakeUntil::For { minutes: 1 }, Some(T0.plus(MINUTE)));
        assert!(m.step(Event::Tick(idle_inputs()), T0.plus(MINUTE - 1)).is_empty());
        assert!(m.step(Event::Tick(idle_inputs()), T0.plus(MINUTE)).contains(&Effect::Release));
        assert_eq!(m.view(), AwakeView::Off);
    }

    #[test]
    fn a_process_hold_ends_when_the_process_is_gone_and_keeps_on_unknown() {
        let p = ProcessRef {
            pid: 42,
            started: Some(Timestamp(5)),
        };
        let mut m = holding(AwakeUntil::WhileProcess { process: p }, None);
        let unknown = Inputs {
            process_alive: None,
            ..idle_inputs()
        };
        assert!(m.step(Event::Tick(unknown), T0.plus(SECOND)).is_empty());
        assert!(m.step(Event::Tick(idle_inputs()), T0.plus(2 * SECOND)).is_empty());
        let gone = Inputs {
            process_alive: Some(false),
            ..idle_inputs()
        };
        assert!(m.step(Event::Tick(gone), T0.plus(3 * SECOND)).contains(&Effect::Release));
    }

    #[test]
    fn a_busy_hold_says_what_holds_and_how_long_until_it_lets_go() {
        let until = AwakeUntil::WhileBusy {
            conds: vec![BusyCond::Cpu { above_pct: 50.0 }, BusyCond::Network { above_bytes_per_s: 1000 }],
            grace_secs: 120,
        };
        let mut m = holding(until, None);
        let busy = |m: &AwakeMachine| match m.view() {
            AwakeView::Holding { busy, .. } => busy,
            other => panic!("{other:?}"),
        };
        assert_eq!(busy(&m), None, "nothing is known before the first reading");
        let loud = Inputs {
            cpu_pct: Some(72.4),
            net_bytes_per_s: Some(2048.0),
            ..idle_inputs()
        };
        m.step(Event::Tick(loud), T0.plus(10 * SECOND));
        assert_eq!(
            busy(&m),
            Some(BusyNow {
                active: true,
                detail: "CPU at 72% and network at 2 KB/s".into(),
                quiet_secs: 0,
                release_in_secs: None
            })
        );
        m.step(Event::Tick(idle_inputs()), T0.plus(50 * SECOND));
        assert_eq!(
            busy(&m),
            Some(BusyNow {
                active: false,
                detail: "quiet".into(),
                quiet_secs: 40,
                release_in_secs: Some(80)
            })
        );
        let unknown = Inputs {
            cpu_pct: None,
            net_bytes_per_s: Some(0.0),
            ..idle_inputs()
        };
        m.step(Event::Tick(unknown), T0.plus(60 * SECOND));
        assert!(matches!(busy(&m), Some(b) if b.active && b.detail == "CPU reading unavailable"));
        let mut other = holding(AwakeUntil::Indefinite, None);
        other.step(Event::Tick(idle_inputs()), T0.plus(SECOND));
        assert_eq!(busy(&other), None, "only a busy hold has a busy state");
    }

    #[test]
    fn each_busy_condition_reads_in_words() {
        let i = Inputs {
            cpu_pct: Some(10.0),
            net_bytes_per_s: Some(5000.0),
            disk_bytes_per_s: Some(3.0 * 1_048_576.0),
            programs: Some(vec!["Build.exe".into()]),
            ..idle_inputs()
        };
        assert_eq!(BusyCond::Cpu { above_pct: 50.0 }.reading(&i), None);
        assert_eq!(
            BusyCond::Network { above_bytes_per_s: 1000 }.reading(&i).as_deref(),
            Some("network at 5 KB/s")
        );
        assert_eq!(
            BusyCond::Disk { above_bytes_per_s: 1000 }.reading(&i).as_deref(),
            Some("disk at 3.0 MB/s")
        );
        assert_eq!(
            BusyCond::Process {
                program: "build.EXE".into()
            }
            .reading(&i)
            .as_deref(),
            Some("build.EXE is running")
        );
        assert_eq!(BusyCond::Process { program: "other".into() }.reading(&i), None);
        let none = Inputs {
            cpu_pct: None,
            net_bytes_per_s: None,
            disk_bytes_per_s: None,
            programs: None,
            ..idle_inputs()
        };
        for c in [
            BusyCond::Cpu { above_pct: 1.0 },
            BusyCond::Network { above_bytes_per_s: 1 },
            BusyCond::Disk { above_bytes_per_s: 1 },
            BusyCond::Process { program: "x".into() },
        ] {
            assert!(c.reading(&none).unwrap().contains("unavailable"));
        }
    }

    #[test]
    fn a_busy_hold_lets_go_only_after_every_condition_was_false_for_the_grace() {
        let until = AwakeUntil::WhileBusy {
            conds: vec![
                BusyCond::Cpu { above_pct: 50.0 },
                BusyCond::Process {
                    program: "Build.exe".into(),
                },
            ],
            grace_secs: 120,
        };
        let mut m = holding(until, None);
        let busy_cpu = Inputs {
            cpu_pct: Some(80.0),
            ..idle_inputs()
        };
        let busy_prog = Inputs {
            programs: Some(vec!["build.EXE".into()]),
            ..idle_inputs()
        };
        // Busy at +100 s restarts the grace.
        assert!(m.step(Event::Tick(busy_cpu), T0.plus(100 * SECOND)).is_empty());
        assert!(
            m.step(Event::Tick(idle_inputs()), T0.plus(219 * SECOND)).is_empty(),
            "119 s of quiet is not enough"
        );
        assert!(m.step(Event::Tick(busy_prog), T0.plus(220 * SECOND)).is_empty());
        assert!(m.step(Event::Tick(idle_inputs()), T0.plus(339 * SECOND)).is_empty());
        assert!(m.step(Event::Tick(idle_inputs()), T0.plus(340 * SECOND)).contains(&Effect::Release));
    }

    #[test]
    fn a_withheld_reading_counts_as_busy() {
        let until = AwakeUntil::WhileBusy {
            conds: vec![
                BusyCond::Network { above_bytes_per_s: 1000 },
                BusyCond::Disk { above_bytes_per_s: 1000 },
            ],
            grace_secs: 10,
        };
        let mut m = holding(until, None);
        let unknown = Inputs {
            net_bytes_per_s: None,
            disk_bytes_per_s: None,
            ..idle_inputs()
        };
        assert!(m.step(Event::Tick(unknown), T0.plus(100 * SECOND)).is_empty());
        let busy_disk = Inputs {
            disk_bytes_per_s: Some(5000.0),
            ..idle_inputs()
        };
        assert!(m.step(Event::Tick(busy_disk), T0.plus(200 * SECOND)).is_empty());
        let no_programs = AwakeUntil::WhileBusy {
            conds: vec![BusyCond::Process { program: "x".into() }, BusyCond::Cpu { above_pct: 1.0 }],
            grace_secs: 10,
        };
        let mut m = holding(no_programs, None);
        let unknown = Inputs {
            programs: None,
            cpu_pct: None,
            ..idle_inputs()
        };
        assert!(m.step(Event::Tick(unknown), T0.plus(100 * SECOND)).is_empty());
    }

    #[test]
    fn when_the_os_ends_the_hold_it_is_taken_again_on_wake() {
        let mut m = holding(AwakeUntil::Indefinite, None);
        let fx = m.step(
            Event::OsEnded {
                why: "the computer went to sleep".into(),
            },
            T0.plus(SECOND),
        );
        assert!(fx.contains(&Effect::Release));
        assert!(matches!(m.view(), AwakeView::Waiting { why, .. } if why == "the computer went to sleep"));
        assert!(m.step(Event::Tick(idle_inputs()), T0.plus(2 * SECOND)).is_empty());
        let fx = m.step(Event::Woke, T0.plus(3 * SECOND));
        assert!(fx.iter().any(|f| matches!(f, Effect::Acquire { .. })));
        m.step(Event::Acquired { os_ref: "again".into() }, T0.plus(3 * SECOND));
        assert!(matches!(m.view(), AwakeView::Holding { os_ref, .. } if os_ref == "again"));
    }

    #[test]
    fn a_deadline_that_passed_while_asleep_is_not_taken_again() {
        let mut m = holding(AwakeUntil::For { minutes: 1 }, Some(T0.plus(MINUTE)));
        m.step(Event::OsEnded { why: "sleep".into() }, T0.plus(SECOND));
        let fx = m.step(Event::Woke, T0.plus(2 * MINUTE));
        assert!(!fx.iter().any(|f| matches!(f, Effect::Acquire { .. })));
        assert_eq!(m.view(), AwakeView::Off);
        let mut m = holding(AwakeUntil::For { minutes: 1 }, Some(T0.plus(MINUTE)));
        m.step(Event::OsEnded { why: "sleep".into() }, T0.plus(SECOND));
        m.step(Event::Tick(idle_inputs()), T0.plus(2 * MINUTE));
        assert_eq!(m.view(), AwakeView::Off);
    }

    #[test]
    fn a_restart_is_shown_and_never_re_takes_the_hold() {
        let mut m = AwakeMachine::new();
        let fx = m.step(Event::Restarted, T0);
        assert!(!fx.iter().any(|f| matches!(f, Effect::Acquire { .. })));
        assert_eq!(
            m.view(),
            AwakeView::Ended {
                why: "Dyno restarted".into(),
                at: T0
            }
        );
        assert_eq!(m.step(Event::Stop, T0).len(), 0);
        assert_eq!(m.view(), AwakeView::Off, "Stop clears the notice");
    }

    #[test]
    fn events_that_do_not_apply_change_nothing() {
        let mut m = AwakeMachine::new();
        for e in [
            Event::Woke,
            Event::Tick(idle_inputs()),
            Event::OsEnded { why: "x".into() },
            Event::AcquireFailed { why: "x".into() },
        ] {
            assert!(m.step(e, T0).is_empty());
            assert_eq!(m.view(), AwakeView::Off);
        }
        // A late Acquired (the user stopped meanwhile) releases the stray hold.
        assert!(m.step(Event::Acquired { os_ref: "late".into() }, T0).contains(&Effect::Release));
        let mut m = holding(AwakeUntil::Indefinite, None);
        assert!(m.step(Event::Restarted, T0).is_empty());
        assert!(m.step(Event::Woke, T0).is_empty());
        assert!(m.step(Event::AcquireFailed { why: "x".into() }, T0).is_empty());
        assert!(matches!(m.view(), AwakeView::Holding { .. }));
    }

    /// Runs `ev` the way the agent does: an `Acquire` is answered at once (taken, or refused), a `Release` drops the hold.
    fn agent_step(m: &mut AwakeMachine, live: &mut bool, ev: Event, now: Timestamp, acquire_ok: bool) {
        let mut queue = std::collections::VecDeque::from([ev]);
        while let Some(ev) = queue.pop_front() {
            for fx in m.step(ev, now) {
                match fx {
                    Effect::Acquire { .. } if acquire_ok => {
                        *live = true;
                        queue.push_back(Event::Acquired { os_ref: "r".into() });
                    }
                    Effect::Acquire { .. } => queue.push_back(Event::AcquireFailed { why: "f".into() }),
                    Effect::Release => *live = false,
                    Effect::Log(_) => {}
                }
            }
        }
    }

    #[test]
    fn every_acquire_is_matched_by_a_release_or_an_os_end() {
        // Exhaustive over all event sequences of length 6 from a small alphabet, with the OS granting and with it refusing:
        // the OS hold is live exactly while the view says Holding, and never outlives an inactive machine.
        let alphabet = |i: usize| match i {
            0 => Event::Start {
                spec: spec(AwakeUntil::For { minutes: 1 }),
                ends: Some(T0.plus(3 * SECOND)),
            },
            1 => Event::Stop,
            2 => Event::OsEnded { why: "w".into() },
            3 => Event::Woke,
            4 => Event::Tick(idle_inputs()),
            _ => Event::Restarted,
        };
        let (n, len) = (6usize, 6usize);
        for acquire_ok in [true, false] {
            for code in 0..n.pow(len as u32) {
                let (mut m, mut live, mut c) = (AwakeMachine::new(), false, code);
                for step in 0..len {
                    agent_step(&mut m, &mut live, alphabet(c % n), T0.plus(step as i64 * SECOND), acquire_ok);
                    c /= n;
                    let holding = matches!(m.view(), AwakeView::Holding { .. });
                    assert_eq!(holding, live, "ok={acquire_ok} sequence {code} step {step}");
                    if !m.is_active() {
                        assert!(!live, "a hold outlived the machine: ok={acquire_ok} sequence {code} step {step}");
                    }
                }
            }
        }
    }

    #[test]
    fn validation_says_what_to_fix() {
        let ok = |u| spec(u).validate().is_ok();
        let err = |u| spec(u).validate().unwrap_err();
        assert!(ok(AwakeUntil::Indefinite) && ok(AwakeUntil::For { minutes: 30 }) && ok(AwakeUntil::At { minute_of_day: 1439 }));
        assert!(ok(AwakeUntil::WhileProcess {
            process: ProcessRef { pid: 1, started: None }
        }));
        assert!(err(AwakeUntil::For { minutes: 0 }).contains("1 minute"));
        assert!(err(AwakeUntil::For {
            minutes: MAX_FOR_MINUTES + 1
        })
        .contains("7 days"));
        assert!(err(AwakeUntil::At { minute_of_day: 1440 }).contains("00:00"));
        let busy = |conds, grace_secs| AwakeUntil::WhileBusy { conds, grace_secs };
        let cpu = || BusyCond::Cpu { above_pct: 10.0 };
        assert!(ok(busy(vec![cpu()], 120)));
        assert!(err(busy(vec![], 120)).contains("conditions"));
        assert!(err(busy(vec![cpu(); MAX_CONDITIONS + 1], 120)).contains("conditions"));
        assert!(err(busy(vec![cpu()], 5)).contains("seconds"));
        assert!(err(busy(vec![BusyCond::Cpu { above_pct: 101.0 }], 120)).contains("percentage"));
        assert!(err(busy(vec![BusyCond::Cpu { above_pct: f32::NAN }], 120)).contains("percentage"));
        assert!(err(busy(vec![BusyCond::Network { above_bytes_per_s: 0 }], 120)).contains("rate"));
        assert!(err(busy(vec![BusyCond::Disk { above_bytes_per_s: 0 }], 120)).contains("rate"));
        assert!(err(busy(vec![BusyCond::Process { program: "  ".into() }], 120)).contains("program"));
        assert!(err(busy(
            vec![BusyCond::Process {
                program: "x".repeat(MAX_PROGRAM_CHARS + 1)
            }],
            120
        ))
        .contains("program"));
        assert!(ok(busy(
            vec![
                BusyCond::Process { program: "a.exe".into() },
                BusyCond::Disk { above_bytes_per_s: 1 }
            ],
            10
        )));
    }

    #[test]
    fn deadlines_follow_the_local_clock() {
        let zone = FixedZone(480);
        // 2023-11-14 22:13:20 UTC is 06:13:20 local the next day.
        let now = T0;
        let f = spec(AwakeUntil::For { minutes: 90 });
        assert_eq!(f.deadline(now, &zone), Some(now.plus(90 * MINUTE)));
        let at = |m| spec(AwakeUntil::At { minute_of_day: m }).deadline(now, &zone).unwrap();
        // 07:00 local is later today, 06:00 local is tomorrow.
        assert_eq!(at(7 * 60).since(now), 46 * MINUTE + 40 * SECOND);
        assert_eq!(at(6 * 60).since(now), 23 * HOUR + 46 * MINUTE + 40 * SECOND);
        assert_eq!(spec(AwakeUntil::Indefinite).deadline(now, &zone), None);
    }

    #[test]
    fn reasons_name_dyno_and_the_request() {
        let r = |u| spec(u).reason();
        assert_eq!(r(AwakeUntil::Indefinite), "keeping awake until you stop it, you asked");
        assert_eq!(r(AwakeUntil::For { minutes: 45 }), "keeping awake for 45 min, you asked");
        assert_eq!(r(AwakeUntil::For { minutes: 90 }), "keeping awake for 1 h 30 min, you asked");
        assert_eq!(r(AwakeUntil::For { minutes: 2880 }), "keeping awake for 2 d, you asked");
        assert_eq!(r(AwakeUntil::At { minute_of_day: 605 }), "keeping awake until 10:05, you asked");
        assert!(r(AwakeUntil::WhileProcess {
            process: ProcessRef { pid: 1, started: None }
        })
        .contains("program you chose"));
        assert!(r(AwakeUntil::WhileBusy {
            conds: vec![],
            grace_secs: 10
        })
        .contains("busy"));
    }

    #[test]
    fn the_wire_uses_camel_case_and_tags() {
        let v = AwakeView::Holding {
            spec: spec(AwakeUntil::For { minutes: 5 }),
            since: T0,
            ends: None,
            os_ref: "r".into(),
            busy: None,
        };
        let j = serde_json::to_value(&v).unwrap();
        assert_eq!(j["holding"]["osRef"], "r");
        assert_eq!(j["holding"]["spec"]["until"]["for"]["minutes"], 5);
        let back: AwakeView = serde_json::from_value(j).unwrap();
        assert_eq!(back, v);
        let c = serde_json::to_value(BusyCond::Network { above_bytes_per_s: 7 }).unwrap();
        assert_eq!(c, serde_json::json!({"network": {"aboveBytesPerS": 7}}));
    }
}
