//! "What changed": a timeline of installs, updates, removals and new or gone startup items, so a slowdown can be put next
//! to what happened just before it. Software and startup items are compared snapshot against snapshot. Dyno itself
//! changes nothing, so nothing here is its own doing.

use super::software::SoftwareItem;
use crate::time::{Timestamp, HOUR};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Most changes returned.
pub const MAX_CHANGES: usize = 500;
/// Widest window around a moment, in hours.
pub const MAX_WINDOW_HOURS: u32 = 24 * 30;

/// What kind of change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeKind {
    /// A program appeared.
    Installed,
    /// A program's version changed.
    Updated,
    /// A program disappeared.
    Removed,
    /// A new startup item.
    StartupAdded,
    /// A startup item went away.
    StartupRemoved,
}

/// One change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    /// When Dyno noticed it.
    pub at: Timestamp,
    /// What kind.
    pub kind: ChangeKind,
    /// One line.
    pub title: String,
    /// More, when there is more.
    pub detail: String,
}

/// What the timeline view asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ChangesQuery {
    /// Centre the window on this moment (the latest changes when absent).
    pub around: Option<Timestamp>,
    /// Hours on each side of `around`, or back from now (1 to 720; 0 means the default of 72).
    pub hours: u32,
}

impl ChangesQuery {
    /// The window as `[from, to]` given `now`.
    pub fn window(&self, now: Timestamp) -> (Timestamp, Timestamp) {
        let hours = i64::from(if self.hours == 0 { 72 } else { self.hours.min(MAX_WINDOW_HOURS) });
        match self.around {
            Some(t) => (t.minus(hours * HOUR), t.plus(hours * HOUR).min_ts(now)),
            None => (now.minus(hours * HOUR), now),
        }
    }
}

impl Timestamp {
    fn min_ts(self, other: Timestamp) -> Timestamp {
        if self.0 <= other.0 {
            self
        } else {
            other
        }
    }
}

/// The timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChangeTimeline {
    /// The window start.
    pub from: Timestamp,
    /// The window end.
    pub to: Timestamp,
    /// Changes, newest first.
    pub changes: Vec<Change>,
    /// Since when Dyno has compared software lists (it cannot know what changed before that).
    pub watching_since: Option<Timestamp>,
}

/// Software added, updated or removed between two lists, all noticed at `at`.
pub fn diff_software(previous: &[SoftwareItem], current: &[SoftwareItem], at: Timestamp) -> Vec<Change> {
    let map = |l: &[SoftwareItem]| -> BTreeMap<String, Option<String>> { l.iter().map(|i| (i.name.clone(), i.version.clone())).collect() };
    let (before, after) = (map(previous), map(current));
    let mut out = Vec::new();
    for (name, version) in &after {
        match before.get(name) {
            None => out.push(Change {
                at,
                kind: ChangeKind::Installed,
                title: format!("Installed {name}"),
                detail: version.clone().map(|v| format!("Version {v}")).unwrap_or_default(),
            }),
            Some(old) if old != version => out.push(Change {
                at,
                kind: ChangeKind::Updated,
                title: format!("Updated {name}"),
                detail: format!(
                    "{} to {}",
                    old.as_deref().unwrap_or("unknown version"),
                    version.as_deref().unwrap_or("unknown version")
                ),
            }),
            Some(_) => {}
        }
    }
    for (name, version) in &before {
        if !after.contains_key(name) {
            out.push(Change {
                at,
                kind: ChangeKind::Removed,
                title: format!("Removed {name}"),
                detail: version.clone().map(|v| format!("Was version {v}")).unwrap_or_default(),
            });
        }
    }
    out
}

/// Startup items added or removed between two lists of `(id, name)`.
pub fn diff_startup(previous: &[(String, String)], current: &[(String, String)], at: Timestamp) -> Vec<Change> {
    fn ids(l: &[(String, String)]) -> std::collections::BTreeSet<&str> {
        l.iter().map(|(i, _)| i.as_str()).collect()
    }
    let (before, after) = (ids(previous), ids(current));
    let mut out = Vec::new();
    for (id, name) in current.iter().filter(|(i, _)| !before.contains(i.as_str())) {
        out.push(Change {
            at,
            kind: ChangeKind::StartupAdded,
            title: format!("New startup item {name}"),
            detail: id.clone(),
        });
    }
    for (id, name) in previous.iter().filter(|(i, _)| !after.contains(i.as_str())) {
        out.push(Change {
            at,
            kind: ChangeKind::StartupRemoved,
            title: format!("Startup item {name} is gone"),
            detail: id.clone(),
        });
    }
    out
}

/// Merges changes into the timeline for `query`: inside the window, newest first, capped.
pub fn timeline(query: &ChangesQuery, now: Timestamp, watching_since: Option<Timestamp>, mut all: Vec<Change>) -> ChangeTimeline {
    let (from, to) = query.window(now);
    all.retain(|c| c.at >= from && c.at <= to);
    all.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.title.cmp(&b.title)));
    all.truncate(MAX_CHANGES);
    ChangeTimeline {
        from,
        to,
        changes: all,
        watching_since,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sw(n: &str, v: &str) -> SoftwareItem {
        SoftwareItem {
            name: n.into(),
            version: Some(v.into()),
            publisher: None,
        }
    }

    #[test]
    fn software_diffs_find_installs_updates_and_removals() {
        let before = [sw("Editor", "1.0"), sw("Old", "2"), sw("Same", "3")];
        let after = [sw("Editor", "1.1"), sw("New", "9"), sw("Same", "3")];
        let d = diff_software(&before, &after, Timestamp(10));
        let kinds: Vec<_> = d.iter().map(|c| (c.kind, c.title.as_str())).collect();
        assert!(kinds.contains(&(ChangeKind::Updated, "Updated Editor")));
        assert!(kinds.contains(&(ChangeKind::Installed, "Installed New")));
        assert!(kinds.contains(&(ChangeKind::Removed, "Removed Old")));
        assert_eq!(d.len(), 3);
        assert_eq!(d.iter().find(|c| c.kind == ChangeKind::Updated).unwrap().detail, "1.0 to 1.1");
        assert!(diff_software(&after, &after, Timestamp(1)).is_empty());
    }

    #[test]
    fn startup_diffs() {
        let a = vec![("a".to_string(), "Alpha".to_string())];
        let b = vec![("b".to_string(), "Beta".to_string())];
        let d = diff_startup(&a, &b, Timestamp(1));
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].kind, ChangeKind::StartupAdded);
        assert_eq!(d[1].kind, ChangeKind::StartupRemoved);
    }

    #[test]
    fn the_window_is_around_a_moment_or_back_from_now() {
        let now = Timestamp(100 * HOUR);
        let q = ChangesQuery { around: None, hours: 0 };
        assert_eq!(q.window(now), (Timestamp(28 * HOUR), now));
        let q = ChangesQuery {
            around: Some(Timestamp(50 * HOUR)),
            hours: 2,
        };
        assert_eq!(q.window(now), (Timestamp(48 * HOUR), Timestamp(52 * HOUR)));
        // The window never reaches into the future.
        let q = ChangesQuery {
            around: Some(Timestamp(99 * HOUR)),
            hours: 5,
        };
        assert_eq!(q.window(now).1, now);
        let huge = ChangesQuery {
            around: None,
            hours: u32::MAX,
        };
        assert_eq!(huge.window(now).0, now.minus(i64::from(MAX_WINDOW_HOURS) * HOUR));
    }

    #[test]
    fn the_timeline_filters_sorts_and_caps() {
        let now = Timestamp(100 * HOUR);
        let mk = |h: i64| Change {
            at: Timestamp(h * HOUR),
            kind: ChangeKind::Installed,
            title: format!("c{h}"),
            detail: String::new(),
        };
        let t = timeline(
            &ChangesQuery { around: None, hours: 10 },
            now,
            Some(Timestamp(1)),
            vec![mk(80), mk(95), mk(99), mk(100)],
        );
        assert_eq!(
            t.changes.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(),
            ["c100", "c99", "c95"]
        );
        assert_eq!(t.watching_since, Some(Timestamp(1)));
        let many: Vec<_> = (0..MAX_CHANGES as i64 + 20)
            .map(|i| Change {
                at: Timestamp(now.0 - i),
                ..mk(99)
            })
            .collect();
        assert_eq!(
            timeline(&ChangesQuery { around: None, hours: 10 }, now, None, many).changes.len(),
            MAX_CHANGES
        );
    }
}
