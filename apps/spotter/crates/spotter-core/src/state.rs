//! What a watch knows between checks, and how a check's result changes it. Pure: the caller passes the time and
//! carries out the returned [`Effects`] (notify, run actions, record events).

use crate::filter::{self, VersionFilter};
use crate::limits::{MAX_CHECK_LOG, MAX_HISTORY};
use crate::notify::{NotificationPolicy, NotifyMode};
use crate::release::Release;
use crate::source::SourceError;
use crate::time::{ClockTime, Timestamp};
use crate::tracking::ProductsSpec;
use crate::version::Version;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

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
    /// A page asked for a login or a captcha; the user has to deal with it in the page.
    NeedsYou(String),
    /// The watch's configuration is not valid (e.g. an unknown source kind).
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
            SourceError::NeedsYou(_) => WatchStatus::NeedsYou(m),
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
    /// Versions seen but not yet announced because of the soak time, by version text: when each was first seen.
    pub soaking: BTreeMap<String, Timestamp>,
    /// For a watch whose pattern has an `id` group: the state of each product (its own latest, new state, soak and
    /// notification). Empty otherwise. The fields above then summarise all products.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub products: BTreeMap<String, WatchState>,
}

/// Most products one watch keeps state for; more are ignored (a pattern that matches too much).
pub const MAX_PRODUCTS: usize = 200;
/// Releases kept per product.
pub const MAX_PRODUCT_HISTORY: usize = 30;

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
    /// Show a notification about each of these releases (one per product at most).
    pub notify: Vec<Release>,
    /// Run the watch's actions for each of these releases.
    pub run_actions: Vec<Release>,
    /// Products that appeared after the first check and are announced (only when the watch asks for it).
    pub new_products: Vec<String>,
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
        if !self.products.is_empty() {
            return self.products.values().any(WatchState::has_unseen);
        }
        match (&self.latest, &self.acknowledged) {
            (Some(l), Some(a)) => l.release.version > *a,
            (Some(_), None) => true,
            (None, _) => false,
        }
    }

    /// Whether the newest release is newer than the installed one (`None` when the installed version is unknown).
    pub fn newer_than_installed(&self) -> Option<bool> {
        if !self.products.is_empty() {
            let each: Vec<bool> = self.products.values().filter_map(WatchState::newer_than_installed).collect();
            return (!each.is_empty()).then(|| each.contains(&true));
        }
        Some(self.latest.as_ref()?.release.version > *self.installed.as_ref()?)
    }

    /// Applies a check that worked. `releases` come from [`crate::release::select`]: newest first.
    pub fn on_success(&mut self, releases: Vec<Release>, policy: &NotificationPolicy, cx: CheckContext) -> Effects {
        let mut fx = Effects::default();
        self.begin_success(&releases, cx, &mut fx);
        self.merge_releases(releases, policy, cx, true, &mut fx);
        fx
    }

    /// Applies a check that worked for a watch with products: `by_product` has the releases of each product
    /// (newest first), `soak` the watch's soak time (a product's own setting replaces it). A product seen for the
    /// first time is a baseline, unless the watch announces new products and this is not the first check.
    pub fn on_success_products(
        &mut self,
        by_product: BTreeMap<String, Vec<Release>>,
        policy: &NotificationPolicy,
        products: &ProductsSpec,
        soak: &VersionFilter,
        cx: CheckContext,
    ) -> Effects {
        let mut fx = Effects::default();
        let total: Vec<Release> = by_product.values().flatten().cloned().collect();
        let first_check = self.products.is_empty() && self.history.is_empty();
        self.begin_success(&total, cx, &mut fx);
        for (id, releases) in by_product {
            if !self.products.contains_key(&id) && self.products.len() >= MAX_PRODUCTS {
                continue;
            }
            let sub = self.products.entry(id.clone()).or_default();
            let new_product = sub.latest.is_none() && sub.history.is_empty();
            let announce = new_product && !first_check && products.announce_new;
            let own_soak = VersionFilter {
                soak_hours: products.soak_hours(&id, soak.soak_hours),
                ..soak.clone()
            };
            let sub_first = sub.latest.is_none() && sub.history.is_empty() && !announce;
            let history = sub.history.clone();
            let soaked = filter::soak(releases, &own_soak, &history, sub_first, &mut sub.soaking, cx.now);
            let mut own_policy = *policy;
            if products.is_muted(&id) {
                own_policy.mode = NotifyMode::Off;
            }
            let mut sub_fx = Effects::default();
            sub.merge_releases(soaked.ready, &own_policy, cx, !announce, &mut sub_fx);
            sub.history.truncate(MAX_PRODUCT_HISTORY);
            if announce && !sub_fx.new_releases.is_empty() {
                fx.new_products.push(id.clone());
            }
            fx.notify.append(&mut sub_fx.notify);
            fx.run_actions.append(&mut sub_fx.run_actions);
            fx.new_releases.append(&mut sub_fx.new_releases);
        }
        self.summarise_products();
        fx
    }

    fn begin_success(&mut self, releases: &[Release], cx: CheckContext, fx: &mut Effects) {
        let new_status = if releases.is_empty() {
            WatchStatus::NoMatch
        } else {
            WatchStatus::Ok
        };
        self.set_status(new_status, fx);
        self.failures = 0;
        self.last_check = Some(cx.now);
        self.last_success = Some(cx.now);
        self.log(CheckRecord {
            at: cx.now,
            took_ms: cx.took_ms,
            error: None,
            found: releases.len(),
        });
    }

    /// The summary fields of a watch with products: the most recently found release is the latest, the history is
    /// all products' releases merged.
    fn summarise_products(&mut self) {
        let mut all: Vec<KnownRelease> = self.products.values().flat_map(|p| p.history.iter().cloned()).collect();
        all.sort_by(|a, b| {
            b.first_seen
                .cmp(&a.first_seen)
                .then_with(|| b.release.version.cmp(&a.release.version))
                .then_with(|| a.release.detail.product.cmp(&b.release.detail.product))
        });
        all.truncate(MAX_HISTORY);
        self.latest = all.first().cloned();
        self.history = all;
    }

    fn merge_releases(&mut self, releases: Vec<Release>, policy: &NotificationPolicy, cx: CheckContext, baseline: bool, fx: &mut Effects) {
        let first_check = baseline && self.latest.is_none() && self.history.is_empty();
        let Some(newest) = releases.first().cloned() else {
            return;
        };
        // Merge into the history.
        for r in &releases {
            match self.history.iter_mut().find(|k| k.release.version == r.version) {
                Some(known) => {
                    // A late artifact joins the release it belongs to; nobody is told twice.
                    if !r.detail.artifacts.is_empty() && known.release.detail.artifacts != r.detail.artifacts {
                        known.release.detail.artifacts = r.detail.artifacts.clone();
                        known.release.name = r.name.clone();
                        known.release.location = r.location.clone();
                        known.release.size = r.size;
                        known.release.published_at = r.published_at;
                    }
                }
                None => {
                    let mut release = r.clone();
                    // The same file name with new contents: say so.
                    release.detail.updated_in_place = !release.detail.artifacts.is_empty()
                        && self
                            .history
                            .iter()
                            .any(|k| k.release.detail.product == release.detail.product && k.release.name == release.name);
                    if !first_check {
                        fx.new_releases.push(release.clone());
                    }
                    self.history.push(KnownRelease {
                        release,
                        first_seen: cx.now,
                    });
                }
            }
        }
        self.history.sort_by(|a, b| b.release.version.cmp(&a.release.version));
        self.history.truncate(MAX_HISTORY);
        let previous = self.latest.as_ref().map(|k| k.release.version.clone());
        let known_newest = self.history.iter().find(|k| k.release.version == newest.version);
        let first_seen = known_newest.map_or(cx.now, |k| k.first_seen);
        let newest = known_newest.map_or(newest, |k| k.release.clone());
        self.latest = Some(KnownRelease {
            release: newest.clone(),
            first_seen,
        });
        if first_check && self.acknowledged.is_none() {
            // The first check sets the baseline: what exists now is not news.
            self.acknowledged = Some(newest.version.clone());
            return;
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
            fx.notify.push(newest.clone());
        }
        let newer_than_installed = self.installed.as_ref().is_none_or(|i| v > i);
        if is_new && unseen && newer_than_installed && self.actions_done.as_ref() != Some(v) {
            self.actions_done = Some(v.clone());
            fx.run_actions.push(newest);
        }
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
        if !self.products.is_empty() && version.is_none() {
            self.products.values_mut().for_each(|p| p.acknowledge(None));
            return;
        }
        if let Some(v) = version.or_else(|| self.latest.as_ref().map(|k| k.release.version.clone())) {
            self.acknowledged = Some(v);
        }
    }

    /// The user has seen the latest release of one product.
    pub fn acknowledge_product(&mut self, product: &str) {
        if let Some(p) = self.products.get_mut(product) {
            p.acknowledge(None);
        }
    }

    /// Forgets the acknowledgement (the old tray "Reset"): the latest release counts as new again.
    pub fn reset_acknowledgement(&mut self) {
        self.products.values_mut().for_each(WatchState::reset_acknowledgement);
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
            detail: Default::default(),
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
        assert!(fx.notify.is_empty());
        assert!(fx.run_actions.is_empty());
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
        assert_eq!(fx.notify.first().map(|r| r.name.as_str()), Some("app-1.1.0"));
        assert!(!fx.run_actions.is_empty());
        assert_eq!(fx.new_releases.len(), 1);
        assert!(!fx.status_changed);
        assert!(s.has_unseen());
        let again = s.on_success(vec![rel("1.1.0"), rel("1.0.0")], &p, cx(3));
        assert!(
            again.notify.is_empty() && again.run_actions.is_empty(),
            "not naggy: once per version"
        );
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
        assert!(!s.on_success(vec![rel("1.1.0")], &p, cx(2)).notify.is_empty());
        assert!(!s.on_success(vec![rel("1.1.0")], &p, cx(3)).notify.is_empty());
        s.acknowledge(Some(ver("1.1.0")));
        assert!(s.on_success(vec![rel("1.1.0")], &p, cx(4)).notify.is_empty());
        s.reset_acknowledgement();
        assert!(
            s.on_success(vec![rel("1.1.0")], &p, cx(5))
                .notify
                .is_empty()
                .then_some(())
                .is_none(),
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
            quiet.notify.is_empty() && !quiet.run_actions.is_empty(),
            "actions do not wait for quiet hours"
        );
        let mut later = cx(3);
        later.notifications_active = false;
        assert!(s.on_success(vec![rel("1.1.0")], &p, later).notify.is_empty());
        let mut after = cx(4);
        after.local_time = ClockTime::new(13, 0).unwrap();
        assert!(
            !s.on_success(vec![rel("1.1.0")], &p, after).notify.is_empty(),
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
        assert!(s.on_success(vec![rel("1.1.0")], &off, cx(2)).notify.is_empty());

        let inst = NotificationPolicy {
            mode: NotifyMode::NewerThanInstalled,
            ..NotificationPolicy::default()
        };
        let mut s = WatchState::default();
        s.on_success(vec![rel("1.0.0")], &inst, cx(1));
        s.set_installed(Some(ver("2.0.0")));
        let fx = s.on_success(vec![rel("1.5.0")], &inst, cx(2));
        assert!(fx.notify.is_empty() && fx.run_actions.is_empty(), "older than installed");
        assert_eq!(s.newer_than_installed(), Some(false));
        let fx = s.on_success(vec![rel("2.1.0")], &inst, cx(3));
        assert!(!fx.notify.is_empty() && !fx.run_actions.is_empty());
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
            (SourceError::InvalidConfig("m".into()), "Invalid"),
            (SourceError::NotFound("m".into()), "Failed"),
            (SourceError::InvalidResponse("m".into()), "Failed"),
            (SourceError::Failed("m".into()), "Failed"),
            (SourceError::NeedsYou("m".into()), "NeedsYou"),
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

    fn prod(product: &str, build: u64, name: &str) -> Release {
        let mut r = rel(&format!("1.{build}.0"));
        r.name = name.into();
        r.detail.product = Some(product.into());
        r.detail.artifacts = vec![crate::release::Artifact {
            name: name.into(),
            location: format!("/d/{name}"),
            kind: "iso".into(),
            variant: String::new(),
            size: Some(1),
            modified: None,
            created: None,
            checksum_file: None,
        }];
        r
    }

    fn by(items: Vec<Release>) -> BTreeMap<String, Vec<Release>> {
        let mut m: BTreeMap<String, Vec<Release>> = BTreeMap::new();
        for r in items {
            m.entry(r.detail.product.clone().unwrap()).or_default().push(r);
        }
        m
    }

    #[test]
    fn products_have_their_own_latest_new_state_and_notification() {
        let (p, spec, soak) = (NotificationPolicy::default(), ProductsSpec::default(), VersionFilter::default());
        let mut s = WatchState::default();
        let fx = s.on_success_products(by(vec![prod("PRO", 1, "pro1"), prod("QA", 4, "qa4")]), &p, &spec, &soak, cx(1));
        assert!(
            fx.notify.is_empty() && fx.new_releases.is_empty(),
            "the first check is the baseline of every product"
        );
        assert!(!s.has_unseen());
        assert_eq!(s.products.len(), 2);
        // PRO gets a new release; QA does not.
        let fx = s.on_success_products(
            by(vec![prod("PRO", 2, "pro2"), prod("PRO", 1, "pro1"), prod("QA", 4, "qa4")]),
            &p,
            &spec,
            &soak,
            cx(2),
        );
        assert_eq!(fx.notify.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["pro2"]);
        assert_eq!(fx.run_actions.len(), 1);
        assert!(s.has_unseen() && s.products["PRO"].has_unseen() && !s.products["QA"].has_unseen());
        assert_eq!(s.latest.as_ref().unwrap().release.name, "pro2");
        assert_eq!(s.history.len(), 3);
        s.acknowledge_product("QA");
        assert!(s.products["PRO"].has_unseen());
        s.acknowledge(None);
        assert!(!s.has_unseen());
        s.reset_acknowledgement();
        assert!(s.has_unseen());
        s.acknowledge_product("none");
        // Round trip through JSON, and a state without products still loads.
        let back: WatchState = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        assert!(serde_json::from_str::<WatchState>("{}").unwrap().products.is_empty());
        assert!(!serde_json::to_string(&WatchState::default()).unwrap().contains("products"));
    }

    #[test]
    fn a_new_product_is_a_baseline_unless_the_watch_announces_it() {
        let (p, soak) = (NotificationPolicy::default(), VersionFilter::default());
        let quiet = ProductsSpec::default();
        let loud = ProductsSpec {
            announce_new: true,
            ..ProductsSpec::default()
        };
        for (spec, announced) in [(&quiet, false), (&loud, true)] {
            let mut s = WatchState::default();
            s.on_success_products(by(vec![prod("PRO", 1, "pro1")]), &p, spec, &soak, cx(1));
            let fx = s.on_success_products(by(vec![prod("PRO", 1, "pro1"), prod("NEW", 7, "new7")]), &p, spec, &soak, cx(2));
            assert_eq!(!fx.notify.is_empty(), announced);
            assert_eq!(fx.new_products, if announced { vec!["NEW".to_string()] } else { Vec::new() });
            assert_eq!(s.products.len(), 2);
        }
    }

    #[test]
    fn products_can_be_muted_and_soaked_on_their_own() {
        use crate::tracking::ProductSetting;
        let p = NotificationPolicy::default();
        let mut spec = ProductsSpec::default();
        spec.settings.insert(
            "QA".into(),
            ProductSetting {
                muted: true,
                ..ProductSetting::default()
            },
        );
        spec.settings.insert(
            "PRO".into(),
            ProductSetting {
                soak_hours: Some(2),
                ..ProductSetting::default()
            },
        );
        let soak = VersionFilter::default();
        let mut s = WatchState::default();
        s.on_success_products(by(vec![prod("PRO", 1, "pro1"), prod("QA", 1, "qa1")]), &p, &spec, &soak, cx(1));
        let new = by(vec![
            prod("PRO", 2, "pro2"),
            prod("PRO", 1, "pro1"),
            prod("QA", 2, "qa2"),
            prod("QA", 1, "qa1"),
        ]);
        let fx = s.on_success_products(new.clone(), &p, &spec, &soak, cx(1_000));
        assert!(fx.notify.is_empty(), "QA is muted and PRO is still soaking");
        assert_eq!(
            fx.run_actions.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["qa2"],
            "muted is not paused"
        );
        let fx = s.on_success_products(new, &p, &spec, &soak, cx(3 * 3_600_000));
        assert_eq!(fx.notify.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["pro2"]);
    }

    #[test]
    fn late_artifacts_join_a_release_and_in_place_updates_are_marked() {
        let p = NotificationPolicy::default();
        let mut s = WatchState::default();
        s.on_success(vec![prod("PRO", 1, "pro1")], &p, cx(1));
        let mut richer = prod("PRO", 2, "pro2.iso");
        let fx = s.on_success(vec![richer.clone(), prod("PRO", 1, "pro1")], &p, cx(2));
        assert_eq!(fx.notify.len(), 1);
        let mut exe = richer.detail.artifacts[0].clone();
        exe.name = "pro2.exe".into();
        exe.kind = "exe".into();
        richer.detail.artifacts.insert(0, exe);
        richer.name = "pro2.exe".into();
        let fx = s.on_success(vec![richer, prod("PRO", 1, "pro1")], &p, cx(3));
        assert!(fx.notify.is_empty() && fx.new_releases.is_empty(), "a late artifact is not news");
        let known = s.history.iter().find(|k| k.release.version == ver("1.2.0")).unwrap();
        assert_eq!(known.release.detail.artifacts.len(), 2);
        assert_eq!(known.release.name, "pro2.exe");
        // The same name with a newer stamp.
        let mut again = prod("PRO", 3, "pro2.exe");
        again.version = ver("1.3.0");
        let fx = s.on_success(vec![again], &p, cx(4));
        assert!(fx.new_releases[0].detail.updated_in_place);
        assert!(s.latest.as_ref().unwrap().release.detail.updated_in_place);
    }
}
