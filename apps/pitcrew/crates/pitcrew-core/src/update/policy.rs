//! Update policy: whether, when and to which version a runner is updated.

use crate::version::RunnerVersion;
use serde::{Deserialize, Serialize};

/// How updates are applied (user decision: automatic, notify only, or manual).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateMode {
    /// Check on a schedule and install inside the update window, between jobs.
    #[default]
    Automatic,
    /// Check on a schedule and notify; the user starts the update.
    NotifyOnly,
    /// No scheduled checks; the user checks and updates.
    Manual,
}

/// A day of the week.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[allow(missing_docs)]
pub enum Weekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

/// A local time of day, `HH:MM` in the configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TimeOfDay(u16);

/// A time of day that is not `HH:MM`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not a time of day such as 02:30")]
pub struct TimeOfDayError(String);

impl TimeOfDay {
    /// `hour`:`minute`, or `None` when out of range.
    pub fn new(hour: u16, minute: u16) -> Option<Self> {
        (hour < 24 && minute < 60).then_some(TimeOfDay(hour * 60 + minute))
    }

    /// Minutes since midnight.
    pub fn minutes(self) -> u16 {
        self.0
    }
}

impl TryFrom<String> for TimeOfDay {
    type Error = TimeOfDayError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        let bad = || TimeOfDayError(s.clone());
        let (h, m) = s.trim().split_once(':').ok_or_else(bad)?;
        if h.is_empty() || h.len() > 2 || m.len() != 2 {
            return Err(bad());
        }
        let h: u16 = h.parse().map_err(|_| bad())?;
        let m: u16 = m.parse().map_err(|_| bad())?;
        TimeOfDay::new(h, m).ok_or_else(bad)
    }
}

impl From<TimeOfDay> for String {
    fn from(t: TimeOfDay) -> Self {
        format!("{:02}:{:02}", t.0 / 60, t.0 % 60)
    }
}

/// When automatic updates may be installed (local time). An end before the start spans midnight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateWindow {
    /// Days the window opens on; empty means every day.
    #[serde(default)]
    pub days: Vec<Weekday>,
    /// Opens at.
    pub start: TimeOfDay,
    /// Closes at.
    pub end: TimeOfDay,
}

/// "Now" in local time, as the service reads it from the OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalMoment {
    /// The day of the week.
    pub weekday: Weekday,
    /// The time of day.
    pub time: TimeOfDay,
}

impl UpdateWindow {
    /// Whether the window is open at `now`. For a window across midnight the day is the day it opened.
    pub fn contains(&self, now: LocalMoment) -> bool {
        let (s, e, t) = (self.start.minutes(), self.end.minutes(), now.time.minutes());
        let day_ok = |d: Weekday| self.days.is_empty() || self.days.contains(&d);
        if s == e {
            day_ok(now.weekday)
        } else if s < e {
            day_ok(now.weekday) && t >= s && t < e
        } else if t >= s {
            day_ok(now.weekday)
        } else {
            t < e && day_ok(previous(now.weekday))
        }
    }
}

fn previous(d: Weekday) -> Weekday {
    use Weekday::*;
    match d {
        Mon => Sun,
        Tue => Mon,
        Wed => Tue,
        Thu => Wed,
        Fri => Thu,
        Sat => Fri,
        Sun => Sat,
    }
}

/// Which releases are offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Channel {
    /// Stable releases only.
    #[default]
    Stable,
    /// Pre-releases too (release candidates).
    Prerelease,
}

/// Most skipped versions kept.
pub const MAX_SKIPPED: usize = 32;

/// The update policy of one runner (the defaults apply to every runner; a runner may override them).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdatePolicy {
    /// How updates are applied.
    pub mode: UpdateMode,
    /// Stay on this version (also to go back to it); `None` follows the latest release.
    pub pinned: Option<RunnerVersion>,
    /// When automatic updates may be installed; `None` is any time.
    pub window: Option<UpdateWindow>,
    /// Hours between scheduled checks (at least 1).
    pub check_every_hours: u32,
    /// Which releases are offered.
    pub channel: Channel,
    /// Versions the user chose to skip (never offered; a pin still wins).
    pub skipped: Vec<RunnerVersion>,
}

impl Default for UpdatePolicy {
    fn default() -> Self {
        UpdatePolicy {
            mode: UpdateMode::Automatic,
            pinned: None,
            window: None,
            check_every_hours: 24,
            channel: Channel::Stable,
            skipped: vec![],
        }
    }
}

/// What to do with the result of a check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UpdateDecision {
    /// Nothing to do.
    UpToDate,
    /// A version is available; show it (and `notify` the user).
    Available {
        /// The version to go to.
        target: RunnerVersion,
        /// Whether to send a notification.
        notify: bool,
    },
    /// Install `target` now (between jobs).
    Apply {
        /// The version to go to.
        target: RunnerVersion,
    },
    /// Install `target` when the update window opens.
    WaitForWindow {
        /// The version to go to.
        target: RunnerVersion,
    },
}

impl UpdatePolicy {
    /// This policy with `version` skipped (bounded: the oldest skips are forgotten first).
    pub fn skip(&self, version: RunnerVersion) -> UpdatePolicy {
        let mut p = self.clone();
        if !p.skipped.contains(&version) {
            p.skipped.push(version);
        }
        let extra = p.skipped.len().saturating_sub(MAX_SKIPPED);
        p.skipped.drain(..extra);
        p
    }

    /// Whether the service checks on a schedule.
    pub fn scheduled_checks(&self) -> bool {
        self.mode != UpdateMode::Manual
    }

    /// Decides what to do, given the installed version (`None`: not installed or unknown) and the latest release.
    /// A pinned version is the target even when it is older (that is how a user goes back); otherwise only newer,
    /// non-pre-release versions are offered.
    pub fn decide(&self, current: Option<&RunnerVersion>, latest: &RunnerVersion, now: LocalMoment) -> UpdateDecision {
        let target = match &self.pinned {
            Some(pin) if current == Some(pin) => return UpdateDecision::UpToDate,
            Some(pin) => pin.clone(),
            None if latest.is_prerelease() && self.channel == Channel::Stable => return UpdateDecision::UpToDate,
            None if self.skipped.contains(latest) => return UpdateDecision::UpToDate,
            None if current.is_some_and(|c| c >= latest) => return UpdateDecision::UpToDate,
            None => latest.clone(),
        };
        match self.mode {
            UpdateMode::Manual => UpdateDecision::Available { target, notify: false },
            UpdateMode::NotifyOnly => UpdateDecision::Available { target, notify: true },
            UpdateMode::Automatic => match &self.window {
                Some(w) if !w.contains(now) => UpdateDecision::WaitForWindow { target },
                _ => UpdateDecision::Apply { target },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> RunnerVersion {
        RunnerVersion::parse(s).unwrap()
    }

    fn t(s: &str) -> TimeOfDay {
        TimeOfDay::try_from(s.to_string()).unwrap()
    }

    fn at(weekday: Weekday, time: &str) -> LocalMoment {
        LocalMoment { weekday, time: t(time) }
    }

    #[test]
    fn channels_and_skipped_versions() {
        let now = at(Weekday::Mon, "12:00");
        let p = UpdatePolicy::default();
        assert_eq!(p.decide(Some(&v("5.1.0")), &v("5.3.0-rc.1"), now), UpdateDecision::UpToDate);
        let pre = UpdatePolicy {
            channel: Channel::Prerelease,
            ..UpdatePolicy::default()
        };
        assert_eq!(
            pre.decide(Some(&v("5.1.0")), &v("5.3.0-rc.1"), now),
            UpdateDecision::Apply { target: v("5.3.0-rc.1") }
        );
        let skipped = p.skip(v("5.2.0")).skip(v("5.2.0"));
        assert_eq!(skipped.skipped, vec![v("5.2.0")]);
        assert_eq!(skipped.decide(Some(&v("5.1.0")), &v("5.2.0"), now), UpdateDecision::UpToDate);
        assert_eq!(
            skipped.decide(Some(&v("5.1.0")), &v("5.2.1"), now),
            UpdateDecision::Apply { target: v("5.2.1") }
        );
        let mut many = UpdatePolicy::default();
        for i in 0..40 {
            many = many.skip(v(&format!("1.0.{i}")));
        }
        assert_eq!(many.skipped.len(), MAX_SKIPPED);
        assert_eq!(many.skipped[0], v("1.0.8"));
    }

    #[test]
    fn times_of_day_parse_strictly() {
        assert_eq!(t("02:30").minutes(), 150);
        assert_eq!(t("2:05").minutes(), 125);
        assert_eq!(String::from(t("23:59")), "23:59");
        for bad in ["", "24:00", "12:60", "12", "12:5", "ab:cd", "-1:00", "123:00"] {
            assert!(TimeOfDay::try_from(bad.to_string()).is_err(), "{bad}");
        }
    }

    #[test]
    fn windows_open_on_their_days_and_span_midnight() {
        let night = UpdateWindow {
            days: vec![Weekday::Sat],
            start: t("22:00"),
            end: t("04:00"),
        };
        assert!(night.contains(at(Weekday::Sat, "23:00")));
        assert!(
            night.contains(at(Weekday::Sun, "03:59")),
            "after midnight belongs to Saturday's window"
        );
        assert!(!night.contains(at(Weekday::Sun, "04:00")));
        assert!(!night.contains(at(Weekday::Sat, "03:00")), "Friday's night has no window");
        assert!(!night.contains(at(Weekday::Sun, "23:00")));

        let day = UpdateWindow {
            days: vec![],
            start: t("02:00"),
            end: t("05:00"),
        };
        assert!(day.contains(at(Weekday::Wed, "02:00")));
        assert!(!day.contains(at(Weekday::Wed, "05:00")));
        let always = UpdateWindow {
            days: vec![Weekday::Mon],
            start: t("00:00"),
            end: t("00:00"),
        };
        assert!(always.contains(at(Weekday::Mon, "13:00")));
        assert!(!always.contains(at(Weekday::Tue, "13:00")));
    }

    #[test]
    fn a_night_window_belongs_to_the_day_it_opens_on_every_weekday() {
        use Weekday::*;
        let week = [Mon, Tue, Wed, Thu, Fri, Sat, Sun];
        for (i, day) in week.iter().enumerate() {
            let next = week[(i + 1) % 7];
            let w = UpdateWindow {
                days: vec![*day],
                start: t("23:00"),
                end: t("02:00"),
            };
            assert!(w.contains(at(next, "01:00")), "{day:?}");
            assert!(!w.contains(at(*day, "01:00")), "{day:?}");
        }
    }

    #[test]
    fn automatic_updates_apply_inside_the_window_only() {
        let now = at(Weekday::Mon, "12:00");
        let p = UpdatePolicy::default();
        assert_eq!(
            p.decide(Some(&v("5.1.0")), &v("5.2.0"), now),
            UpdateDecision::Apply { target: v("5.2.0") }
        );
        assert_eq!(p.decide(None, &v("5.2.0"), now), UpdateDecision::Apply { target: v("5.2.0") });
        assert_eq!(p.decide(Some(&v("5.2.0")), &v("5.2.0"), now), UpdateDecision::UpToDate);
        assert_eq!(
            p.decide(Some(&v("5.3.0")), &v("5.2.0"), now),
            UpdateDecision::UpToDate,
            "never downgrades by itself"
        );
        let windowed = UpdatePolicy {
            window: Some(UpdateWindow {
                days: vec![],
                start: t("02:00"),
                end: t("04:00"),
            }),
            ..p
        };
        assert_eq!(
            windowed.decide(Some(&v("5.1.0")), &v("5.2.0"), now),
            UpdateDecision::WaitForWindow { target: v("5.2.0") }
        );
        assert_eq!(
            windowed.decide(Some(&v("5.1.0")), &v("5.2.0"), at(Weekday::Mon, "03:00")),
            UpdateDecision::Apply { target: v("5.2.0") }
        );
    }

    #[test]
    fn notify_only_and_manual_never_apply() {
        let now = at(Weekday::Mon, "12:00");
        let notify = UpdatePolicy {
            mode: UpdateMode::NotifyOnly,
            ..UpdatePolicy::default()
        };
        assert_eq!(
            notify.decide(Some(&v("5.1.0")), &v("5.2.0"), now),
            UpdateDecision::Available {
                target: v("5.2.0"),
                notify: true
            }
        );
        assert!(notify.scheduled_checks());
        let manual = UpdatePolicy {
            mode: UpdateMode::Manual,
            ..UpdatePolicy::default()
        };
        assert_eq!(
            manual.decide(Some(&v("5.1.0")), &v("5.2.0"), now),
            UpdateDecision::Available {
                target: v("5.2.0"),
                notify: false
            }
        );
        assert!(!manual.scheduled_checks());
    }

    #[test]
    fn pins_win_and_can_go_back() {
        let now = at(Weekday::Mon, "12:00");
        let pinned = UpdatePolicy {
            pinned: Some(v("5.1.0")),
            ..UpdatePolicy::default()
        };
        assert_eq!(pinned.decide(Some(&v("5.1.0")), &v("5.2.0"), now), UpdateDecision::UpToDate);
        assert_eq!(
            pinned.decide(Some(&v("5.2.0")), &v("5.2.0"), now),
            UpdateDecision::Apply { target: v("5.1.0") }
        );
    }

    #[test]
    fn pre_releases_are_never_offered_unless_pinned() {
        let now = at(Weekday::Mon, "12:00");
        let p = UpdatePolicy::default();
        assert_eq!(p.decide(Some(&v("5.2.0")), &v("5.3.0-rc.1"), now), UpdateDecision::UpToDate);
        let pinned = UpdatePolicy {
            pinned: Some(v("5.3.0-rc.1")),
            ..p
        };
        assert_eq!(
            pinned.decide(Some(&v("5.2.0")), &v("5.2.0"), now),
            UpdateDecision::Apply { target: v("5.3.0-rc.1") }
        );
    }
}
