//! Calendar maths for the "at times" and cron schedules: time zones, daylight-saving rules and the next occurrence.
//!
//! Everything here is pure: the time zone database is compiled in (jiff's bundled tzdb) and "now" is passed in as a
//! [`Timestamp`]. Daylight saving follows two rules, the same for both modes:
//!
//! - **Spring forward.** A wall-clock time that does not exist (02:30 on the night the clocks jump from 02:00 to
//!   03:00) still runs once, as the shifted time (03:30) in "at times" and at the first instant after the gap
//!   (03:00) for cron expressions.
//! - **Fall back.** A wall-clock time that happens twice (02:30 when the clocks go from 03:00 back to 02:00) runs
//!   once, the first time.

use crate::time::{ClockTime, Timestamp};
use croner::parser::{CronParser, Seconds};
use croner::Cron;
use jiff::tz::{Offset, TimeZone};
use serde::{Deserialize, Serialize};

/// Longest cron expression accepted, in bytes.
pub const MAX_CRON_LEN: usize = 200;
/// Longest time zone name accepted, in bytes.
pub const MAX_ZONE_LEN: usize = 64;
/// Most times of day in an "at times" schedule.
pub const MAX_TIMES: usize = 24;

/// A day of the week (Monday first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
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
    /// All seven days, Monday first.
    pub const ALL: [Weekday; 7] = [
        Weekday::Mon,
        Weekday::Tue,
        Weekday::Wed,
        Weekday::Thu,
        Weekday::Fri,
        Weekday::Sat,
        Weekday::Sun,
    ];

    /// Monday to Friday.
    pub const WEEKDAYS: [Weekday; 5] = [Weekday::Mon, Weekday::Tue, Weekday::Wed, Weekday::Thu, Weekday::Fri];

    /// The three-letter English name.
    pub fn short(self) -> &'static str {
        match self {
            Weekday::Mon => "Mon",
            Weekday::Tue => "Tue",
            Weekday::Wed => "Wed",
            Weekday::Thu => "Thu",
            Weekday::Fri => "Fri",
            Weekday::Sat => "Sat",
            Weekday::Sun => "Sun",
        }
    }

    fn from_jiff(d: jiff::civil::Weekday) -> Weekday {
        use jiff::civil::Weekday as J;
        match d {
            J::Monday => Weekday::Mon,
            J::Tuesday => Weekday::Tue,
            J::Wednesday => Weekday::Wed,
            J::Thursday => Weekday::Thu,
            J::Friday => Weekday::Fri,
            J::Saturday => Weekday::Sat,
            J::Sunday => Weekday::Sun,
        }
    }
}

/// The machine's own time zone, as the shell reports it: an IANA name when it knows one (which keeps daylight saving
/// right) and the current offset from UTC as the fallback.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LocalZone {
    /// The IANA name (`Europe/Berlin`), when known.
    pub name: Option<String>,
    /// The offset from UTC in minutes, used when `name` is missing or unknown.
    pub offset_minutes: i32,
}

/// Looks up an IANA time zone by name; the error says what to fix.
pub fn zone_named(name: &str) -> Result<TimeZone, String> {
    let name = name.trim();
    let valid = !name.is_empty()
        && name.len() <= MAX_ZONE_LEN
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'-' | b'+'));
    if !valid {
        return Err(format!(
            "\"{name}\" is not a time zone name. Use an IANA name such as Europe/Berlin or America/New_York."
        ));
    }
    TimeZone::get(name).map_err(|_| format!("Unknown time zone \"{name}\". Use an IANA name such as Europe/Berlin or America/New_York."))
}

/// The zone a schedule runs in: the chosen one, else the machine's.
pub fn resolve_zone(chosen: Option<&str>, local: &LocalZone) -> Result<TimeZone, String> {
    if let Some(name) = chosen.map(str::trim).filter(|n| !n.is_empty()) {
        return zone_named(name);
    }
    if let Some(zone) = local.name.as_deref().and_then(|n| zone_named(n).ok()) {
        return Ok(zone);
    }
    let seconds = local.offset_minutes.clamp(-25 * 60, 25 * 60) * 60;
    Offset::from_seconds(seconds)
        .map(TimeZone::fixed)
        .map_err(|_| "The computer's time zone is not known; choose one.".to_string())
}

/// A readable name for the zone in effect (`Europe/Berlin`, or `UTC+02:00` when only an offset is known).
pub fn zone_label(chosen: Option<&str>, local: &LocalZone) -> String {
    if let Some(name) = chosen.map(str::trim).filter(|n| !n.is_empty()) {
        return name.to_string();
    }
    if let Some(name) = local.name.as_deref().filter(|n| zone_named(n).is_ok()) {
        return name.to_string();
    }
    let m = local.offset_minutes;
    let sign = if m < 0 { '-' } else { '+' };
    format!("UTC{sign}{:02}:{:02}", m.abs() / 60, m.abs() % 60)
}

/// Parses a cron expression: the standard five fields (minute, hour, day of month, month, day of week), optionally
/// with a leading seconds field. The error says what to fix.
pub fn parse_cron(expr: &str) -> Result<Cron, String> {
    let expr = expr.trim();
    if expr.is_empty() {
        return Err("Enter a cron expression with five fields, for example 0 9 * * 1-5.".into());
    }
    if expr.len() > MAX_CRON_LEN {
        return Err(format!("The cron expression is too long (at most {MAX_CRON_LEN} characters)."));
    }
    let fields = expr.split_whitespace().count();
    if !expr.starts_with('@') && !(5..=6).contains(&fields) {
        return Err(format!(
            "A cron expression has five fields (minute hour day-of-month month day-of-week), but this has {fields}. Example: 0 9 * * 1-5."
        ));
    }
    CronParser::builder()
        .seconds(Seconds::Optional)
        .build()
        .parse(expr)
        .map_err(|e| format!("This is not a valid cron expression: {e}. Example: 0 9 * * 1-5 (weekdays at 09:00)."))
}

fn zoned_at(at: Timestamp, zone: &TimeZone) -> Option<jiff::Zoned> {
    jiff::Timestamp::from_millisecond(at.0).ok().map(|t| t.to_zoned(zone.clone()))
}

/// The first time of the "at times" schedule strictly after `after`.
pub fn next_at_times(days: &[Weekday], times: &[ClockTime], zone: &TimeZone, after: Timestamp) -> Option<Timestamp> {
    let mut date = zoned_at(after, zone)?.date();
    // A week plus the day the search starts on (and one more for a daylight-saving shift).
    for _ in 0..9 {
        if days.contains(&Weekday::from_jiff(date.weekday())) {
            let best = times
                .iter()
                .filter_map(|t| {
                    let local = date.at((t.minutes() / 60) as i8, (t.minutes() % 60) as i8, 0, 0);
                    let at = zone.to_ambiguous_zoned(local).compatible().ok()?;
                    Some(at.timestamp().as_millisecond())
                })
                .filter(|ms| *ms > after.0)
                .min();
            if let Some(ms) = best {
                return Some(Timestamp(ms));
            }
        }
        date = date.tomorrow().ok()?;
    }
    None
}

/// The first time of the cron expression strictly after `after`.
pub fn next_cron(cron: &Cron, zone: &TimeZone, after: Timestamp) -> Option<Timestamp> {
    let start = zoned_at(after, zone)?;
    cron.find_next_occurrence(&start, false)
        .ok()
        .map(|z| Timestamp(z.timestamp().as_millisecond()))
}

/// The wall-clock time of `at` in `zone` as `YYYY-MM-DD HH:MM` (for messages and tests).
pub fn wall_clock(at: Timestamp, zone: &TimeZone) -> String {
    zoned_at(at, zone).map_or_else(String::new, |z| z.strftime("%Y-%m-%d %H:%M").to_string())
}
