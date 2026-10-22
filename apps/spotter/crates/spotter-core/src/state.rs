//! What a watch knows between checks, and how a check's result changes it. Pure: the caller passes the time and
//! carries out the returned [`Effects`] (notify, run actions, record events).

use crate::limits::{MAX_CHECK_LOG, MAX_HISTORY};
use crate::notify::{NotificationPolicy, NotifyMode};
use crate::release::Release;
use crate::source::SourceError;
use crate::time::{ClockTime, Timestamp};
use crate::version::Version;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// A watch's status after its last check.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", content = "message", rename_all = "kebab-case")]
pub enum WatchStatus {
    /// Not checked yet.
    #[default]
    Pending,
    /// The last check worked.
    Ok,
    /// The last check worked but nothing matched the filters.
    NoMatch,
    /// The place could not be reached (offline share, network down); retried with backoff.
    Unreachable(String),
    /// Credentials are missing or were refused.
    AuthNeeded(String),
    /// The plugin failed or is quarantined.
    PluginError(String),
    /// The watch's configuration is not valid (e.g. its plugin is not installed).
    Invalid(String),
    /// Any other failure.
    Failed(String),
}

impl WatchStatus {
    /// The status for a failed check.
    pub fn from_error(e: &SourceError) -> WatchStatus {
        let m = e.message().to_string();
        match e {
            SourceError::Unreachable(_) => WatchStatus::Unreachable(m),
            SourceError::Unauthorized(_) => WatchStatus::AuthNeeded(m),
            SourceError::Plugin(_) => WatchStatus::PluginError(m),
            SourceError::InvalidConfig(_) => WatchStatus::Invalid(m),
            SourceError::NotFound(_) | SourceError::InvalidResponse(_) | SourceError::Failed(_) => WatchStatus::Failed(m),
        }
    }

    /// Whether this is a failure.
    pub fn is_failure(&self) -> bool {
        !matches!(self, WatchStatus::Pending | WatchStatus::Ok | WatchStatus::NoMatch)
    }
}

/// A release in a watch's history, with when Spotter first saw it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnownRelease {
    /// The release.
    #[serde(flatten)]
    pub release: Release,
    /// When a check first found it.
    pub first_seen: Timestamp,
}

impl KnownRelease {
    /// When it came out, as far as Spotter knows: the published time, else when it was first seen.
    pub fn released_at(&self) -> Timestamp {
        self.release.published_at.unwrap_or(self.first_seen)
    }
}

/// One past check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckRecord {
    /// When it ran.
    pub at: Timestamp,
    /// How long it took, in milliseconds.
    pub took_ms: u64,
    /// `None` when it worked; the reason otherwise.
    pub error: Option<String>,
    /// How many releases it found.
    pub found: usize,
}

/// What Spotter remembers about one watch.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WatchState {
    /// The last status.
    pub status: WatchStatus,
    /// When it was last checked.
    pub last_check: Option<Timestamp>,
    /// When the last check that worked ran.
    pub last_success: Option<Timestamp>,
    /// When it is due next (set by the scheduler).
    pub next_check: Option<Timestamp>,
    /// Failures since the last success.
    pub failures: u32,
    /// The newest release.
    pub latest: Option<KnownRelease>,
    /// Known releases, newest first, at most [`MAX_HISTORY`].
    pub history: Vec<KnownRelease>,
    /// The newest version the user has seen (acknowledged); new watches start with the first latest version.
    pub acknowledged: Option<Version>,
    /// The version the user has installed, when known.
    pub installed: Option<Version>,
    /// The version the last notification was about.
    pub notified: Option<Version>,
    /// The version actions last ran for.
    pub actions_done: Option<Version>,
    /// Recent checks, newest last, at most [`MAX_CHECK_LOG`].
    pub checks: VecDeque<CheckRecord>,
}

/// The context of a check.
#[derive(Debug, Clone, Copy)]
pub struct CheckContext {
    /// Now.
    pub now: Timestamp,
    /// The local time of day (for quiet hours).
    pub local_time: ClockTime,
    /// Whether notifications are on and not paused app-wide.
    pub notifications_active: bool,
    /// How long the check took, in milliseconds.
    pub took_ms: u64,
}

/// What the caller does after a check.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Effects {
    /// Show a notification about this release.
    pub notify: Option<Release>,
    /// Run the watch's actions for this release.
    pub run_actions: Option<Release>,
    /// Releases the check found for the first time (newest first), for the event journal; empty on the first check.
    pub new_releases: Vec<Release>,
    /// The status changed (log it once, rather than at every failed check).
    pub status_changed: bool,
    /// The previous status, when it changed.
    pub previous_status: Option<WatchStatus>,
}

impl WatchState {
    /// Whether the newest release is newer than the one acknowledged (the tray shows this as "new").
    pub fn has_unseen(&self) -> bool {
        match (&self.latest, &self.acknowledged) {
            (Some(l), Some(a)) => l.release.version > *a,
            (Some(_), None) => true,
            (None, _) => false,
        }
    }

    /// Whether the newest release is newer than the installed one (`None` when the installed version is unknown).
    pub fn newer_than_installed(&self) -> Option<bool> {
        Some(self.latest.as_ref()?.release.version > *self.installed.as_ref()?)
    }

    /// Applies a check that worked. `releases` come from [`crate::release::select`]: newest first.
    pub fn on_success(&mut self, releases: Vec<Release>, policy: &NotificationPolicy, cx: CheckContext) -> Effects {
        let mut fx = Effects::default();
        let first_check = self.latest.is_none() && self.history.is_empty();
        let new_status = if releases.is_empty() {
            WatchStatus::NoMatch
        } else {
            WatchStatus::Ok
        };
        self.set_status(new_status, &mut fx);
        self.failures = 0;
        self.last_check = Some(cx.now);
        self.last_success = Some(cx.now);
        self.log(CheckRecord {
            at: cx.now,
            took_ms: cx.took_ms,
            error: None,
            found: releases.len(),
        });
        let Some(newest) = releases.first().cloned() else {
            return fx;
        };
        // Merge into the history.
        for r in &releases {
            if !self.history.iter().any(|k| k.release.version == r.version) {
                if !first_check {
                    fx.new_releases.push(r.clone());
                }
                self.history.push(KnownRelease {
                    release: r.clone(),
                    first_seen: cx.now,
                });
            }
        }
        self.history.sort_by(|a, b| b.release.version.cmp(&a.release.version));
        self.history.truncate(MAX_HISTORY);
        let previous = self.latest.as_ref().map(|k| k.release.version.clone());
        let first_seen = self
            .history
            .iter()
            .find(|k| k.release.version == newest.version)
            .map_or(cx.now, |k| k.first_seen);
        self.latest = Some(KnownRelease {
            release: newest.clone(),
            first_seen,
        });
        if first_check && self.acknowledged.is_none() {
            // The first check sets the baseline: what exists now is not news.
            self.acknowledged = Some(newest.version.clone());
            return fx;
        }
        let v = &newest.version;
        let is_new = previous.as_ref().is_none_or(|p| v > p);
        let unseen = self.acknowledged.as_ref().is_none_or(|a| v > a);
        let wanted = match policy.mode {
            NotifyMode::Off => false,
            NotifyMode::EveryNew => unseen,
            NotifyMode::NewerThanInstalled => unseen && self.installed.as_ref().is_none_or(|i| v > i),
        };
        let repeat = self.notified.as_ref() == Some(v);
        let quiet = policy.quiet_hours.is_some_and(|q| q.contains(cx.local_time));
        if wanted && cx.notifications_active && !quiet && (!repeat || policy.nag) {
            self.notified = Some(v.clone());
            fx.notify = Some(newest.clone());
        }
        let newer_than_installed = self.installed.as_ref().is_none_or(|i| v > i);
        if is_new && unseen && newer_than_installed && self.actions_done.as_ref() != Some(v) {
            self.actions_done = Some(v.clone());
            fx.run_actions = Some(newest);
        }
        fx
    }

    /// Applies a failed check.
    pub fn on_failure(&mut self, error: &SourceError, cx: CheckContext) -> Effects {
        let mut fx = Effects::default();
        self.set_status(WatchStatus::from_error(error), &mut fx);
        self.failures = self.failures.saturating_add(1);
        self.last_check = Some(cx.now);
        self.log(CheckRecord {
            at: cx.now,
            took_ms: cx.took_ms,
            error: Some(error.message().to_string()),
            found: 0,
        });
        fx
    }

    /// Marks a configuration problem (no check ran).
    pub fn set_invalid(&mut self, message: String) -> bool {
        let mut fx = Effects::default();
        self.set_status(WatchStatus::Invalid(message), &mut fx);
        fx.status_changed
    }

    /// The user has seen `version` (the latest when `None`). Stops repeated notifications for it.
    pub fn acknowledge(&mut self, version: Option<Version>) {
        if let Some(v) = version.or_else(|| self.latest.as_ref().map(|k| k.release.version.clone())) {
            self.acknowledged = Some(v);
        }
    }

    /// Forgets the acknowledgement (the old tray "Reset"): the latest release counts as new again.
    pub fn reset_acknowledgement(&mut self) {
        self.acknowledged = None;
        self.notified = None;
    }

    /// Sets (or clears) the installed version.
    pub fn set_installed(&mut self, version: Option<Version>) {
        self.installed = version;
    }

    fn set_status(&mut self, status: WatchStatus, fx: &mut Effects) {
        if self.status != status {
            fx.status_changed = true;
            fx.previous_status = Some(std::mem::replace(&mut self.status, status));
        }
    }

    fn log(&mut self, record: CheckRecord) {
        self.checks.push_back(record);
        while self.checks.len() > MAX_CHECK_LOG {
            self.checks.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::QuietHours;
    use crate::version::VersionScheme;

    fn rel(v: &str) -> Release {
        Release {
            version: VersionScheme::Semver.extract(v).unwrap(),
            name: format!("app-{v}"),
            location: format!("/d/app-{v}"),
            published_at: None,
            size: None,
            sha256: None,
        }
    }

    fn cx(now: i64) -> CheckContext {
        CheckContext {
            now: Timestamp(now),
            local_time: ClockTime::new(12, 0).unwrap(),
            notifications_active: true,
            took_ms: 5,
        }
    }

    fn ver(v: &str) -> Version {
        VersionScheme::Semver.extract(v).unwrap()
    }

    #[test]
    fn the_first_check_is_a_baseline() {
        let mut s = WatchState::default();
        let fx = s.on_success(vec![rel("1.1.0"), rel("1.0.0")], &NotificationPolicy::default(), cx(1));
        assert_eq!(fx.notify, None);
        assert_eq!(fx.run_actions, None);
        assert!(fx.new_releases.is_empty() && fx.status_changed);
        assert_eq!(s.acknowledged, Some(ver("1.1.0")));
        assert!(!s.has_unseen());
        assert_eq!(s.history.len(), 2);
        assert_eq!(s.status, WatchStatus::Ok);
    }

    #[test]
    fn a_new_release_notifies_once_and_runs_actions_once() {
        let p = NotificationPolicy::default();
        let mut s = WatchState::default();
        s.on_success(vec![rel("1.0.0")], &p, cx(1));
        let fx = s.on_success(vec![rel("1.1.0"), rel("1.0.0")], &p, cx(2));
        assert_eq!(fx.notify.as_ref().map(|r| r.name.as_str()), Some("app-1.1.0"));
        assert!(fx.run_actions.is_some());
        assert_eq!(fx.new_releases.len(), 1);
        assert!(!fx.status_changed);
        assert!(s.has_unseen());
        let again = s.on_success(vec![rel("1.1.0"), rel("1.0.0")], &p, cx(3));
        assert_eq!((again.notify, again.run_actions), (None, None), "not naggy: once per version");
        s.acknowledge(None);
        assert!(!s.has_unseen());
        assert_eq!(s.latest.as_ref().unwrap().first_seen, Timestamp(2));
    }

    #[test]
    fn nag_repeats_until_acknowledged() {
        let p = NotificationPolicy {
            nag: true,
            ..NotificationPolicy::default()
        };
        let mut s = WatchState::default();
        s.on_success(vec![rel("1.0.0")], &p, cx(1));
        assert!(s.on_success(vec![rel("1.1.0")], &p, cx(2)).notify.is_some());
        assert!(s.on_success(vec![rel("1.1.0")], &p, cx(3)).notify.is_some());
        s.acknowledge(Some(ver("1.1.0")));
        assert!(s.on_success(vec![rel("1.1.0")], &p, cx(4)).notify.is_none());
        s.reset_acknowledgement();
        assert!(
            s.on_success(vec![rel("1.1.0")], &p, cx(5)).notify.is_some(),
            "after a reset the latest is new again"
        );
    }

    #[test]
    fn quiet_hours_and_global_pause_defer_notifications() {
        let p = NotificationPolicy {
            quiet_hours: Some(QuietHours {
                start: ClockTime::new(11, 0).unwrap(),
                end: ClockTime::new(13, 0).unwrap(),
            }),
            ..NotificationPolicy::default()
        };
        let mut s = WatchState::default();
        s.on_success(vec![rel("1.0.0")], &p, cx(1));
        let quiet = s.on_success(vec![rel("1.1.0")], &p, cx(2));
        assert!(
            quiet.notify.is_none() && quiet.run_actions.is_some(),
            "actions do not wait for quiet hours"
        );
        let mut later = cx(3);
        later.notifications_active = false;
        assert!(s.on_success(vec![rel("1.1.0")], &p, later).notify.is_none());
        let mut after = cx(4);
        after.local_time = ClockTime::new(13, 0).unwrap();
        assert!(
            s.on_success(vec![rel("1.1.0")], &p, after).notify.is_some(),
            "delivered after quiet hours"
        );
    }

    #[test]
    fn modes_off_and_newer_than_installed() {
        let mut s = WatchState::default();
        let off = NotificationPolicy {
            mode: NotifyMode::Off,
            ..NotificationPolicy::default()
        };
        s.on_success(vec![rel("1.0.0")], &off, cx(1));
        assert!(s.on_success(vec![rel("1.1.0")], &off, cx(2)).notify.is_none());

        let inst = NotificationPolicy {
            mode: NotifyMode::NewerThanInstalled,
            ..NotificationPolicy::default()
        };
        let mut s = WatchState::default();
        s.on_success(vec![rel("1.0.0")], &inst, cx(1));
        s.set_installed(Some(ver("2.0.0")));
        let fx = s.on_success(vec![rel("1.5.0")], &inst, cx(2));
        assert!(fx.notify.is_none() && fx.run_actions.is_none(), "older than installed");
        assert_eq!(s.newer_than_installed(), Some(false));
        let fx = s.on_success(vec![rel("2.1.0")], &inst, cx(3));
        assert!(fx.notify.is_some() && fx.run_actions.is_some());
        assert_eq!(s.newer_than_installed(), Some(true));
        s.set_installed(None);
        assert_eq!(s.newer_than_installed(), None);
    }

    #[test]
    fn failures_count_change_status_once_and_recover() {
        let mut s = WatchState::default();
        let e = SourceError::Unreachable("share offline".into());
        let fx = s.on_failure(&e, cx(1));
        assert!(fx.status_changed && fx.previous_status == Some(WatchStatus::Pending));
        assert!(!s.on_failure(&e, cx(2)).status_changed, "the same failure is not news");
        assert_eq!(s.failures, 2);
        assert!(s.status.is_failure());
        let fx = s.on_success(vec![], &NotificationPolicy::default(), cx(3));
        assert_eq!(s.status, WatchStatus::NoMatch);
        assert!(fx.status_changed && s.failures == 0 && !s.status.is_failure());
        assert!(!s.has_unseen());
        assert!(s.set_invalid("plugin missing".into()));
        assert!(!s.set_invalid("plugin missing".into()));
        assert_eq!(s.checks.back().unwrap().found, 0);
    }

    #[test]
    fn statuses_map_from_errors() {
        let cases = [
            (SourceError::Unreachable("m".into()), "Unreachable"),
            (SourceError::Unauthorized("m".into()), "AuthNeeded"),
            (SourceError::Plugin("m".into()), "PluginError"),
            (SourceError::InvalidConfig("m".into()), "Invalid"),
            (SourceError::NotFound("m".into()), "Failed"),
            (SourceError::InvalidResponse("m".into()), "Failed"),
            (SourceError::Failed("m".into()), "Failed"),
        ];
        for (e, name) in cases {
            assert!(format!("{:?}", WatchStatus::from_error(&e)).starts_with(name));
        }
        let json = serde_json::to_string(&WatchStatus::Unreachable("x".into())).unwrap();
        assert_eq!(json, r#"{"kind":"unreachable","message":"x"}"#);
    }

    #[test]
    fn history_and_check_log_are_bounded_and_withdrawn_releases_lower_the_latest() {
        let p = NotificationPolicy::default();
        let mut s = WatchState::default();
        let many: Vec<Release> = (0..MAX_HISTORY + 20).rev().map(|i| rel(&format!("1.{i}.0"))).collect();
        s.on_success(many, &p, cx(1));
        assert_eq!(s.history.len(), MAX_HISTORY);
        for i in 0..MAX_CHECK_LOG + 5 {
            s.on_success(vec![rel("1.219.0")], &p, cx(2 + i as i64));
        }
        assert_eq!(s.checks.len(), MAX_CHECK_LOG);
        s.on_success(vec![rel("1.5.0")], &p, cx(100));
        assert_eq!(s.latest.as_ref().unwrap().release.version, ver("1.5.0"));
        let mut k = s.latest.clone().unwrap();
        assert_eq!(k.released_at(), k.first_seen);
        k.release.published_at = Some(Timestamp(7));
        assert_eq!(k.released_at(), Timestamp(7));
        s.acknowledged = None;
        assert!(s.has_unseen());
    }
}
