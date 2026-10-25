//! Version filters: a prefix (`1.` keeps the 1.x line) and soak time (a release is announced only after it has
//! existed for a number of hours, so a bad release that is pulled quickly never bothers anyone). Pure: the caller
//! passes the time and keeps the `soaking` map in the watch's state.
use crate::release::Release;
use crate::state::KnownRelease;
use crate::time::Timestamp;
use crate::version::Version;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Longest soak time (30 days).
pub const MAX_SOAK_HOURS: u32 = 24 * 30;
/// Longest prefix.
pub const MAX_PREFIX_LEN: usize = 64;

/// Narrows which versions count.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct VersionFilter {
    /// Only versions starting with this (`1.`, `2.4`), ignoring a leading `v`; empty for all.
    #[serde(default)]
    pub prefix: String,
    /// Hours a release must exist before it is announced; 0 for none.
    #[serde(default)]
    pub soak_hours: u32,
}

impl VersionFilter {
    /// Checks the filter; the message says what to fix.
    pub fn check(&self) -> Result<(), String> {
        if self.prefix.len() > MAX_PREFIX_LEN {
            return Err(format!("The version prefix is too long (at most {MAX_PREFIX_LEN} characters)."));
        }
        if self.soak_hours > MAX_SOAK_HOURS {
            return Err(format!("The soak time can be at most {MAX_SOAK_HOURS} hours (30 days)."));
        }
        Ok(())
    }

    /// Whether `version` has the prefix (always when there is none).
    pub fn matches(&self, version: &Version) -> bool {
        let prefix = trim_v(self.prefix.trim());
        prefix.is_empty() || trim_v(&version.text).starts_with(prefix)
    }

    /// Keeps the releases whose version has the prefix.
    pub fn apply_prefix(&self, releases: Vec<Release>) -> Vec<Release> {
        releases.into_iter().filter(|r| self.matches(&r.version)).collect()
    }

    /// The soak time in milliseconds.
    pub fn soak_ms(&self) -> i64 {
        i64::from(self.soak_hours.min(MAX_SOAK_HOURS)) * 3_600_000
    }
}

fn trim_v(s: &str) -> &str {
    s.strip_prefix(['v', 'V']).unwrap_or(s)
}

/// What [`soak`] decided.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Soaked {
    /// The releases to hand to the watch state (newest first): known ones and those that have soaked.
    pub ready: Vec<Release>,
    /// How many new releases are still waiting.
    pub waiting: usize,
}

/// Holds back releases that are new and younger than the soak time. `history` is what the watch already announced
/// (those pass at once); `first_check` is true while the watch has no history (the baseline is never held back).
/// `soaking` remembers when each waiting version was first seen and is pruned to the versions still offered.
pub fn soak(
    releases: Vec<Release>,
    filter: &VersionFilter,
    history: &[KnownRelease],
    first_check: bool,
    soaking: &mut BTreeMap<String, Timestamp>,
    now: Timestamp,
) -> Soaked {
    if filter.soak_hours == 0 || first_check {
        soaking.clear();
        return Soaked {
            ready: releases,
            waiting: 0,
        };
    }
    let offered: Vec<String> = releases.iter().map(|r| r.version.text.clone()).collect();
    soaking.retain(|k, _| offered.contains(k));
    let mut out = Soaked::default();
    for r in releases {
        if history.iter().any(|k| k.release.version == r.version) {
            out.ready.push(r);
            continue;
        }
        let first = *soaking.entry(r.version.text.clone()).or_insert(now);
        if now.0.saturating_sub(first.0) >= filter.soak_ms() {
            soaking.remove(&r.version.text);
            out.ready.push(r);
        } else {
            out.waiting += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::VersionScheme;

    fn rel(v: &str) -> Release {
        Release {
            version: VersionScheme::Semver.extract(v).unwrap(),
            name: v.into(),
            location: "l".into(),
            published_at: None,
            size: None,
            sha256: None,
            detail: Default::default(),
        }
    }

    fn known(v: &str) -> KnownRelease {
        KnownRelease {
            release: rel(v),
            first_seen: Timestamp(0),
        }
    }

    #[test]
    fn prefixes_ignore_a_leading_v() {
        let f = VersionFilter {
            prefix: "v1.".into(),
            soak_hours: 0,
        };
        let kept = f.apply_prefix(vec![rel("1.4.0"), rel("v2.0.0"), rel("v1.9.1")]);
        assert_eq!(kept.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["1.4.0", "v1.9.1"]);
        assert!(VersionFilter::default().matches(&rel("9.9.9").version));
        assert!(!f.matches(&rel("10.0.0").version), "1. is not 10.");
    }

    #[test]
    fn filters_are_bounded() {
        assert!(VersionFilter::default().check().is_ok());
        let long = VersionFilter {
            prefix: "1".repeat(MAX_PREFIX_LEN + 1),
            soak_hours: 0,
        };
        assert!(long.check().unwrap_err().contains("too long"));
        let slow = VersionFilter {
            prefix: String::new(),
            soak_hours: MAX_SOAK_HOURS + 1,
        };
        assert!(slow.check().unwrap_err().contains("soak"));
        assert_eq!(slow.soak_ms(), i64::from(MAX_SOAK_HOURS) * 3_600_000);
    }

    #[test]
    fn new_releases_wait_for_the_soak_time() {
        let f = VersionFilter {
            prefix: String::new(),
            soak_hours: 2,
        };
        let history = [known("1.0.0")];
        let mut soaking = BTreeMap::new();
        let t0 = Timestamp(1_000);
        let out = soak(vec![rel("1.1.0"), rel("1.0.0")], &f, &history, false, &mut soaking, t0);
        assert_eq!((out.ready.len(), out.waiting), (1, 1), "the known one passes, the new one waits");
        assert_eq!(soaking.get("1.1.0"), Some(&t0));
        let early = soak(vec![rel("1.1.0")], &f, &history, false, &mut soaking, Timestamp(1_000 + 3_600_000));
        assert_eq!((early.ready.len(), early.waiting), (0, 1));
        let done = soak(vec![rel("1.1.0")], &f, &history, false, &mut soaking, Timestamp(1_000 + 7_200_000));
        assert_eq!((done.ready.len(), done.waiting), (1, 0));
        assert!(soaking.is_empty());
    }

    #[test]
    fn the_baseline_and_zero_soak_pass_and_withdrawn_versions_are_forgotten() {
        let f = VersionFilter {
            prefix: String::new(),
            soak_hours: 5,
        };
        let mut soaking = BTreeMap::from([("9.9.9".to_string(), Timestamp(1))]);
        let baseline = soak(vec![rel("1.0.0")], &f, &[], true, &mut soaking, Timestamp(10));
        assert_eq!((baseline.ready.len(), baseline.waiting), (1, 0));
        assert!(soaking.is_empty());
        let none = VersionFilter::default();
        assert_eq!(
            soak(vec![rel("2.0.0")], &none, &[], false, &mut soaking, Timestamp(10)).ready.len(),
            1
        );
        let mut stale = BTreeMap::from([("9.9.9".to_string(), Timestamp(1))]);
        soak(vec![rel("1.1.0")], &f, &[known("1.0.0")], false, &mut stale, Timestamp(10));
        assert!(!stale.contains_key("9.9.9") && stale.contains_key("1.1.0"));
    }
}
