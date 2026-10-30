//! Maintenance windows: the times when Dyno's heavier background work (reading the OS logs, backup checks, inventory
//! snapshots) may run. With no windows configured everything may run at any time. Work the user starts with a click
//! always runs: a window limits only what Dyno starts by itself.

use crate::time::{local_epoch_day, local_minute, LocalZone, Timestamp, MINUTE};
use serde::{Deserialize, Serialize};

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
        !self.enabled || self.windows.iter().any(|w| covers(w, now, zone))
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
        // Scan minute by minute for the next change over at most a week and a day (bounded: 10 000 steps).
        let mut closes_at = None;
        let mut next_opens_at = None;
        let mut t = now.floor_to(MINUTE);
        for _ in 0..(8 * 24 * 60) {
            t = t.plus(MINUTE);
            let open = self.is_open(t, zone);
            if open_now && !open {
                closes_at = Some(t);
                break;
            }
            if !open_now && open {
                next_opens_at = Some(t);
                break;
            }
        }
        MaintenanceStatus {
            enabled: true,
            open_now,
            closes_at,
            next_opens_at,
        }
    }
}

/// Whether `w` covers the local minute of `now` (a window running past midnight belongs to the day it started).
fn covers(w: &Window, now: Timestamp, zone: &dyn LocalZone) -> bool {
    let day = local_epoch_day(now, zone);
    let minute = local_minute(now, zone);
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
