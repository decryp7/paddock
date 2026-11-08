//! Maintenance windows: the times when Dyno's heavier background work (reading the OS logs, inventory
//! snapshots) may run. With no windows configured everything may run at any time. Work the user starts with a click
//! always runs: a window limits only what Dyno starts by itself.

use crate::time::{starts_of, to_local, LocalZone, Timestamp, DAY, MINUTE};
use serde::{Deserialize, Serialize};

/// How far ahead the status looks for the next change: a week and a day.
const LOOK_AHEAD_DAYS: i64 = 8;

/// The most windows a user may define.
pub const MAX_WINDOWS: usize = 14;
/// Minutes in a day.
const DAY_MINUTES: u16 = 24 * 60;

/// A day of the week.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Weekday {
    /// Monday.
    Mon,
    /// Tuesday.
    Tue,
    /// Wednesday.
    Wed,
    /// Thursday.
    Thu,
    /// Friday.
    Fri,
    /// Saturday.
    Sat,
    /// Sunday.
    Sun,
}

impl Weekday {
    const ALL: [Weekday; 7] = [
        Weekday::Mon,
        Weekday::Tue,
        Weekday::Wed,
        Weekday::Thu,
        Weekday::Fri,
        Weekday::Sat,
        Weekday::Sun,
    ];

    /// The weekday of a count of days since 1970-01-01 (a Thursday).
    fn from_epoch_day(day: i64) -> Weekday {
        Weekday::ALL[(day + 3).rem_euclid(7) as usize]
    }
}

/// One window: some weekdays, from a local time to a local time. An end at or before the start runs past midnight into
/// the next day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    /// The days it starts on.
    pub days: Vec<Weekday>,
    /// Start, minutes after local midnight (0-1439).
    pub start_minute: u16,
    /// End, minutes after local midnight (0-1439); not equal to the start.
    pub end_minute: u16,
}

/// The setting: off (always allowed) or a list of windows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct MaintenanceSettings {
    /// Only run heavier background work inside the windows.
    pub enabled: bool,
    /// The windows.
    pub windows: Vec<Window>,
}

/// Where the user is in their windows, for the Maintenance view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaintenanceStatus {
    /// Windows are in use.
    pub enabled: bool,
    /// Heavier work may run now.
    pub open_now: bool,
    /// When the current window ends (when open and windows are in use).
    pub closes_at: Option<Timestamp>,
    /// When the next window opens (when closed).
    pub next_opens_at: Option<Timestamp>,
}

impl MaintenanceSettings {
    /// Checks the setting; the agent refuses to save one that fails.
    pub fn validate(&self) -> Result<(), String> {
        if self.windows.len() > MAX_WINDOWS {
            return Err(format!("At most {MAX_WINDOWS} maintenance windows."));
        }
        if self.enabled && self.windows.is_empty() {
            return Err("Add a window, or turn maintenance windows off.".into());
        }
        for w in &self.windows {
            if w.days.is_empty() {
                return Err("Pick at least one day for each window.".into());
            }
            if w.start_minute >= DAY_MINUTES || w.end_minute >= DAY_MINUTES {
                return Err("A window's times are between 00:00 and 23:59.".into());
            }
            if w.start_minute == w.end_minute {
                return Err("A window must end at a different time than it starts.".into());
            }
        }
        Ok(())
    }

    /// Whether heavier work may run at `now` (always, when windows are not in use).
    pub fn is_open(&self, now: Timestamp, zone: &dyn LocalZone) -> bool {
        if !self.enabled {
            return true;
        }
        // One zone lookup, then every window against the same local day and minute.
        let local = to_local(now, zone);
        let (day, minute) = (local.div_euclid(DAY), (local.rem_euclid(DAY) / MINUTE) as u16);
        self.windows.iter().any(|w| covers(w, day, minute))
    }

    /// The status at `now`.
    pub fn status(&self, now: Timestamp, zone: &dyn LocalZone) -> MaintenanceStatus {
        let open_now = self.is_open(now, zone);
        if !self.enabled {
            return MaintenanceStatus {
                enabled: false,
                open_now,
                closes_at: None,
                next_opens_at: None,
            };
        }
        // The state changes only where a window starts or ends: look at those instants in order, up to a week and a day
        // ahead (bounded: 9 days x 14 windows x 2 edges), instead of at every minute.
        let today = to_local(now, zone).div_euclid(DAY);
        let horizon = now.plus(LOOK_AHEAD_DAYS * DAY);
        let mut edges: Vec<Timestamp> = (today..=today + LOOK_AHEAD_DAYS)
            .flat_map(|day| {
                self.windows
                    .iter()
                    .flat_map(move |w| [w.start_minute, w.end_minute].map(|m| day * DAY + i64::from(m) * MINUTE))
            })
            .flat_map(|wall| starts_of(wall, zone))
            .filter(|t| *t > now && *t <= horizon)
            .collect();
        edges.sort_unstable();
        edges.dedup();
        let change = edges.into_iter().find(|t| self.is_open(*t, zone) != open_now);
        let (closes_at, next_opens_at) = if open_now { (change, None) } else { (None, change) };
        MaintenanceStatus {
            enabled: true,
            open_now,
            closes_at,
            next_opens_at,
        }
    }
}

/// Whether `w` covers `minute` of the local `day` (days since the local epoch; a window running past midnight belongs to the
/// day it started).
fn covers(w: &Window, day: i64, minute: u16) -> bool {
    let on = |d: i64| w.days.contains(&Weekday::from_epoch_day(d));
    if w.start_minute < w.end_minute {
        on(day) && minute >= w.start_minute && minute < w.end_minute
    } else {
        (on(day) && minute >= w.start_minute) || (on(day - 1) && minute < w.end_minute)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::{FixedZone, DAY};

    const UTC: &FixedZone = &FixedZone::UTC;

    // 2026-10-23 is a Friday. 2026-10-23T00:00:00Z in ms:
    const FRI: i64 = 1_792_713_600_000;

    fn at(day_offset: i64, hh: i64, mm: i64) -> Timestamp {
        Timestamp(FRI + day_offset * DAY + hh * 60 * MINUTE + mm * MINUTE)
    }

    fn night() -> MaintenanceSettings {
        MaintenanceSettings {
            enabled: true,
            windows: vec![Window {
                days: vec![Weekday::Fri],
                start_minute: 23 * 60,
                end_minute: 2 * 60,
            }],
        }
    }

    #[test]
    fn the_reference_day_is_a_friday() {
        assert_eq!(Weekday::from_epoch_day(FRI / DAY), Weekday::Fri);
        assert_eq!(Weekday::from_epoch_day(0), Weekday::Thu);
    }

    #[test]
    fn off_means_always_open() {
        let s = MaintenanceSettings::default();
        assert!(s.is_open(at(0, 12, 0), UTC));
        let st = s.status(at(0, 12, 0), UTC);
        assert!(!st.enabled && st.open_now && st.closes_at.is_none() && st.next_opens_at.is_none());
    }

    #[test]
    fn a_window_past_midnight_belongs_to_the_day_it_started() {
        let s = night();
        assert!(!s.is_open(at(0, 22, 59), UTC));
        assert!(s.is_open(at(0, 23, 0), UTC));
        // Saturday 01:59 is still Friday's window; Saturday 02:00 is not.
        assert!(s.is_open(at(1, 1, 59), UTC));
        assert!(!s.is_open(at(1, 2, 0), UTC));
        // Friday 01:00 belongs to Thursday's window, which does not exist.
        assert!(!s.is_open(at(0, 1, 0), UTC));
    }

    #[test]
    fn the_local_offset_moves_the_window() {
        let s = night();
        // 15:30 UTC is 23:30 at +08:00.
        assert!(s.is_open(at(0, 15, 30), &FixedZone(8 * 60)));
        assert!(!s.is_open(at(0, 15, 30), UTC));
    }

    #[test]
    fn status_reports_when_it_closes_and_opens() {
        let s = night();
        let open = s.status(at(0, 23, 30), UTC);
        assert!(open.open_now);
        assert_eq!(open.closes_at, Some(at(1, 2, 0)));
        let closed = s.status(at(0, 12, 0), UTC);
        assert!(!closed.open_now);
        assert_eq!(closed.next_opens_at, Some(at(0, 23, 0)));
        // After this Friday's window, the next opens a week later.
        let after = s.status(at(1, 3, 0), UTC);
        assert_eq!(after.next_opens_at, Some(at(7, 23, 0)));
    }

    #[test]
    fn a_same_day_window() {
        let s = MaintenanceSettings {
            enabled: true,
            windows: vec![Window {
                days: vec![Weekday::Fri, Weekday::Sat],
                start_minute: 9 * 60,
                end_minute: 10 * 60,
            }],
        };
        assert!(s.is_open(at(0, 9, 0), UTC));
        assert!(!s.is_open(at(0, 10, 0), UTC));
        assert!(s.is_open(at(1, 9, 59), UTC));
        assert!(!s.is_open(at(2, 9, 30), UTC));
    }

    /// Central European Time with 2026's summer time (+60, +120 from 29 March 01:00 UTC to 25 October 01:00 UTC).
    #[derive(Debug)]
    struct Berlin2026;
    impl LocalZone for Berlin2026 {
        fn offset_minutes(&self, at: Timestamp) -> i32 {
            if (1_774_746_000_000..1_792_890_000_000).contains(&at.millis()) {
                120
            } else {
                60
            }
        }
    }

    /// The status the slow way: every minute for a week and a day (the implementation before edges, kept as the reference).
    fn by_minutes(s: &MaintenanceSettings, now: Timestamp, zone: &dyn LocalZone) -> MaintenanceStatus {
        let open_now = s.is_open(now, zone);
        let mut t = now.floor_to(MINUTE);
        let mut change = None;
        for _ in 0..(LOOK_AHEAD_DAYS * 24 * 60) {
            t = t.plus(MINUTE);
            if s.is_open(t, zone) != open_now {
                change = Some(t);
                break;
            }
        }
        MaintenanceStatus {
            enabled: true,
            open_now,
            closes_at: change.filter(|_| open_now),
            next_opens_at: change.filter(|_| !open_now),
        }
    }

    #[test]
    fn jumping_to_window_edges_finds_what_scanning_every_minute_finds() {
        let windows = [
            (vec![Weekday::Fri], 23 * 60, 2 * 60),
            (vec![Weekday::Sun], 2 * 60 + 30, 3 * 60),
            (vec![Weekday::Mon, Weekday::Wed], 0, 60),
            (vec![Weekday::Sat, Weekday::Sun], 9 * 60, 17 * 60),
        ];
        let s = MaintenanceSettings {
            enabled: true,
            windows: windows
                .into_iter()
                .map(|(days, start_minute, end_minute)| Window {
                    days,
                    start_minute,
                    end_minute,
                })
                .collect(),
        };
        // Every 97 minutes over the weeks around both of 2026's clock changes, in Berlin, UTC and UTC+8.
        let zones: [&dyn LocalZone; 3] = [&Berlin2026, UTC, &FixedZone(480)];
        for start in [1_774_137_600_000i64, 1_792_281_600_000] {
            for step in 0..300 {
                let now = Timestamp(start + step * 97 * MINUTE + 7_000);
                for zone in zones {
                    assert_eq!(s.status(now, zone), by_minutes(&s, now, zone), "{now:?} {zone:?}");
                }
            }
        }
    }

    #[test]
    fn validation_names_what_to_fix() {
        let mut s = night();
        assert!(s.validate().is_ok());
        s.windows[0].days.clear();
        assert!(s.validate().unwrap_err().contains("at least one day"));
        s.windows[0].days = vec![Weekday::Mon];
        s.windows[0].end_minute = s.windows[0].start_minute;
        assert!(s.validate().unwrap_err().contains("different time"));
        s.windows[0].end_minute = 1500;
        assert!(s.validate().unwrap_err().contains("00:00"));
        s.windows.clear();
        assert!(s.validate().unwrap_err().contains("Add a window"));
        s.enabled = false;
        assert!(s.validate().is_ok());
        s.windows = vec![night().windows[0].clone(); MAX_WINDOWS + 1];
        assert!(s.validate().unwrap_err().contains("At most"));
    }

    #[test]
    fn the_wire_shape_is_camel_case() {
        let j = serde_json::to_value(night()).unwrap();
        assert_eq!(j["windows"][0]["startMinute"], 1380);
        assert_eq!(j["windows"][0]["days"][0], "fri");
        let back: MaintenanceSettings = serde_json::from_value(j).unwrap();
        assert_eq!(back, night());
        // Every key is optional.
        assert_eq!(
            serde_json::from_str::<MaintenanceSettings>("{}").unwrap(),
            MaintenanceSettings::default()
        );
    }
}
