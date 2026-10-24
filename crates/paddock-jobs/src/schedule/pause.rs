//! Pause windows and quiet hours: spans of local time (in a chosen or the computer's time zone, daylight saving
//! included) when work waits or notifications are held. A window belongs to the weekday it starts on and may span
//! midnight. Like the rest of [`super`], everything takes "now" as a value.

use super::calendar::{self, LocalZone, MAX_ZONE_LEN, Weekday};
use super::moment::{ClockTime, UnixMs};
use serde::{Deserialize, Serialize};

/// Most windows in one set.
pub const MAX_WINDOWS: usize = 14;

/// A daily span on chosen days. `start == end` is empty; `start > end` spans midnight into the next day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PauseWindow {
    /// The days it starts on; empty means every day.
    #[serde(default)]
    pub days: Vec<Weekday>,
    /// When it opens.
    pub start: ClockTime,
    /// When it closes (exclusive).
    pub end: ClockTime,
}

impl PauseWindow {
    /// A window every day (quiet hours).
    pub fn daily(start: ClockTime, end: ClockTime) -> PauseWindow {
        PauseWindow {
            days: Vec::new(),
            start,
            end,
        }
    }

    fn on(&self, day: Weekday) -> bool {
        self.days.is_empty() || self.days.contains(&day)
    }

    /// Whether the local moment (`day`, `time`) is inside.
    pub fn contains(&self, day: Weekday, time: ClockTime) -> bool {
        let (s, e, n) = (self.start.minutes(), self.end.minutes(), time.minutes());
        match s.cmp(&e) {
            std::cmp::Ordering::Equal => false,
            std::cmp::Ordering::Less => self.on(day) && s <= n && n < e,
            std::cmp::Ordering::Greater => {
                (self.on(day) && n >= s) || (self.on(day.previous()) && n < e)
            }
        }
    }
}

/// A set of windows in one time zone.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PauseWindows {
    /// The windows; the set is open while any of them is.
    #[serde(default)]
    pub windows: Vec<PauseWindow>,
    /// An IANA time zone; the computer's own when absent.
    #[serde(default)]
    pub zone: Option<String>,
}

impl PauseWindows {
    /// Checks the set; the error lists every problem and says what to fix.
    pub fn check(&self) -> Result<(), Vec<String>> {
        let mut problems = Vec::new();
        if self.windows.len() > MAX_WINDOWS {
            problems.push(format!("Use at most {MAX_WINDOWS} windows."));
        }
        if self.zone.as_deref().is_some_and(|z| z.len() > MAX_ZONE_LEN) {
            problems.push(format!(
                "The time zone name is too long (at most {MAX_ZONE_LEN} characters)."
            ));
        } else if let Err(e) = calendar::resolve_zone(self.zone.as_deref(), &LocalZone::default()) {
            problems.push(e);
        }
        if self.windows.iter().any(|w| w.start == w.end) {
            problems.push("A window needs a start that differs from its end.".to_string());
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }

    /// Whether the set is open at `at`.
    pub fn holds(&self, local: &LocalZone, at: UnixMs) -> Result<bool, Vec<String>> {
        let zone = self.zone(local)?;
        Ok(self.holds_in(&zone, at))
    }

    fn zone(&self, local: &LocalZone) -> Result<jiff::tz::TimeZone, Vec<String>> {
        self.check()?;
        calendar::resolve_zone(self.zone.as_deref(), local).map_err(|e| vec![e])
    }

    fn holds_in(&self, zone: &jiff::tz::TimeZone, at: UnixMs) -> bool {
        let Some((day, time)) = calendar::local_moment(at, zone) else {
            return false;
        };
        self.windows.iter().any(|w| w.contains(day, time))
    }

    /// When the set next closes, if it is open at `at` (`None` when it is not open, or when the windows cover all
    /// time and never close).
    pub fn resume_at(&self, local: &LocalZone, at: UnixMs) -> Result<Option<UnixMs>, Vec<String>> {
        let zone = self.zone(local)?;
        if !self.holds_in(&zone, at) {
            return Ok(None);
        }
        // Each step moves to the earliest end of an open window; a daylight-saving shift can need one extra step.
        let end_after = |t: &UnixMs| {
            self.windows
                .iter()
                .filter(|w| w.contains_at(&zone, *t))
                .filter_map(|w| calendar::next_at_times(&Weekday::ALL, &[w.end], &zone, *t))
                .min()
        };
        Ok(std::iter::successors(Some(at), end_after)
            .skip(1)
            .take(self.windows.len() * 2 + 8)
            .find(|t| !self.holds_in(&zone, *t)))
    }

    /// When the set next opens, if it is closed at `at` (`None` when it is open, or never opens).
    pub fn next_open(&self, local: &LocalZone, at: UnixMs) -> Result<Option<UnixMs>, Vec<String>> {
        let zone = self.zone(local)?;
        if self.holds_in(&zone, at) {
            return Ok(None);
        }
        let start_after = |t: &UnixMs| {
            self.windows
                .iter()
                .filter_map(|w| calendar::next_at_times(&Weekday::ALL, &[w.start], &zone, *t))
                .min()
        };
        Ok(std::iter::successors(start_after(&at), start_after)
            .take(MAX_WINDOWS * 9)
            .find(|t| self.holds_in(&zone, *t)))
    }
}

impl PauseWindow {
    fn contains_at(&self, zone: &jiff::tz::TimeZone, at: UnixMs) -> bool {
        calendar::local_moment(at, zone).is_some_and(|(day, time)| self.contains(day, time))
    }
}
