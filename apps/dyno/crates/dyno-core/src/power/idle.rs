//! Idle and timed actions (ADR 0032 section 4, 8 and 10), pure. A sleep, hibernate or shut down that the person asked for in
//! advance: after the computer has been truly idle for a while, or at a time of day. Off by default, never forced (the OS's
//! normal path; an app with unsaved work may stop a shutdown), and every decision is a typed value the agent logs.
//!
//! The rules that make a wrong action unlikely: **unknown is busy** (an input the OS cannot give blocks the action, and the
//! page says which); a **countdown** with Cancel before anything happens; at zero **every input is read once more** and the
//! action runs only if all still pass; **one attempt per idle period** (after a cancel, an action or a failure the rule
//! re-arms only after new activity); `ac_only` is re-checked at zero.

use crate::metrics::Reading;
use crate::time::{local_epoch_day, local_minute, LocalZone, Timestamp, MINUTE, SECOND};
use serde::{Deserialize, Serialize};

/// The shortest countdown, in seconds.
pub const MIN_COUNTDOWN_SECS: u32 = 30;
/// The longest countdown, in seconds.
pub const MAX_COUNTDOWN_SECS: u32 = 600;
/// How long after asking the agent waits before it says "it did not happen".
pub const ACT_WAIT_SECS: i64 = 120;
/// At most this many timed actions.
pub const MAX_TIMED: usize = 16;

/// What to do to the computer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IdleAction {
    /// Sleep.
    Sleep,
    /// Hibernate.
    Hibernate,
    /// Shut down, the normal way (apps may object).
    ShutDown,
}

impl IdleAction {
    /// The verb for messages ("sleep", "hibernate", "shut down").
    pub fn verb(self) -> &'static str {
        match self {
            IdleAction::Sleep => "sleep",
            IdleAction::Hibernate => "hibernate",
            IdleAction::ShutDown => "shut down",
        }
    }
}

/// What keeps the computer from acting. Each is a switch; the ones with a limit are off when the limit is `None`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Exceptions {
    /// Audio is playing.
    pub audio: bool,
    /// A full-screen app or a presentation is showing.
    pub full_screen: bool,
    /// Another program asked the computer to stay awake (power assertions, power requests, inhibitors).
    pub other_stay_awake: bool,
    /// CPU above this percentage.
    pub cpu_pct: Option<f32>,
    /// Disk above this many bytes per second.
    pub disk_bytes_per_s: Option<u64>,
    /// Network above this many bytes per second.
    pub net_bytes_per_s: Option<u64>,
    /// A program with one of these names is running (empty: off).
    pub programs: Vec<String>,
}

impl Default for Exceptions {
    fn default() -> Self {
        Self {
            audio: true,
            full_screen: true,
            other_stay_awake: true,
            cpu_pct: Some(50.0),
            disk_bytes_per_s: Some(5 * 1024 * 1024),
            net_bytes_per_s: Some(500 * 1024),
            programs: Vec::new(),
        }
    }
}

/// The idle rule (ADR 0032 section 9, `power.idle.*`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct IdleSettings {
    /// On: acts on the machine, so off by default.
    pub enabled: bool,
    /// What to do.
    pub action: IdleAction,
    /// How long the computer must be idle first, in seconds.
    pub after_secs: u32,
    /// The warning before acting, in seconds.
    pub countdown_secs: u32,
    /// Only while plugged in (re-checked at zero).
    pub ac_only: bool,
    /// What blocks it.
    pub exceptions: Exceptions,
}

impl Default for IdleSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            action: IdleAction::Sleep,
            after_secs: 30 * 60,
            countdown_secs: 60,
            ac_only: true,
            exceptions: Exceptions::default(),
        }
    }
}

impl IdleSettings {
    /// Every value in range.
    pub fn normalised(mut self) -> Self {
        self.after_secs = self.after_secs.clamp(60, 24 * 3600);
        self.countdown_secs = self.countdown_secs.clamp(MIN_COUNTDOWN_SECS, MAX_COUNTDOWN_SECS);
        let e = &mut self.exceptions;
        e.cpu_pct = e.cpu_pct.filter(|v| v.is_finite()).map(|v| v.clamp(1.0, 100.0));
        e.disk_bytes_per_s = e.disk_bytes_per_s.map(|v| v.clamp(1024, 10 << 30));
        e.net_bytes_per_s = e.net_bytes_per_s.map(|v| v.clamp(1024, 10 << 30));
        e.programs = e
            .programs
            .iter()
            .map(|p| p.trim().chars().take(256).collect::<String>())
            .filter(|p| !p.is_empty())
            .take(16)
            .collect();
        self
    }
}

/// How often a timed action repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Repeat {
    /// Once, then it is done.
    Once,
    /// Every day.
    Daily,
    /// On these days: bit 0 Monday to bit 6 Sunday.
    Weekdays(u8),
}

/// An action at a time of day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimedAction {
    /// Identifies it for Cancel.
    pub id: u32,
    /// What to do.
    pub action: IdleAction,
    /// When, as the minute of the local day (0 to 1439).
    pub at_minute: u16,
    /// How often.
    pub repeat: Repeat,
    /// The local day it last ran (so a repeated hour in a DST fold runs once, and a skipped one runs at the first minute after).
    #[serde(default)]
    pub last_day: Option<i64>,
    /// Set after a `Once` entry ran.
    #[serde(default)]
    pub done: bool,
}

/// The timed actions (`power.timed.*`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TimedSettings {
    /// On: acts on the machine, so off by default.
    pub enabled: bool,
    /// The actions.
    pub entries: Vec<TimedAction>,
}

impl TimedSettings {
    /// Validation with a message that says what to fix.
    pub fn validate(&self) -> Result<(), String> {
        if self.entries.len() > MAX_TIMED {
            return Err(format!("At most {MAX_TIMED} timed actions."));
        }
        for e in &self.entries {
            if e.at_minute >= 1440 {
                return Err("Pick a time of day between 00:00 and 23:59.".into());
            }
            if let Repeat::Weekdays(mask) = e.repeat {
                if mask & 0x7f == 0 || mask > 0x7f {
                    return Err("Pick at least one day of the week.".into());
                }
            }
        }
        let mut ids: Vec<u32> = self.entries.iter().map(|e| e.id).collect();
        ids.sort_unstable();
        ids.dedup();
        if ids.len() != self.entries.len() {
            return Err("Every timed action needs its own id.".into());
        }
        Ok(())
    }

    /// The entries due at `now` (their ids and actions), in order. An entry is due once per local day when the clock has reached
    /// its minute: a time that does not exist (a DST gap) is due at the first minute after it, a repeated one runs once.
    pub fn due(&self, now: Timestamp, zone: &dyn LocalZone) -> Vec<(u32, IdleAction)> {
        if !self.enabled {
            return Vec::new();
        }
        let day = local_epoch_day(now, zone);
        let minute = local_minute(now, zone);
        // 1970-01-01 was a Thursday; bit 0 is Monday.
        let weekday = (day + 3).rem_euclid(7) as u8;
        self.entries
            .iter()
            .filter(|e| !e.done && e.last_day != Some(day) && minute >= e.at_minute)
            .filter(|e| match e.repeat {
                Repeat::Once | Repeat::Daily => true,
                Repeat::Weekdays(mask) => mask & (1 << weekday) != 0,
            })
            .map(|e| (e.id, e.action))
            .collect()
    }

    /// Notes that entry `id` ran at `now` (a `Once` entry is done; the others will not run again today).
    pub fn mark_run(&mut self, id: u32, now: Timestamp, zone: &dyn LocalZone) {
        let day = local_epoch_day(now, zone);
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.last_day = Some(day);
            e.done = e.repeat == Repeat::Once;
        }
    }
}

/// What blocks, in words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BlockerKind {
    /// Audio is playing.
    Audio,
    /// A full-screen app or presentation.
    FullScreen,
    /// Another program asked the computer to stay awake.
    StayAwake,
    /// CPU busy.
    Cpu,
    /// Disk busy.
    Disk,
    /// Network busy.
    Network,
    /// A program the person named is running.
    Program,
    /// On battery while the rule says plugged in only.
    Battery,
}

/// One reason the action waits: either it holds, or Dyno cannot tell (which blocks too, and says why).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Blocker {
    /// What.
    pub kind: BlockerKind,
    /// The words for the page and the log (no user content beyond a program or app name the person chose or the OS reports).
    pub reason: String,
    /// True when this is "Dyno cannot tell", false when it is a reading.
    pub cant_tell: bool,
}

/// What the machine reads at each tick. `Unavailable` is "Dyno cannot tell" with the OS's reason.
#[derive(Debug, Clone, PartialEq)]
pub struct IdleInputs {
    /// Seconds since the last keyboard or mouse input.
    pub idle_secs: Reading<u64>,
    /// Whether the computer runs on battery (no battery reads as plugged in).
    pub on_battery: Reading<bool>,
    /// Whether audio is playing.
    pub audio: Reading<bool>,
    /// Whether a full-screen app or presentation shows.
    pub full_screen: Reading<bool>,
    /// The names of the other programs that asked the computer to stay awake.
    pub stay_awake: Reading<Vec<String>>,
    /// CPU load, 0 to 100.
    pub cpu_pct: Reading<f32>,
    /// Disk bytes per second.
    pub disk_bps: Reading<f64>,
    /// Network bytes per second.
    pub net_bps: Reading<f64>,
    /// The running programs' file names.
    pub programs: Reading<Vec<String>>,
}

fn cant(kind: BlockerKind, what: &str, reason: &str) -> Blocker {
    Blocker {
        kind,
        reason: format!("Dyno cannot tell whether {what}: {reason}"),
        cant_tell: true,
    }
}

fn holds(kind: BlockerKind, reason: String) -> Blocker {
    Blocker {
        kind,
        reason,
        cant_tell: false,
    }
}

/// Everything that keeps the idle action from running now, for the rule `s`. Empty means every input passes and is known.
pub fn blockers(s: &IdleSettings, i: &IdleInputs) -> Vec<Blocker> {
    let e = &s.exceptions;
    let mut out = Vec::new();
    if s.ac_only {
        match &i.on_battery {
            Reading::Available(true) => out.push(holds(BlockerKind::Battery, "the computer is on battery".into())),
            Reading::Available(false) => {}
            Reading::Unavailable(r) => out.push(cant(BlockerKind::Battery, "it is on battery", r)),
        }
    }
    let flag = |on: bool, kind: BlockerKind, r: &Reading<bool>, yes: &str, what: &str, out: &mut Vec<Blocker>| {
        if on {
            match r {
                Reading::Available(true) => out.push(holds(kind, yes.into())),
                Reading::Available(false) => {}
                Reading::Unavailable(why) => out.push(cant(kind, what, why)),
            }
        }
    };
    flag(
        e.audio,
        BlockerKind::Audio,
        &i.audio,
        "audio is playing",
        "audio is playing",
        &mut out,
    );
    flag(
        e.full_screen,
        BlockerKind::FullScreen,
        &i.full_screen,
        "a full-screen app or presentation is showing",
        "a full-screen app is showing",
        &mut out,
    );
    if e.other_stay_awake {
        match &i.stay_awake {
            Reading::Available(names) if !names.is_empty() => {
                out.push(holds(
                    BlockerKind::StayAwake,
                    format!("{} asked the computer to stay awake", names.join(", ")),
                ));
            }
            Reading::Available(_) => {}
            Reading::Unavailable(why) => out.push(cant(BlockerKind::StayAwake, "another program asked it to stay awake", why)),
        }
    }
    if let Some(limit) = e.cpu_pct {
        match &i.cpu_pct {
            Reading::Available(v) if *v > limit => {
                out.push(holds(BlockerKind::Cpu, format!("the CPU is busy ({v:.0}%, limit {limit:.0}%)")))
            }
            Reading::Available(_) => {}
            Reading::Unavailable(why) => out.push(cant(BlockerKind::Cpu, "the CPU is busy", why)),
        }
    }
    if let Some(limit) = e.disk_bytes_per_s {
        match &i.disk_bps {
            Reading::Available(v) if *v > limit as f64 => out.push(holds(BlockerKind::Disk, "the disk is busy".into())),
            Reading::Available(_) => {}
            Reading::Unavailable(why) => out.push(cant(BlockerKind::Disk, "the disk is busy", why)),
        }
    }
    if let Some(limit) = e.net_bytes_per_s {
        match &i.net_bps {
            Reading::Available(v) if *v > limit as f64 => out.push(holds(BlockerKind::Network, "the network is busy".into())),
            Reading::Available(_) => {}
            Reading::Unavailable(why) => out.push(cant(BlockerKind::Network, "the network is busy", why)),
        }
    }
    if !e.programs.is_empty() {
        match &i.programs {
            Reading::Available(running) => {
                if let Some(p) = e.programs.iter().find(|p| running.iter().any(|r| r.eq_ignore_ascii_case(p))) {
                    out.push(holds(BlockerKind::Program, format!("{p} is running")));
                }
            }
            Reading::Unavailable(why) => out.push(cant(BlockerKind::Program, "a program you named is running", why)),
        }
    }
    out
}

/// Why a countdown started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Cause {
    /// The computer was idle long enough.
    Idle,
    /// A timed action's time came.
    Timed,
}

/// What the page shows.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum IdleView {
    /// Neither rule is on.
    #[default]
    Disabled,
    /// The OS gives no idle time here, so the idle rule cannot work; the reason is the OS's.
    Unavailable {
        /// Why.
        reason: String,
    },
    /// Watching: how long it has been idle and what blocks now.
    Watching {
        /// Seconds idle now (0 when unknown).
        idle_secs: u64,
        /// What keeps it from acting.
        blockers: Vec<Blocker>,
        /// Snoozed until, when it is.
        snoozed_until: Option<Timestamp>,
    },
    /// The warning before acting.
    Countdown {
        /// What it will do.
        action: IdleAction,
        /// When.
        deadline: Timestamp,
        /// Why.
        cause: Cause,
    },
    /// Asked the system; waiting for it (no timeout, rule 12).
    Acting {
        /// What was asked.
        action: IdleAction,
        /// Since when.
        since: Timestamp,
    },
    /// It ended without a retry; the rule re-arms after new activity.
    Stopped {
        /// Why, in plain words.
        why: String,
    },
}

/// Something that happened.
#[derive(Debug, Clone, PartialEq)]
pub enum IdleEvent {
    /// A reading round.
    Tick(Box<IdleInputs>),
    /// The settings changed (`timed_on`: the timed actions are on).
    Configure {
        /// The idle rule.
        idle: Box<IdleSettings>,
        /// Whether timed actions are on.
        timed_on: bool,
    },
    /// The person cancelled (tray, dialog, page, CLI).
    Cancel,
    /// The person snoozed the countdown.
    Snooze {
        /// Minutes.
        minutes: u32,
    },
    /// A timed action's time came.
    TimedDue {
        /// What to do.
        action: IdleAction,
        /// The countdown, in seconds.
        countdown_secs: u32,
    },
    /// The system did not take the request.
    ActFailed {
        /// Why.
        why: String,
    },
}

/// What the agent must do.
#[derive(Debug, Clone, PartialEq)]
pub enum IdleEffect {
    /// Show the countdown (notification, dialog, tray header) until `deadline`.
    ShowCountdown {
        /// What will happen.
        action: IdleAction,
        /// When.
        deadline: Timestamp,
    },
    /// Remove the countdown.
    HideCountdown,
    /// Ask the OS, the normal way, now.
    Act {
        /// What.
        action: IdleAction,
    },
    /// Write one decision to the log.
    Log(String),
}

#[derive(Debug, Clone, PartialEq)]
enum State {
    Off,
    Watching {
        armed: bool,
        last_idle: u64,
        snooze_until: Option<Timestamp>,
        /// A timed action whose countdown was snoozed: its countdown shows again when the snooze ends.
        timed_resume: Option<IdleAction>,
        last_blockers: Vec<Blocker>,
        unavailable: Option<String>,
    },
    Countdown {
        action: IdleAction,
        deadline: Timestamp,
        cause: Cause,
        last_idle: u64,
    },
    Acting {
        action: IdleAction,
        since: Timestamp,
        last_idle: u64,
    },
    Stopped {
        why: String,
        last_idle: u64,
    },
}

/// The idle and timed state machine. Owns no clock and calls nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct IdleMachine {
    settings: IdleSettings,
    timed_on: bool,
    state: State,
}

impl Default for IdleMachine {
    fn default() -> Self {
        Self {
            settings: IdleSettings::default(),
            timed_on: false,
            state: State::Off,
        }
    }
}

fn watching(armed: bool, last_idle: u64) -> State {
    State::Watching {
        armed,
        last_idle,
        snooze_until: None,
        timed_resume: None,
        last_blockers: Vec::new(),
        unavailable: None,
    }
}

impl IdleMachine {
    /// An idle machine with both rules off.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the agent should keep reading inputs.
    pub fn is_on(&self) -> bool {
        !matches!(self.state, State::Off)
    }

    /// What the page shows.
    pub fn view(&self) -> IdleView {
        match &self.state {
            State::Off => IdleView::Disabled,
            // The idle rule is off (only timed actions are on): the card never reports a rule that is off.
            State::Watching { timed_resume: None, .. } if !self.settings.enabled => IdleView::Disabled,
            State::Watching {
                unavailable: Some(reason), ..
            } => IdleView::Unavailable { reason: reason.clone() },
            State::Watching {
                last_idle,
                snooze_until,
                last_blockers,
                ..
            } => IdleView::Watching {
                idle_secs: *last_idle,
                blockers: last_blockers.clone(),
                snoozed_until: *snooze_until,
            },
            State::Countdown {
                action, deadline, cause, ..
            } => IdleView::Countdown {
                action: *action,
                deadline: *deadline,
                cause: *cause,
            },
            State::Acting { action, since, .. } => IdleView::Acting {
                action: *action,
                since: *since,
            },
            State::Stopped { why, .. } => IdleView::Stopped { why: why.clone() },
        }
    }

    /// When the machine next needs a tick on its own (the countdown's end).
    pub fn next_deadline(&self) -> Option<Timestamp> {
        match &self.state {
            State::Countdown { deadline, .. } => Some(*deadline),
            _ => None,
        }
    }

    /// The idle rule as configured.
    pub fn settings(&self) -> &IdleSettings {
        &self.settings
    }

    /// Applies one event at `now`.
    pub fn step(&mut self, event: IdleEvent, now: Timestamp) -> Vec<IdleEffect> {
        let state = std::mem::replace(&mut self.state, State::Off);
        let (next, fx) = self.transition(state, event, now);
        self.state = next;
        fx
    }

    fn transition(&mut self, state: State, event: IdleEvent, now: Timestamp) -> (State, Vec<IdleEffect>) {
        use IdleEffect::{HideCountdown, Log};
        let log = Log;
        match (state, event) {
            (state, IdleEvent::Configure { idle, timed_on }) => {
                self.settings = idle.normalised();
                self.timed_on = timed_on;
                let on = self.settings.enabled || timed_on;
                match (on, state) {
                    (false, State::Countdown { .. }) => (
                        State::Off,
                        vec![HideCountdown, log("idle and timed actions turned off; countdown removed".into())],
                    ),
                    (false, _) => (State::Off, vec![]),
                    (true, State::Off) => (watching(true, 0), vec![log("idle and timed actions turned on".into())]),
                    (true, s) => (s, vec![]),
                }
            }
            (State::Off, _) => (State::Off, vec![]),
            (State::Watching { last_idle, .. }, IdleEvent::Cancel) => (
                watching(false, last_idle),
                vec![log(
                    "this idle period skipped by the person; no action until there is new activity".into()
                )],
            ),
            (State::Countdown { last_idle, .. }, IdleEvent::Cancel) => (
                watching(false, last_idle),
                vec![
                    HideCountdown,
                    log("countdown cancelled by the person; no new attempt until there is new activity".into()),
                ],
            ),
            (State::Acting { last_idle, .. }, IdleEvent::Cancel) => (
                watching(false, last_idle),
                vec![log(
                    "cancel after the request: the system may still go on; waiting for activity".into()
                )],
            ),
            (State::Stopped { last_idle, .. }, IdleEvent::Cancel) => (watching(false, last_idle), vec![]),
            (
                State::Countdown {
                    last_idle, action, cause, ..
                },
                IdleEvent::Snooze { minutes },
            ) => {
                let until = now.plus(i64::from(minutes.clamp(1, 24 * 60)) * MINUTE);
                // A timed action was already marked as run when its countdown began; the snooze delays it, so it comes back.
                let timed_resume = (cause == Cause::Timed).then_some(action);
                (
                    State::Watching {
                        armed: true,
                        last_idle,
                        snooze_until: Some(until),
                        timed_resume,
                        last_blockers: Vec::new(),
                        unavailable: None,
                    },
                    vec![HideCountdown, log(format!("countdown snoozed for {minutes} min"))],
                )
            }
            (s, IdleEvent::Snooze { .. }) => (s, vec![]),
            // Another timed action is due while one is snoozed: it stays due until this one has had its turn.
            (s @ State::Watching { timed_resume: Some(_), .. }, IdleEvent::TimedDue { .. }) => (s, vec![]),
            (State::Watching { last_idle, .. }, IdleEvent::TimedDue { action, countdown_secs })
            | (State::Stopped { last_idle, .. }, IdleEvent::TimedDue { action, countdown_secs }) => {
                let deadline = now.plus(i64::from(countdown_secs.clamp(MIN_COUNTDOWN_SECS, MAX_COUNTDOWN_SECS)) * SECOND);
                (
                    State::Countdown {
                        action,
                        deadline,
                        cause: Cause::Timed,
                        last_idle,
                    },
                    vec![
                        IdleEffect::ShowCountdown { action, deadline },
                        log(format!("timed action due: will {} unless cancelled", action.verb())),
                    ],
                )
            }
            (s, IdleEvent::TimedDue { .. }) => (s, vec![]),
            (State::Acting { action, last_idle, .. }, IdleEvent::ActFailed { why }) => (
                State::Stopped {
                    why: why.clone(),
                    last_idle,
                },
                vec![log(format!("the system did not take the {} request: {why}", action.verb()))],
            ),
            (s, IdleEvent::ActFailed { .. }) => (s, vec![]),
            (
                State::Watching {
                    armed,
                    last_idle,
                    snooze_until,
                    timed_resume,
                    ..
                },
                IdleEvent::Tick(i),
            ) => self.tick_watching(armed, last_idle, snooze_until, timed_resume, &i, now),
            (
                State::Countdown {
                    action,
                    deadline,
                    cause,
                    last_idle,
                },
                IdleEvent::Tick(i),
            ) => self.tick_countdown(action, deadline, cause, last_idle, &i, now),
            (State::Acting { action, since, last_idle }, IdleEvent::Tick(i)) => {
                let idle = i.idle_secs.value().copied().unwrap_or(last_idle);
                if idle < last_idle {
                    (
                        State::Stopped {
                            why: format!("the computer was used again after the {} request", action.verb()),
                            last_idle: idle,
                        },
                        vec![log("resumed or used again after the request".into())],
                    )
                } else if now.since(since) >= ACT_WAIT_SECS * SECOND {
                    let why = match action {
                        IdleAction::ShutDown => "Shutdown did not happen: an app may be asking you to save".to_string(),
                        a => format!("The computer did not {}", a.verb()),
                    };
                    (
                        State::Stopped {
                            why: why.clone(),
                            last_idle: idle,
                        },
                        vec![log(format!("{why}; no retry in this idle period"))],
                    )
                } else {
                    (
                        State::Acting {
                            action,
                            since,
                            last_idle: idle,
                        },
                        vec![],
                    )
                }
            }
            (State::Stopped { why, last_idle }, IdleEvent::Tick(i)) => {
                let idle = i.idle_secs.value().copied().unwrap_or(last_idle);
                if idle < last_idle {
                    (watching(true, idle), vec![log("new activity: watching again".into())])
                } else {
                    (State::Stopped { why, last_idle: idle }, vec![])
                }
            }
        }
    }

    fn tick_watching(
        &self,
        armed: bool,
        last_idle: u64,
        snooze_until: Option<Timestamp>,
        timed_resume: Option<IdleAction>,
        i: &IdleInputs,
        now: Timestamp,
    ) -> (State, Vec<IdleEffect>) {
        let s = &self.settings;
        let (idle, unavailable) = match &i.idle_secs {
            Reading::Available(v) => (*v, None),
            Reading::Unavailable(r) => (last_idle, Some(r.clone())),
        };
        let armed = armed || idle < last_idle;
        let snooze_until = snooze_until.filter(|t| *t > now);
        // The snooze of a timed action is over: its countdown shows again (the timed rule does not look at idle time or blockers).
        if let (None, Some(action)) = (snooze_until, timed_resume) {
            let deadline = now.plus(i64::from(s.countdown_secs.clamp(MIN_COUNTDOWN_SECS, MAX_COUNTDOWN_SECS)) * SECOND);
            return (
                State::Countdown {
                    action,
                    deadline,
                    cause: Cause::Timed,
                    last_idle: idle,
                },
                vec![
                    IdleEffect::ShowCountdown { action, deadline },
                    IdleEffect::Log(format!("snooze over: the timed action will {} unless cancelled", action.verb())),
                ],
            );
        }
        let blockers = if s.enabled { blockers(s, i) } else { Vec::new() };
        let ready =
            s.enabled && unavailable.is_none() && armed && snooze_until.is_none() && idle >= u64::from(s.after_secs) && blockers.is_empty();
        if ready {
            let deadline = now.plus(i64::from(s.countdown_secs) * SECOND);
            let fx = vec![
                IdleEffect::ShowCountdown {
                    action: s.action,
                    deadline,
                },
                IdleEffect::Log(format!(
                    "idle for {idle} s (rule {} s) and nothing blocks: will {} in {} s unless cancelled",
                    s.after_secs,
                    s.action.verb(),
                    s.countdown_secs
                )),
            ];
            return (
                State::Countdown {
                    action: s.action,
                    deadline,
                    cause: Cause::Idle,
                    last_idle: idle,
                },
                fx,
            );
        }
        let state = State::Watching {
            armed,
            last_idle: idle,
            snooze_until,
            timed_resume,
            last_blockers: if idle >= u64::from(s.after_secs) { blockers } else { Vec::new() },
            unavailable,
        };
        (state, vec![])
    }

    fn tick_countdown(
        &self,
        action: IdleAction,
        deadline: Timestamp,
        cause: Cause,
        last_idle: u64,
        i: &IdleInputs,
        now: Timestamp,
    ) -> (State, Vec<IdleEffect>) {
        use IdleEffect::{HideCountdown, Log};
        let s = &self.settings;
        let idle = i.idle_secs.value().copied().unwrap_or(last_idle);
        let stay = |idle: u64| {
            (
                State::Countdown {
                    action,
                    deadline,
                    cause,
                    last_idle: idle,
                },
                Vec::new(),
            )
        };
        match cause {
            Cause::Idle => {
                let blockers = blockers(s, i);
                let active = i.idle_secs.value().is_some_and(|v| *v < u64::from(s.after_secs));
                if active || !blockers.is_empty() {
                    let why = if active {
                        "there was new activity".to_string()
                    } else {
                        blockers.iter().map(|b| b.reason.clone()).collect::<Vec<_>>().join("; ")
                    };
                    return (
                        watching(true, idle),
                        vec![HideCountdown, Log(format!("countdown cancelled: {why}"))],
                    );
                }
                if now >= deadline {
                    return (
                        State::Acting {
                            action,
                            since: now,
                            last_idle: idle,
                        },
                        vec![
                            Log(format!(
                                "countdown over; every input re-read and passes: asking the system to {} the normal way",
                                action.verb()
                            )),
                            IdleEffect::Act { action },
                        ],
                    );
                }
                stay(idle)
            }
            Cause::Timed => {
                if now >= deadline {
                    return (
                        State::Acting {
                            action,
                            since: now,
                            last_idle: idle,
                        },
                        vec![
                            Log(format!(
                                "timed countdown over: asking the system to {} the normal way",
                                action.verb()
                            )),
                            IdleEffect::Act { action },
                        ],
                    );
                }
                stay(idle)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::{FixedZone, DAY, HOUR};
    use Reading::{Available, Unavailable};

    const T0: Timestamp = Timestamp(1_700_000_000_000);

    fn inputs(idle: u64) -> IdleInputs {
        IdleInputs {
            idle_secs: Available(idle),
            on_battery: Available(false),
            audio: Available(false),
            full_screen: Available(false),
            stay_awake: Available(vec![]),
            cpu_pct: Available(1.0),
            disk_bps: Available(0.0),
            net_bps: Available(0.0),
            programs: Available(vec![]),
        }
    }

    fn rule() -> IdleSettings {
        IdleSettings {
            enabled: true,
            after_secs: 600,
            countdown_secs: 60,
            ..Default::default()
        }
    }

    fn machine(s: IdleSettings, timed: bool) -> IdleMachine {
        let mut m = IdleMachine::new();
        m.step(
            IdleEvent::Configure {
                idle: Box::new(s),
                timed_on: timed,
            },
            T0,
        );
        m
    }

    fn tick(m: &mut IdleMachine, i: IdleInputs, at: i64) -> Vec<IdleEffect> {
        m.step(IdleEvent::Tick(Box::new(i)), T0.plus(at * SECOND))
    }

    fn acts(fx: &[IdleEffect]) -> usize {
        fx.iter().filter(|f| matches!(f, IdleEffect::Act { .. })).count()
    }

    #[test]
    fn it_is_off_until_configured_and_off_again_removes_the_countdown() {
        let mut m = IdleMachine::new();
        assert_eq!(m.view(), IdleView::Disabled);
        assert!(tick(&mut m, inputs(9999), 0).is_empty());
        let mut m = machine(rule(), false);
        assert!(m.is_on());
        tick(&mut m, inputs(700), 1);
        assert!(matches!(m.view(), IdleView::Countdown { .. }));
        let fx = m.step(
            IdleEvent::Configure {
                idle: Box::new(IdleSettings::default()),
                timed_on: false,
            },
            T0.plus(2 * SECOND),
        );
        assert!(fx.contains(&IdleEffect::HideCountdown));
        assert_eq!(m.view(), IdleView::Disabled);
        assert!(!m.is_on());
        let mut off = machine(IdleSettings::default(), false);
        assert!(off
            .step(
                IdleEvent::Configure {
                    idle: Box::new(IdleSettings::default()),
                    timed_on: false
                },
                T0
            )
            .is_empty());
    }

    #[test]
    fn idle_long_enough_with_nothing_blocking_starts_the_countdown_and_acts_only_at_zero_after_a_fresh_read() {
        let mut m = machine(rule(), false);
        assert!(tick(&mut m, inputs(599), 0).is_empty());
        let fx = tick(&mut m, inputs(600), 1);
        assert!(matches!(fx[0], IdleEffect::ShowCountdown { action: IdleAction::Sleep, deadline } if deadline == T0.plus(61 * SECOND)));
        assert_eq!(m.next_deadline(), Some(T0.plus(61 * SECOND)));
        assert_eq!(acts(&tick(&mut m, inputs(630), 31)), 0, "not before zero");
        let fx = tick(&mut m, inputs(660), 61);
        assert_eq!(acts(&fx), 1);
        assert!(matches!(
            m.view(),
            IdleView::Acting {
                action: IdleAction::Sleep,
                ..
            }
        ));
        assert_eq!(acts(&tick(&mut m, inputs(670), 62)), 0, "one attempt");
    }

    #[test]
    fn any_activity_or_blocker_during_the_countdown_cancels_it_and_a_blocker_at_zero_stops_the_action() {
        for change in 0..3 {
            let mut m = machine(rule(), false);
            tick(&mut m, inputs(700), 0);
            let mut i = inputs(710);
            match change {
                0 => i.idle_secs = Available(2),
                1 => i.audio = Available(true),
                _ => i.on_battery = Available(true),
            }
            let fx = tick(&mut m, i, 10);
            assert!(fx.contains(&IdleEffect::HideCountdown), "{change}");
            assert!(matches!(m.view(), IdleView::Watching { .. }));
        }
        // A blocker that appears exactly at zero: the fresh read refuses.
        let mut m = machine(rule(), false);
        tick(&mut m, inputs(700), 0);
        let mut i = inputs(760);
        i.stay_awake = Available(vec!["Zoom".into()]);
        assert_eq!(acts(&tick(&mut m, i, 60)), 0);
    }

    #[test]
    fn unknown_is_busy_and_the_page_says_which() {
        let mut m = machine(rule(), false);
        let mut i = inputs(900);
        i.audio = Unavailable("Windows gives no audio state to Dyno yet".into());
        assert!(tick(&mut m, i.clone(), 0).is_empty());
        let IdleView::Watching { blockers, .. } = m.view() else { panic!() };
        assert_eq!(blockers.len(), 1);
        assert!(
            blockers[0].cant_tell
                && blockers[0].reason.contains("audio is playing")
                && blockers[0].reason.contains("Windows gives no audio")
        );
        // Switching that exception off lets it through.
        let mut s = rule();
        s.exceptions.audio = false;
        let mut m = machine(s, false);
        assert!(!tick(&mut m, i, 0).is_empty());
    }

    #[test]
    fn each_exception_blocks_only_while_it_is_on_and_holds() {
        let mut s = rule();
        s.exceptions.programs = vec!["build.exe".into()];
        let base = |f: &dyn Fn(&mut IdleInputs)| {
            let mut i = inputs(900);
            f(&mut i);
            blockers(&s, &i).into_iter().map(|b| b.kind).collect::<Vec<_>>()
        };
        assert!(base(&|_| {}).is_empty());
        assert_eq!(base(&|i| i.audio = Available(true)), [BlockerKind::Audio]);
        assert_eq!(base(&|i| i.full_screen = Available(true)), [BlockerKind::FullScreen]);
        assert_eq!(
            base(&|i| i.stay_awake = Available(vec!["a".into(), "b".into()])),
            [BlockerKind::StayAwake]
        );
        assert_eq!(base(&|i| i.cpu_pct = Available(90.0)), [BlockerKind::Cpu]);
        assert_eq!(base(&|i| i.disk_bps = Available(1e9)), [BlockerKind::Disk]);
        assert_eq!(base(&|i| i.net_bps = Available(1e9)), [BlockerKind::Network]);
        assert_eq!(base(&|i| i.programs = Available(vec!["Build.EXE".into()])), [BlockerKind::Program]);
        assert_eq!(base(&|i| i.on_battery = Available(true)), [BlockerKind::Battery]);
        let mut off = rule();
        off.ac_only = false;
        off.exceptions = Exceptions {
            audio: false,
            full_screen: false,
            other_stay_awake: false,
            cpu_pct: None,
            disk_bytes_per_s: None,
            net_bytes_per_s: None,
            programs: vec![],
        };
        let mut i = inputs(900);
        i.on_battery = Available(true);
        i.audio = Available(true);
        i.cpu_pct = Unavailable("x".into());
        assert!(blockers(&off, &i).is_empty());
        // Every unreadable input blocks with its reason.
        let mut u = inputs(900);
        u.on_battery = Unavailable("a".into());
        u.full_screen = Unavailable("b".into());
        u.stay_awake = Unavailable("c".into());
        u.cpu_pct = Unavailable("d".into());
        u.disk_bps = Unavailable("e".into());
        u.net_bps = Unavailable("f".into());
        u.programs = Unavailable("g".into());
        assert_eq!(blockers(&s, &u).len(), 7);
    }

    #[test]
    fn cancel_snooze_and_a_failed_request_never_retry_in_the_same_idle_period() {
        let mut m = machine(rule(), false);
        tick(&mut m, inputs(700), 0);
        let fx = m.step(IdleEvent::Cancel, T0.plus(5 * SECOND));
        assert!(fx.contains(&IdleEffect::HideCountdown));
        assert_eq!(acts(&tick(&mut m, inputs(2000), 100)), 0);
        assert!(tick(&mut m, inputs(2100), 200).is_empty(), "still idle: not re-armed");
        tick(&mut m, inputs(1), 300);
        assert!(!tick(&mut m, inputs(700), 400).is_empty(), "new activity re-arms it");

        let mut m = machine(rule(), false);
        tick(&mut m, inputs(700), 0);
        let fx = m.step(IdleEvent::Snooze { minutes: 30 }, T0.plus(5 * SECOND));
        assert!(fx.contains(&IdleEffect::HideCountdown));
        assert!(tick(&mut m, inputs(800), 600).is_empty(), "snoozed");
        assert!(
            !tick(&mut m, inputs(2600), 30 * 60 + 10).is_empty(),
            "after the snooze it asks again"
        );

        let mut m = machine(rule(), false);
        tick(&mut m, inputs(700), 0);
        tick(&mut m, inputs(760), 60);
        m.step(
            IdleEvent::ActFailed {
                why: "polkit refused".into(),
            },
            T0.plus(61 * SECOND),
        );
        assert!(matches!(m.view(), IdleView::Stopped { ref why } if why == "polkit refused"));
        assert!(tick(&mut m, inputs(900), 100).is_empty());
        tick(&mut m, inputs(3), 200);
        assert!(matches!(m.view(), IdleView::Watching { .. }));
    }

    #[test]
    fn after_asking_it_says_what_happened_without_retrying() {
        let mut m = machine(
            IdleSettings {
                action: IdleAction::ShutDown,
                ..rule()
            },
            false,
        );
        tick(&mut m, inputs(700), 0);
        tick(&mut m, inputs(760), 60);
        assert!(tick(&mut m, inputs(800), 100).is_empty());
        tick(&mut m, inputs(1000), 60 + 121);
        assert!(matches!(m.view(), IdleView::Stopped { ref why } if why.contains("app may be asking you to save")));
        // Used again after the request: resumed.
        let mut m = machine(rule(), false);
        tick(&mut m, inputs(700), 0);
        tick(&mut m, inputs(760), 60);
        tick(&mut m, inputs(2), 90);
        assert!(matches!(m.view(), IdleView::Stopped { ref why } if why.contains("used again")));
        // Cancelling during or after the request never acts.
        let mut m = machine(rule(), false);
        tick(&mut m, inputs(700), 0);
        tick(&mut m, inputs(760), 60);
        assert_eq!(acts(&m.step(IdleEvent::Cancel, T0.plus(62 * SECOND))), 0);
        let mut m = machine(rule(), false);
        assert_eq!(acts(&m.step(IdleEvent::Cancel, T0)), 0);
        let mut m = machine(rule(), false);
        tick(&mut m, inputs(700), 0);
        m.step(IdleEvent::ActFailed { why: "x".into() }, T0);
        assert!(
            matches!(m.view(), IdleView::Countdown { .. }),
            "a failure without a request is ignored"
        );
        let mut m = machine(rule(), false);
        assert!(m.step(IdleEvent::Snooze { minutes: 5 }, T0).is_empty());
    }

    #[test]
    fn the_did_not_happen_headline_names_the_action_that_was_asked() {
        for (action, expect) in [
            (IdleAction::ShutDown, "Shutdown did not happen: an app may be asking you to save"),
            (IdleAction::Sleep, "The computer did not sleep"),
            (IdleAction::Hibernate, "The computer did not hibernate"),
        ] {
            let mut m = machine(IdleSettings { action, ..rule() }, false);
            tick(&mut m, inputs(700), 0);
            tick(&mut m, inputs(760), 60);
            tick(&mut m, inputs(900), 60 + 121);
            assert!(
                matches!(m.view(), IdleView::Stopped { ref why } if why == expect),
                "{action:?}: {:?}",
                m.view()
            );
        }
    }

    #[test]
    fn the_view_never_reports_a_rule_that_is_off() {
        // Only timed actions are on: whatever the readings, the idle card is Disabled and never lists blockers or a watch.
        let mut m = machine(IdleSettings::default(), true);
        let mut loud = inputs(9000);
        loud.audio = Available(true);
        for t in 0..4 {
            tick(&mut m, if t % 2 == 0 { loud.clone() } else { inputs(5) }, t);
            assert_eq!(m.view(), IdleView::Disabled, "tick {t}");
        }
        let mut no_idle = inputs(0);
        no_idle.idle_secs = Unavailable("no idle source".into());
        tick(&mut m, no_idle, 9);
        assert_eq!(
            m.view(),
            IdleView::Disabled,
            "an unavailable idle source is not news while the rule is off"
        );
        // Turning the rule on shows it; turning it off again hides it at once.
        m.step(
            IdleEvent::Configure {
                idle: Box::new(rule()),
                timed_on: true,
            },
            T0.plus(10 * SECOND),
        );
        tick(&mut m, inputs(5), 10);
        assert!(matches!(m.view(), IdleView::Watching { .. }));
        m.step(
            IdleEvent::Configure {
                idle: Box::new(IdleSettings::default()),
                timed_on: true,
            },
            T0.plus(11 * SECOND),
        );
        assert_eq!(m.view(), IdleView::Disabled);
        // Cancel while watching skips this idle period; activity re-arms it.
        let mut m = machine(rule(), false);
        tick(&mut m, inputs(100), 0);
        m.step(IdleEvent::Cancel, T0.plus(SECOND));
        assert_eq!(acts(&tick(&mut m, inputs(2000), 5)), 0);
        tick(&mut m, inputs(1), 6);
        assert!(!tick(&mut m, inputs(700), 7).is_empty(), "new activity re-armed it");
    }

    #[test]
    fn without_an_idle_source_the_rule_is_not_available_and_never_acts() {
        let mut m = machine(rule(), false);
        let mut i = inputs(0);
        i.idle_secs = Unavailable("Linux without a desktop idle source".into());
        for t in 0..5 {
            assert_eq!(acts(&tick(&mut m, i.clone(), t * 1000)), 0);
        }
        assert!(matches!(m.view(), IdleView::Unavailable { ref reason } if reason.contains("idle source")));
    }

    #[test]
    fn snoozing_a_timed_countdown_brings_it_back_after_the_snooze_and_cancel_ends_it() {
        let due = |action| IdleEvent::TimedDue {
            action,
            countdown_secs: 60,
        };
        let mut m = machine(IdleSettings::default(), true);
        m.step(due(IdleAction::Hibernate), T0);
        let fx = m.step(IdleEvent::Snooze { minutes: 10 }, T0.plus(5 * SECOND));
        assert!(fx.contains(&IdleEffect::HideCountdown));
        assert!(
            matches!(
                m.view(),
                IdleView::Watching {
                    snoozed_until: Some(_),
                    ..
                }
            ),
            "the snooze shows"
        );
        assert!(
            m.step(due(IdleAction::Sleep), T0.plus(6 * SECOND)).is_empty(),
            "another due entry waits"
        );
        assert!(tick(&mut m, inputs(1), 300).is_empty(), "still snoozed");
        let fx = tick(&mut m, inputs(1), 10 * 60 + 6);
        assert!(
            matches!(fx[0], IdleEffect::ShowCountdown { action: IdleAction::Hibernate, deadline } if deadline == T0.plus((10 * 60 + 66) * SECOND)),
            "{fx:?}"
        );
        assert_eq!(acts(&tick(&mut m, inputs(1), 10 * 60 + 66)), 1);
        // A longer request than a day is a day.
        let mut m = machine(IdleSettings::default(), true);
        m.step(due(IdleAction::Hibernate), T0);
        m.step(IdleEvent::Snooze { minutes: 5000 }, T0);
        assert!(matches!(m.view(), IdleView::Watching { snoozed_until: Some(t), .. } if t == T0.plus(24 * HOUR)));
        // Cancel during the snooze ends it: nothing comes back.
        let mut m = machine(IdleSettings::default(), true);
        m.step(due(IdleAction::Hibernate), T0);
        m.step(IdleEvent::Snooze { minutes: 10 }, T0.plus(SECOND));
        m.step(IdleEvent::Cancel, T0.plus(2 * SECOND));
        assert!(tick(&mut m, inputs(1), 3600).is_empty());
        assert!(
            !m.step(due(IdleAction::Sleep), T0.plus(3601 * SECOND)).is_empty(),
            "the machine takes the next entry"
        );
    }

    #[test]
    fn a_timed_action_warns_then_acts_and_input_does_not_cancel_it_but_cancel_does() {
        let mut m = machine(IdleSettings::default(), true);
        let fx = m.step(
            IdleEvent::TimedDue {
                action: IdleAction::Hibernate,
                countdown_secs: 5,
            },
            T0,
        );
        assert!(
            matches!(fx[0], IdleEffect::ShowCountdown { action: IdleAction::Hibernate, deadline } if deadline == T0.plus(30 * SECOND)),
            "the countdown is at least 30 s"
        );
        assert_eq!(
            acts(&tick(&mut m, inputs(1), 10)),
            0,
            "using the computer does not cancel a timed action"
        );
        assert_eq!(acts(&tick(&mut m, inputs(1), 30)), 1);
        let mut m = machine(IdleSettings::default(), true);
        m.step(
            IdleEvent::TimedDue {
                action: IdleAction::Sleep,
                countdown_secs: 60,
            },
            T0,
        );
        assert!(
            m.step(
                IdleEvent::TimedDue {
                    action: IdleAction::ShutDown,
                    countdown_secs: 60
                },
                T0
            )
            .is_empty(),
            "one at a time"
        );
        m.step(IdleEvent::Cancel, T0.plus(SECOND));
        assert_eq!(acts(&tick(&mut m, inputs(5000), 100)), 0);
        assert_eq!(
            m.view(),
            IdleView::Disabled,
            "the idle rule is off, so the card says so even while a timed action is on"
        );
        assert!(m.settings().action == IdleAction::Sleep);
    }

    #[test]
    fn act_only_follows_a_countdown_that_every_fresh_read_passed_over_all_short_sequences() {
        // Exhaustive over sequences of 6 events: the system is asked at most once per idle period and only from Countdown.
        let n = 7usize;
        for code in 0..n.pow(6) {
            let mut m = machine(rule(), false);
            let (mut c, mut asked, mut prev_countdown) = (code, 0, false);
            for step in 0..6i64 {
                let ev = match c % n {
                    0 => IdleEvent::Tick(Box::new(inputs(700 + step as u64 * 100))),
                    1 => IdleEvent::Tick(Box::new(inputs(3))),
                    2 => IdleEvent::Tick(Box::new({
                        let mut i = inputs(900);
                        i.audio = Available(true);
                        i
                    })),
                    3 => IdleEvent::Cancel,
                    4 => IdleEvent::Snooze { minutes: 1 },
                    5 => IdleEvent::ActFailed { why: "x".into() },
                    _ => IdleEvent::Tick(Box::new(inputs(0))),
                };
                c /= n;
                let was_countdown = matches!(m.view(), IdleView::Countdown { .. });
                let fx = m.step(ev, T0.plus(step * 61 * SECOND));
                if acts(&fx) > 0 {
                    assert!(was_countdown, "Act outside a countdown: sequence {code}");
                    asked += 1;
                }
                prev_countdown = was_countdown;
            }
            let _ = prev_countdown;
            assert!(asked <= 3, "sequence {code}");
        }
    }

    #[test]
    fn timed_actions_are_due_once_per_local_day_at_or_after_their_minute() {
        let zone = FixedZone(480);
        // 2023-11-14 22:13:20 UTC is Wednesday 06:13:20 local (UTC+8).
        let now = T0;
        let e = |id, at_minute, repeat| TimedAction {
            id,
            action: IdleAction::Sleep,
            at_minute,
            repeat,
            last_day: None,
            done: false,
        };
        let mut s = TimedSettings {
            enabled: true,
            entries: vec![
                e(1, 6 * 60, Repeat::Daily),
                e(2, 7 * 60, Repeat::Daily),
                e(3, 6 * 60 + 10, Repeat::Weekdays(0b0000_0100)),
                e(4, 6 * 60, Repeat::Weekdays(0b0001_1011)),
                e(5, 0, Repeat::Once),
            ],
        };
        assert!(s.validate().is_ok());
        // Wednesday: bit 2. Due: 1 (06:00 passed), 3 (06:10 passed, Wednesday), 5 (once, 00:00 passed). Not 2 (07:00), not 4 (no Wednesday).
        assert_eq!(s.due(now, &zone).iter().map(|d| d.0).collect::<Vec<_>>(), [1, 3, 5]);
        s.mark_run(1, now, &zone);
        s.mark_run(5, now, &zone);
        assert_eq!(
            s.due(now, &zone).iter().map(|d| d.0).collect::<Vec<_>>(),
            [3],
            "a daily one ran today, a once one is done"
        );
        assert_eq!(
            s.due(now.plus(DAY), &zone).iter().map(|d| d.0).collect::<Vec<_>>(),
            [1, 4],
            "tomorrow (Thursday) the daily and the Monday-Tuesday-Thursday-Friday ones are due, the once one never"
        );
        assert_eq!(
            s.due(now.plus(HOUR), &zone).iter().map(|d| d.0).collect::<Vec<_>>(),
            [2, 3],
            "07:13 local: the 07:00 one is due"
        );
        s.enabled = false;
        assert!(s.due(now, &zone).is_empty());
        s.mark_run(99, now, &zone);
    }

    #[test]
    fn timed_settings_are_validated_with_what_to_fix() {
        let e = |id, at_minute, repeat| TimedAction {
            id,
            action: IdleAction::ShutDown,
            at_minute,
            repeat,
            last_day: None,
            done: false,
        };
        let ok = |v: Vec<TimedAction>| TimedSettings { enabled: true, entries: v }.validate();
        assert!(ok(vec![e(1, 1439, Repeat::Once)]).is_ok());
        assert!(ok(vec![e(1, 1440, Repeat::Once)]).unwrap_err().contains("00:00"));
        assert!(ok(vec![e(1, 5, Repeat::Weekdays(0))]).unwrap_err().contains("day of the week"));
        assert!(ok(vec![e(1, 5, Repeat::Weekdays(0x80))]).unwrap_err().contains("day of the week"));
        assert!(ok(vec![e(1, 5, Repeat::Daily), e(1, 6, Repeat::Daily)])
            .unwrap_err()
            .contains("own id"));
        assert!(ok((0..17).map(|i| e(i, 5, Repeat::Daily)).collect())
            .unwrap_err()
            .contains("At most"));
    }

    #[test]
    fn settings_are_off_by_default_clamped_and_optional_on_the_wire() {
        let d = IdleSettings::default();
        assert!(!d.enabled && d.ac_only && d.action == IdleAction::Sleep && d.after_secs == 1800 && d.countdown_secs == 60);
        assert!(!TimedSettings::default().enabled);
        let s = IdleSettings {
            after_secs: 1,
            countdown_secs: 5,
            exceptions: Exceptions {
                cpu_pct: Some(f32::NAN),
                disk_bytes_per_s: Some(1),
                net_bytes_per_s: Some(u64::MAX),
                programs: vec!["  a ".into(), "".into(), "x".repeat(300)],
                ..Default::default()
            },
            ..Default::default()
        }
        .normalised();
        assert_eq!((s.after_secs, s.countdown_secs), (60, MIN_COUNTDOWN_SECS));
        assert_eq!(
            (s.exceptions.cpu_pct, s.exceptions.disk_bytes_per_s, s.exceptions.net_bytes_per_s),
            (None, Some(1024), Some(10 << 30))
        );
        assert_eq!(s.exceptions.programs.len(), 2);
        assert_eq!(s.exceptions.programs[0], "a");
        assert_eq!(s.exceptions.programs[1].len(), 256);
        let j: IdleSettings = serde_json::from_str(r#"{"enabled":true}"#).unwrap();
        assert!(j.enabled && j.exceptions.audio);
        assert_eq!(IdleAction::ShutDown.verb(), "shut down");
        assert_eq!(IdleAction::Hibernate.verb(), "hibernate");
        let big = IdleSettings {
            countdown_secs: 99999,
            ..Default::default()
        }
        .normalised();
        assert_eq!(big.countdown_secs, MAX_COUNTDOWN_SECS);
    }
}
