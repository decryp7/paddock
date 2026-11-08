//! Scheduled wake (ADR 0032 section 5, D9): a time of day (once, every day or on chosen weekdays) at which the computer wakes and
//! then, as the person chose, stays awake for a while, wakes saved Wake-on-LAN computers and runs one job from a fixed list
//! ([`DynoJob`]: never a command line). Pure: the zone and the clock are passed in; the OS wake events and the actions are the
//! agent's. Nothing wakes a computer that is shut down; the page says so.

use crate::time::{from_local, to_local, LocalZone, Timestamp, DAY, MINUTE};
use serde::{Deserialize, Serialize};

/// The most schedules Dyno keeps: the helper holds at most 8 pending wakes (`MAX_WAKES`), one per schedule.
pub const MAX_ENTRIES: usize = 8;
/// The most Wake-on-LAN computers one schedule wakes.
pub const MAX_DEVICES: usize = 8;
/// The longest "stay awake", in minutes (12 hours).
pub const MAX_STAY_MINUTES: u32 = 12 * 60;
/// The longest name, in characters.
pub const MAX_NAME_CHARS: usize = 48;
/// How late a wake may still be acted on (the agent was busy or just started): ten minutes. Later than that it is "missed".
pub const GRACE_MS: i64 = 10 * MINUTE;

/// When it repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Repeat {
    /// The next time the clock shows the time, then it is switched off.
    Once,
    /// Every day.
    Daily,
    /// On these weekdays: bit 0 Monday to bit 6 Sunday.
    Days {
        /// The weekday bits.
        mask: u8,
    },
}

/// The jobs a schedule may run: a fixed list, never a command line (D9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DynoJob {
    /// List the installed software.
    SoftwareInventory,
}

/// How the last run went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastRun {
    /// When.
    pub at: Timestamp,
    /// What happened, in words.
    pub summary: String,
}

/// A saved schedule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WakeSchedule {
    /// Its id (assigned by the agent; the OS wake's tag is derived from it).
    pub id: u32,
    /// The person's name for it.
    pub name: String,
    /// The local time of day, minutes after midnight.
    pub minute_of_day: u16,
    /// When it repeats.
    pub repeat: Repeat,
    /// Stay awake this many minutes after waking, when set.
    pub stay_awake_minutes: Option<u32>,
    /// Saved Wake-on-LAN computers to wake.
    pub wake_devices: Vec<u64>,
    /// A job to run.
    pub job: Option<DynoJob>,
    /// Switched on.
    pub enabled: bool,
    /// The next wake this schedule is armed for (kept so a restart still knows what was due).
    pub next_at: Option<Timestamp>,
    /// How its last run went.
    pub last_run: Option<LastRun>,
}

impl Default for WakeSchedule {
    fn default() -> Self {
        WakeSchedule {
            id: 0,
            name: String::new(),
            minute_of_day: 7 * 60,
            repeat: Repeat::Daily,
            stay_awake_minutes: None,
            wake_devices: Vec::new(),
            job: None,
            enabled: true,
            next_at: None,
            last_run: None,
        }
    }
}

/// What the person typed when adding or changing a schedule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WakeInput {
    /// The schedule being changed; none when adding.
    pub id: Option<u32>,
    /// The name.
    pub name: String,
    /// The local time of day, minutes after midnight.
    pub minute_of_day: u16,
    /// When it repeats.
    pub repeat: Repeat,
    /// Stay awake this long after waking.
    pub stay_awake_minutes: Option<u32>,
    /// Computers to wake.
    pub wake_devices: Vec<u64>,
    /// A job to run.
    pub job: Option<DynoJob>,
    /// Switched on.
    pub enabled: bool,
}

/// The OS wake's tag for a schedule: `dyno-<id>`, always a valid `WakeTag`.
pub fn tag_of(id: u32) -> String {
    format!("dyno-{id}")
}

/// Checks what the UI sent (the agent re-validates). The message says what to fix. `known_devices` are the saved computers.
pub fn validate(input: &WakeInput, saved: &[WakeSchedule], known_devices: &[u64]) -> Result<WakeSchedule, String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("Give the schedule a name.".into());
    }
    if name.chars().count() > MAX_NAME_CHARS || name.chars().any(char::is_control) {
        return Err(format!(
            "A name is at most {MAX_NAME_CHARS} characters, with no control characters."
        ));
    }
    if input.minute_of_day >= 24 * 60 {
        return Err("The time is a time of day, 00:00 to 23:59.".into());
    }
    if let Repeat::Days { mask } = input.repeat {
        if mask == 0 || mask > 0b111_1111 {
            return Err("Choose at least one day.".into());
        }
    }
    if let Some(m) = input.stay_awake_minutes {
        if !(1..=MAX_STAY_MINUTES).contains(&m) {
            return Err(format!("Stay awake for 1 to {MAX_STAY_MINUTES} minutes."));
        }
    }
    if input.wake_devices.len() > MAX_DEVICES {
        return Err(format!("A schedule wakes at most {MAX_DEVICES} computers."));
    }
    for (i, d) in input.wake_devices.iter().enumerate() {
        if !known_devices.contains(d) {
            return Err("One of the computers to wake is no longer in the Wake devices list.".into());
        }
        if input.wake_devices[..i].contains(d) {
            return Err("A computer is listed twice.".into());
        }
    }
    match input.id {
        Some(id) if !saved.iter().any(|s| s.id == id) => return Err("That schedule is no longer in the list.".into()),
        None if saved.len() >= MAX_ENTRIES => return Err(format!("At most {MAX_ENTRIES} schedules; remove one first.")),
        _ => {}
    }
    if input.stay_awake_minutes.is_none() && input.wake_devices.is_empty() && input.job.is_none() {
        return Err("Choose what happens after the wake: stay awake, wake a computer or run a job.".into());
    }
    let base = input
        .id
        .and_then(|id| saved.iter().find(|s| s.id == id))
        .cloned()
        .unwrap_or_default();
    Ok(WakeSchedule {
        id: base.id,
        name: name.to_string(),
        minute_of_day: input.minute_of_day,
        repeat: input.repeat,
        stay_awake_minutes: input.stay_awake_minutes,
        wake_devices: input.wake_devices.clone(),
        job: input.job,
        enabled: input.enabled,
        next_at: base.next_at,
        last_run: base.last_run,
    })
}

fn weekday_of_local_day(epoch_day: i64) -> u32 {
    // 1970-01-01 was a Thursday; Monday is 0.
    u32::try_from((epoch_day + 3).rem_euclid(7)).unwrap_or(0)
}

/// The first local occurrence of the schedule's time strictly after `after` (a nonexistent local time in a DST gap runs at the first
/// minute after it, a repeated one once: through the zone port). `None` for a schedule with no day.
pub fn next_occurrence(s: &WakeSchedule, after: Timestamp, zone: &dyn LocalZone) -> Option<Timestamp> {
    let day0 = to_local(after, zone).div_euclid(DAY);
    let offset = i64::from(s.minute_of_day) * MINUTE;
    (0..9).find_map(|k| {
        let day = day0 + k;
        let on = match s.repeat {
            Repeat::Once | Repeat::Daily => true,
            Repeat::Days { mask } => mask & (1 << weekday_of_local_day(day)) != 0,
        };
        let cand = from_local(day * DAY + offset, zone);
        (on && cand > after).then_some(cand)
    })
}

/// What a schedule does at `now`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fire {
    /// Its time has not come, or it is switched off.
    NotYet,
    /// It is due and within the grace: run its actions.
    Run,
    /// Its time passed more than [`GRACE_MS`] ago (the computer was off or the agent was not running): skip it.
    Missed,
}

/// Whether `s` is due at `now`, by its armed time.
pub fn fire(s: &WakeSchedule, now: Timestamp) -> Fire {
    match s.next_at {
        Some(at) if s.enabled && now >= at => {
            if now.since(at) <= GRACE_MS {
                Fire::Run
            } else {
                Fire::Missed
            }
        }
        _ => Fire::NotYet,
    }
}

/// The schedule after its armed time was handled: armed for the next occurrence, or switched off when it was a one-time wake.
pub fn advance(s: &WakeSchedule, now: Timestamp, zone: &dyn LocalZone) -> WakeSchedule {
    let mut n = s.clone();
    match s.repeat {
        Repeat::Once => {
            n.enabled = false;
            n.next_at = None;
        }
        _ => n.next_at = next_occurrence(s, now, zone),
    }
    n
}

/// A schedule armed for its next occurrence after `now` (switched-off ones are not armed).
pub fn armed(s: &WakeSchedule, now: Timestamp, zone: &dyn LocalZone) -> WakeSchedule {
    let mut n = s.clone();
    n.next_at = if s.enabled { next_occurrence(s, now, zone) } else { None };
    n
}

/// How a row is doing on the page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum RowState {
    /// The OS wake is set (through the route).
    Armed {
        /// Who sets the OS wake.
        route: WakeRoute,
    },
    /// Switched off.
    Off,
    /// The schedule is on, but scheduled wake is turned off in Settings, so no OS wake is set and nothing runs.
    Paused,
    /// The wake cannot be set until the administrator helper is on (macOS, Linux).
    NeedsHelper,
    /// The OS refused or the helper is not usable.
    Failed {
        /// Why, with the next step.
        why: String,
    },
}

/// Who sets the OS wake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WakeRoute {
    /// The agent (Windows: a wake timer; it works while the agent runs).
    Agent,
    /// The administrator helper (macOS and the Linux package).
    Helper,
}

/// One row of the Schedule tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WakeRow {
    /// The schedule.
    pub entry: WakeSchedule,
    /// How it is doing.
    pub state: RowState,
}

/// What the Schedule tab shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum WakeRouteState {
    /// The agent sets wakes (Windows).
    Agent {
        /// How it works and what it needs.
        note: String,
        /// What Dyno cannot tell here (Allow wake timers, Modern Standby), each with where to check.
        cant_tell: Vec<String>,
    },
    /// The helper sets them and it is ready.
    HelperReady,
    /// The helper is needed and is not ready; `reason` says why and the page offers to turn it on.
    NeedsHelper {
        /// Why, from the helper's state.
        reason: String,
    },
    /// Nothing can set a wake here (no helper in this build).
    Unavailable {
        /// Why.
        reason: String,
    },
}

/// The Schedule tab's data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WakesView {
    /// The schedules.
    pub rows: Vec<WakeRow>,
    /// Who can set the OS wake here.
    pub route: WakeRouteState,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::FixedZone;

    const T0: Timestamp = Timestamp(1_700_000_000_000); // 2023-11-14 22:13:20 UTC = 06:13:20 on the 15th at +08:00 (a Wednesday)

    fn s(minute: u16, repeat: Repeat) -> WakeSchedule {
        WakeSchedule {
            id: 1,
            name: "n".into(),
            minute_of_day: minute,
            repeat,
            stay_awake_minutes: Some(30),
            ..WakeSchedule::default()
        }
    }

    fn input() -> WakeInput {
        WakeInput {
            id: None,
            name: " Morning ".into(),
            minute_of_day: 420,
            repeat: Repeat::Daily,
            stay_awake_minutes: Some(30),
            wake_devices: vec![1],
            job: None,
            enabled: true,
        }
    }

    #[test]
    fn the_next_occurrence_follows_the_local_clock() {
        let zone = FixedZone(480);
        // 07:00 local is later today, 06:00 local is tomorrow.
        assert_eq!(
            next_occurrence(&s(420, Repeat::Daily), T0, &zone).unwrap().since(T0),
            46 * MINUTE + 40_000
        );
        assert_eq!(
            next_occurrence(&s(360, Repeat::Daily), T0, &zone).unwrap().since(T0),
            23 * 60 * MINUTE + 46 * MINUTE + 40_000
        );
        // Exactly at the time is not "after": the next one is a day later.
        let at = next_occurrence(&s(420, Repeat::Once), T0, &zone).unwrap();
        assert_eq!(next_occurrence(&s(420, Repeat::Once), at, &zone).unwrap().since(at), DAY);
    }

    #[test]
    fn weekdays_pick_the_next_chosen_day() {
        let zone = FixedZone(480);
        // T0 is Wednesday 06:13 local. Friday only (bit 4): Friday 07:00.
        let fri = next_occurrence(&s(420, Repeat::Days { mask: 1 << 4 }), T0, &zone).unwrap();
        assert_eq!(fri.since(T0), 2 * DAY + 46 * MINUTE + 40_000);
        // Wednesday only and the time passed today: next Wednesday.
        let wed = next_occurrence(&s(360, Repeat::Days { mask: 1 << 2 }), T0, &zone).unwrap();
        assert_eq!(wed.since(T0), 7 * DAY - 13 * MINUTE - 20_000);
        assert_eq!(next_occurrence(&s(420, Repeat::Days { mask: 0 }), T0, &zone), None);
    }

    #[test]
    fn a_due_schedule_runs_within_the_grace_and_is_missed_after_it() {
        let mut x = s(420, Repeat::Daily);
        x.next_at = Some(T0);
        assert_eq!(fire(&x, T0.minus(1)), Fire::NotYet);
        assert_eq!(fire(&x, T0), Fire::Run);
        assert_eq!(fire(&x, T0.plus(GRACE_MS)), Fire::Run);
        assert_eq!(fire(&x, T0.plus(GRACE_MS + 1)), Fire::Missed);
        x.enabled = false;
        assert_eq!(fire(&x, T0.plus(1)), Fire::NotYet);
        assert_eq!(fire(&s(420, Repeat::Daily), T0), Fire::NotYet, "an unarmed schedule never fires");
    }

    #[test]
    fn advancing_arms_the_next_one_and_a_one_time_wake_switches_off() {
        let zone = FixedZone(480);
        let mut d = s(420, Repeat::Daily);
        d.next_at = Some(T0);
        let handled = T0.plus(46 * MINUTE + 40_000);
        let n = advance(&d, handled, &zone);
        assert_eq!(n.next_at.unwrap().since(handled), DAY);
        assert!(n.enabled);
        let mut o = s(420, Repeat::Once);
        o.next_at = Some(T0);
        let n = advance(&o, T0, &zone);
        assert!(!n.enabled && n.next_at.is_none());
        assert_eq!(armed(&o, T0, &zone).next_at.unwrap().since(T0), 46 * MINUTE + 40_000);
        o.enabled = false;
        assert_eq!(armed(&o, T0, &zone).next_at, None);
    }

    #[test]
    fn a_good_input_becomes_a_schedule_and_a_change_keeps_what_the_agent_owns() {
        let v = validate(&input(), &[], &[1, 2]).unwrap();
        assert_eq!(
            (v.name.as_str(), v.minute_of_day, v.wake_devices.clone()),
            ("Morning", 420, vec![1])
        );
        let mut saved = s(300, Repeat::Daily);
        saved.next_at = Some(T0);
        saved.last_run = Some(LastRun {
            at: T0,
            summary: "ok".into(),
        });
        let mut i = input();
        i.id = Some(1);
        let v = validate(&i, &[saved.clone()], &[1]).unwrap();
        assert_eq!((v.next_at, v.last_run), (saved.next_at, saved.last_run));
        assert_eq!(tag_of(7), "dyno-7");
        assert!(crate::wake::WakeTag::parse(&tag_of(u32::MAX)).is_ok());
    }

    #[test]
    fn bad_inputs_say_what_to_fix() {
        let err = |f: &dyn Fn(&mut WakeInput)| {
            let mut i = input();
            f(&mut i);
            validate(&i, &[s(1, Repeat::Daily)], &[1]).unwrap_err()
        };
        assert!(err(&|i| i.name = "  ".into()).contains("name"));
        assert!(err(&|i| i.name = "x".repeat(49)).contains("at most 48"));
        assert!(err(&|i| i.minute_of_day = 1440).contains("time of day"));
        assert!(err(&|i| i.repeat = Repeat::Days { mask: 0 }).contains("at least one day"));
        assert!(err(&|i| i.repeat = Repeat::Days { mask: 0x80 }).contains("at least one day"));
        assert!(err(&|i| i.stay_awake_minutes = Some(0)).contains("1 to 720"));
        assert!(err(&|i| i.stay_awake_minutes = Some(721)).contains("1 to 720"));
        assert!(err(&|i| i.wake_devices = vec![9]).contains("no longer"));
        assert!(err(&|i| i.wake_devices = vec![1, 1]).contains("twice"));
        assert!(err(&|i| i.wake_devices = (1..=9).collect()).contains("at most 8"));
        assert!(err(&|i| i.id = Some(5)).contains("no longer"));
        assert!(err(&|i| {
            i.stay_awake_minutes = None;
            i.wake_devices = vec![];
        })
        .contains("what happens"));
        let full: Vec<WakeSchedule> = (1..=8)
            .map(|n| WakeSchedule {
                id: n,
                ..s(1, Repeat::Daily)
            })
            .collect();
        assert!(validate(&input(), &full, &[1]).unwrap_err().contains("At most 8"));
    }

    /// A zone whose offset changes once, at `at` (a DST step).
    #[derive(Debug)]
    struct StepZone {
        at: Timestamp,
        before: i32,
        after: i32,
    }

    impl LocalZone for StepZone {
        fn offset_minutes(&self, t: Timestamp) -> i32 {
            if t < self.at {
                self.before
            } else {
                self.after
            }
        }
    }

    fn local_minute_of(t: Timestamp, zone: &dyn LocalZone) -> i64 {
        to_local(t, zone).rem_euclid(DAY) / MINUTE
    }

    #[test]
    fn the_next_wake_is_the_chosen_local_time_in_any_offset() {
        // The schedule's time is local wall time: whatever the offset, the next occurrence shows that time on the local clock.
        for offset in [-300, 0, 330, 480, 765] {
            let zone = FixedZone(offset);
            for minute in [0u16, 30, 420, 1080, 1439] {
                let next = next_occurrence(&s(minute, Repeat::Daily), T0, &zone).unwrap();
                assert_eq!(local_minute_of(next, &zone), i64::from(minute), "offset {offset}, minute {minute}");
                assert!(next > T0 && next.since(T0) <= DAY);
            }
        }
        // 07:00 at UTC+8 is 23:00 UTC the evening before.
        let sg = FixedZone(480);
        let next = next_occurrence(&s(420, Repeat::Daily), T0, &sg).unwrap();
        assert_eq!(next.0 % DAY, 23 * 3_600_000);
    }

    #[test]
    fn a_wake_in_the_spring_gap_runs_the_first_minute_after_it_and_a_fall_fold_runs_once() {
        // New York: clocks 02:00 EST -> 03:00 EDT at 2023-03-12 07:00 UTC; 02:00 EDT -> 01:00 EST at 2023-11-05 06:00 UTC.
        let spring = StepZone {
            at: Timestamp(1_678_604_400_000),
            before: -300,
            after: -240,
        };
        let eve = Timestamp(1_678_564_800_000); // 2023-03-11 20:00 UTC = 15:00 EST
        let next = next_occurrence(&s(150, Repeat::Daily), eve, &spring).unwrap(); // 02:30 daily
        assert_eq!(
            local_minute_of(next, &spring),
            3 * 60 + 30,
            "02:30 does not exist that night: 03:30"
        );
        assert!(next >= spring.at);
        let fall = StepZone {
            at: Timestamp(1_699_164_000_000),
            before: -240,
            after: -300,
        };
        let eve = Timestamp(1_699_131_600_000); // 2023-11-04 21:00 UTC = 17:00 EDT
        let first = next_occurrence(&s(90, Repeat::Daily), eve, &fall).unwrap(); // 01:30 daily
        assert_eq!(first.0, 1_699_162_200_000, "01:30 happens twice that night: the earlier one (EDT)");
        let second = next_occurrence(&s(90, Repeat::Daily), first, &fall).unwrap();
        assert_eq!(
            second.since(first),
            DAY + 3_600_000,
            "once that night: the next is the next day's 01:30 EST"
        );
    }

    #[test]
    fn the_weekday_of_a_day_counts_from_monday() {
        assert_eq!(weekday_of_local_day(0), 3, "1970-01-01 was a Thursday");
        assert_eq!(weekday_of_local_day(4), 0);
        assert_eq!(weekday_of_local_day(-3), 0);
    }
}
