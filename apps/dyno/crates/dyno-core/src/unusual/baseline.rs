//! The baseline of programs seen on this machine, kept in memory by the agent and persisted through [`Change`]s. Pure:
//! time, the process table and the startup list come in as values. A fresh baseline learns silently for the configured
//! number of days (nothing is flagged), then lists programs it has not seen as new, with reasons when they are unusual.

use super::{assess, program_key, Assessment, Facts, Level, Reason, Signing, UnusualSettings, UNUSUAL_SCORE};
use crate::os::Os;
use crate::time::{Timestamp, DAY, MINUTE};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Most programs remembered; beyond it the ones not seen for longest are forgotten.
pub const MAX_ENTRIES: usize = 20_000;
/// Most items one answer lists.
pub const MAX_ITEMS: usize = 200;
/// A new program stays on the list this many days, then counts as known.
pub const NEW_FOR_DAYS: i64 = 14;
/// "Last seen" is written to storage at most this often per program.
pub const LAST_SEEN_WRITE_MS: i64 = 10 * MINUTE;
/// A program seen within this long is "running".
pub const RUNNING_MS: i64 = 2 * MINUTE;

/// What the user decided about a program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    /// Listed: new or unusual and not dealt with.
    New,
    /// Known: learned, aged out or acknowledged.
    Known,
    /// The user chose to ignore it; it stays out of the list.
    Ignored,
    /// The user added it to the protected list (and it stays out of the list).
    Protected,
}

/// What the user can do with an item. None of them changes the program itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UnusualAction {
    /// Hide it.
    Ignore,
    /// Add it to the protected list so rules never touch it, and hide it.
    Protect,
    /// Bring an ignored program back to the list.
    Restore,
}

/// Why a program is listed, as of the last look.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Flag {
    /// 0 to 100.
    pub score: u32,
    /// Why.
    pub reasons: Vec<Reason>,
}

/// One program in the baseline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// Its identity ([`program_key`]).
    pub key: String,
    /// Executable name.
    pub name: String,
    /// Full path, when readable.
    pub exe: Option<String>,
    /// When it was first seen.
    pub first_seen: Timestamp,
    /// When it was last seen.
    pub last_seen: Timestamp,
    /// Seen while the baseline was still learning (so never "new").
    pub learned: bool,
    /// Where it stands.
    pub status: Status,
    /// Its signature, as far as checked.
    pub signing: Signing,
    /// Why it is listed (only while `status` is [`Status::New`]).
    pub flag: Option<Flag>,
}

/// A program the process table shows right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observed {
    /// Executable name.
    pub name: String,
    /// Full path, when readable.
    pub exe: Option<String>,
    /// The most children one of its processes has.
    pub children: u32,
    /// Its open network connections.
    pub connections: u32,
}

/// A startup entry as the baseline sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupObs {
    /// The entry's stable id.
    pub id: String,
    /// The entry's name.
    pub name: String,
    /// The executable it starts.
    pub exe: Option<String>,
}

/// What to persist after a look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Store or replace a program.
    Entry(Entry),
    /// Forget these programs.
    Remove(Vec<String>),
    /// A startup entry id was learned at this time.
    Startup(String, Timestamp),
    /// The baseline started at this time.
    Since(Timestamp),
}

/// The result of one look.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Outcome {
    /// What to persist.
    pub changes: Vec<Change>,
    /// Programs that just became unusual (worth one line in the activity feed each).
    pub newly_unusual: Vec<String>,
}

/// Where learning stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Learning {
    /// Still learning silently.
    pub active: bool,
    /// When learning ends (or ended); none before the first look.
    pub until: Option<Timestamp>,
}

/// A program on the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnusualItem {
    /// Its identity (what actions refer to).
    pub key: String,
    /// Executable name.
    pub name: String,
    /// Full path, when readable.
    pub exe: Option<String>,
    /// The publisher, when signed and checked.
    pub publisher: Option<String>,
    /// When it was first seen.
    pub first_seen: Timestamp,
    /// When it was last seen.
    pub last_seen: Timestamp,
    /// Seen in the last couple of minutes.
    pub running: bool,
    /// New, or worth a look.
    pub level: Level,
    /// 0 to 100.
    pub score: u32,
    /// Where it stands.
    pub status: Status,
    /// Why, in plain words ("was not seen on this computer before").
    pub reasons: Vec<String>,
}

/// What the page shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnusualOverview {
    /// When this was built (the engine's clock, which ages in the page are measured against).
    pub at: Timestamp,
    /// Whether the feature is on.
    pub enabled: bool,
    /// Where learning stands.
    pub learning: Learning,
    /// Days of silent learning configured.
    pub learning_days: u32,
    /// Programs in the baseline.
    pub known: u32,
    /// Programs the user ignored.
    pub ignored: u32,
    /// The list, most unusual first (at most [`MAX_ITEMS`]).
    pub items: Vec<UnusualItem>,
}

/// The baseline.
#[derive(Debug, Clone, Default)]
pub struct Baseline {
    since: Option<Timestamp>,
    entries: HashMap<String, Entry>,
    startup: HashSet<String>,
}

impl Baseline {
    /// A baseline restored from storage.
    pub fn restore(since: Option<Timestamp>, entries: Vec<Entry>, startup: Vec<String>) -> Baseline {
        Baseline {
            since,
            entries: entries.into_iter().map(|e| (e.key.clone(), e)).collect(),
            startup: startup.into_iter().collect(),
        }
    }

    /// When the baseline started.
    pub fn since(&self) -> Option<Timestamp> {
        self.since
    }

    /// Programs remembered.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing is remembered.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether a program is not in the baseline yet (the agent checks signatures of these only).
    pub fn is_new(&self, key: &str) -> bool {
        !self.entries.contains_key(key)
    }

    /// A program by key.
    pub fn entry(&self, key: &str) -> Option<&Entry> {
        self.entries.get(key)
    }

    /// Where learning stands at `now`.
    pub fn learning(&self, now: Timestamp, s: &UnusualSettings) -> Learning {
        match self.since {
            None => Learning { active: true, until: None },
            Some(since) => {
                let until = since.plus(i64::from(s.learning_days) * DAY);
                Learning {
                    active: now < until,
                    until: Some(until),
                }
            }
        }
    }

    /// Looks at the programs running now. `signatures` holds the signatures the agent checked this round (by key).
    pub fn observe(
        &mut self,
        now: Timestamp,
        s: &UnusualSettings,
        os: Os,
        seen: &[Observed],
        startup: &[StartupObs],
        signatures: &HashMap<String, Signing>,
    ) -> Outcome {
        let mut out = Outcome::default();
        if !s.enabled {
            return out;
        }
        let first_look = self.since.is_none();
        if first_look {
            self.since = Some(now);
            out.changes.push(Change::Since(now));
        }
        // The very first look is always silent: a baseline that has not seen anything yet cannot call anything new.
        let learning = first_look || self.learning(now, s).active;
        // Startup entries not in the baseline: learned silently while learning, new persistence afterwards.
        let mut new_startup: Vec<&StartupObs> = Vec::new();
        for st in startup {
            if self.startup.insert(st.id.clone()) {
                out.changes.push(Change::Startup(st.id.clone(), now));
                if !learning {
                    new_startup.push(st);
                }
            }
        }
        let mut touched: HashSet<String> = HashSet::new();
        for o in seen {
            let key = program_key(&o.name, o.exe.as_deref());
            if !touched.insert(key.clone()) {
                continue;
            }
            let startup_hit = new_startup
                .iter()
                .any(|st| st.exe.as_deref().is_some_and(|e| program_key(&o.name, Some(e)) == key));
            let before = self.entries.get(&key).cloned();
            let mut e = before.clone().unwrap_or_else(|| Entry {
                key: key.clone(),
                name: o.name.clone(),
                exe: o.exe.clone(),
                first_seen: now,
                last_seen: now,
                learned: learning,
                status: if learning { Status::Known } else { Status::New },
                signing: Signing::Unchecked,
                flag: None,
            });
            e.last_seen = now;
            if let Some(sig) = signatures.get(&key) {
                e.signing = sig.clone();
            }
            if !learning && matches!(e.status, Status::New | Status::Known) {
                self.judge(&mut e, now, os, o.children, o.connections, startup_hit, &mut out);
            }
            self.keep(e, before, &mut out);
        }
        for st in new_startup {
            let name = st
                .exe
                .as_deref()
                .map_or_else(|| st.name.clone(), |p| crate::grouping::file_name(p).to_string());
            let key = program_key(&name, st.exe.as_deref());
            if touched.contains(&key) {
                continue;
            }
            let before = self.entries.get(&key).cloned();
            let mut e = before.clone().unwrap_or_else(|| Entry {
                key: key.clone(),
                name,
                exe: st.exe.clone(),
                first_seen: now,
                last_seen: Timestamp(0),
                learned: false,
                status: Status::New,
                signing: Signing::Unchecked,
                flag: None,
            });
            if matches!(e.status, Status::New | Status::Known) {
                self.judge(&mut e, now, os, 0, 0, true, &mut out);
            }
            self.keep(e, before, &mut out);
        }
        self.prune(&mut out);
        out
    }

    /// Re-judges a program that is new, or known and behaving oddly, and updates its flag and status.
    #[allow(clippy::too_many_arguments)]
    fn judge(&self, e: &mut Entry, now: Timestamp, os: Os, children: u32, connections: u32, new_startup: bool, out: &mut Outcome) {
        let aged = now.since(e.first_seen) > NEW_FOR_DAYS * DAY;
        let first = !e.learned && e.status == Status::New && !aged;
        let a: Assessment = assess(
            os,
            &Facts {
                name: &e.name,
                exe: e.exe.as_deref(),
                first_seen: first,
                signing: &e.signing,
                children,
                connections,
                new_startup,
            },
        );
        let was_unusual = e.flag.as_ref().is_some_and(|f| f.score >= UNUSUAL_SCORE);
        let behaving = a.reasons.iter().any(|r| !matches!(r, Reason::FirstSeen));
        // A known program is listed again only when what it does now is unusual by itself.
        let list = if first { true } else { a.score >= UNUSUAL_SCORE && behaving };
        if list {
            e.status = Status::New;
            e.flag = Some(Flag {
                score: a.score,
                reasons: a.reasons,
            });
            if !was_unusual && a.score >= UNUSUAL_SCORE {
                out.newly_unusual.push(e.key.clone());
            }
        } else {
            e.status = Status::Known;
            e.flag = None;
        }
    }

    fn keep(&mut self, e: Entry, before: Option<Entry>, out: &mut Outcome) {
        // "Last seen" moves every look; it is written only now and then, everything else when it changes.
        let material = before.as_ref().is_none_or(|b| {
            let mut same = e.clone();
            same.last_seen = b.last_seen;
            same != *b || e.last_seen.since(b.last_seen) >= LAST_SEEN_WRITE_MS
        });
        if material {
            out.changes.push(Change::Entry(e.clone()));
        }
        self.entries.insert(e.key.clone(), e);
    }

    fn prune(&mut self, out: &mut Outcome) {
        if self.entries.len() <= MAX_ENTRIES {
            return;
        }
        let mut old: Vec<(&String, Timestamp)> = self
            .entries
            .iter()
            .filter(|(_, e)| matches!(e.status, Status::Known))
            .map(|(k, e)| (k, e.last_seen))
            .collect();
        old.sort_by_key(|(_, t)| *t);
        let excess = self.entries.len() - MAX_ENTRIES + MAX_ENTRIES / 10;
        let drop: Vec<String> = old.into_iter().take(excess).map(|(k, _)| k.clone()).collect();
        for k in &drop {
            self.entries.remove(k);
        }
        if !drop.is_empty() {
            out.changes.push(Change::Remove(drop));
        }
    }

    /// The user's decision about a program. `None` when it is not in the baseline.
    pub fn act(&mut self, key: &str, action: UnusualAction) -> Option<Entry> {
        let e = self.entries.get_mut(key)?;
        match action {
            UnusualAction::Ignore => e.status = Status::Ignored,
            UnusualAction::Protect => e.status = Status::Protected,
            UnusualAction::Restore => {
                if matches!(e.status, Status::Ignored | Status::Protected) {
                    e.status = Status::New;
                }
            }
        }
        Some(e.clone())
    }

    /// Forgets everything; the next look starts a new learning period.
    pub fn forget(&mut self) {
        *self = Baseline::default();
    }

    /// The page's data at `now`.
    pub fn overview(&self, now: Timestamp, s: &UnusualSettings, include_ignored: bool) -> UnusualOverview {
        let mut items: Vec<UnusualItem> = self
            .entries
            .values()
            .filter(|e| e.status == Status::New || (include_ignored && matches!(e.status, Status::Ignored | Status::Protected)))
            .map(|e| {
                let (score, reasons) = e
                    .flag
                    .as_ref()
                    .map_or((0, Vec::new()), |f| (f.score, f.reasons.iter().map(Reason::text).collect()));
                UnusualItem {
                    key: e.key.clone(),
                    name: e.name.clone(),
                    exe: e.exe.clone(),
                    publisher: match &e.signing {
                        Signing::Signed(who) => Some(who.clone()),
                        _ => None,
                    },
                    first_seen: e.first_seen,
                    last_seen: e.last_seen,
                    running: e.last_seen.millis() > 0 && now.since(e.last_seen) < RUNNING_MS,
                    level: if score >= UNUSUAL_SCORE { Level::Unusual } else { Level::New },
                    score,
                    status: e.status,
                    reasons,
                }
            })
            .collect();
        items.sort_by(|a, b| {
            (a.status != Status::New)
                .cmp(&(b.status != Status::New))
                .then(b.score.cmp(&a.score))
                .then(b.first_seen.cmp(&a.first_seen))
                .then(a.key.cmp(&b.key))
        });
        items.truncate(MAX_ITEMS);
        UnusualOverview {
            at: now,
            enabled: s.enabled,
            learning: self.learning(now, s),
            learning_days: s.learning_days,
            known: self.entries.values().filter(|e| e.status == Status::Known).count() as u32,
            ignored: self
                .entries
                .values()
                .filter(|e| matches!(e.status, Status::Ignored | Status::Protected))
                .count() as u32,
            items,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_761_091_200_000;
    const S: UnusualSettings = UnusualSettings {
        enabled: true,
        learning_days: 7,
    };

    fn prog(name: &str, exe: &str) -> Observed {
        Observed {
            name: name.into(),
            exe: Some(exe.into()),
            children: 0,
            connections: 0,
        }
    }

    fn at(days: i64) -> Timestamp {
        Timestamp(T0 + days * DAY)
    }

    fn look(b: &mut Baseline, t: Timestamp, seen: &[Observed], startup: &[StartupObs]) -> Outcome {
        b.observe(t, &S, Os::Macos, seen, startup, &HashMap::new())
    }

    #[test]
    fn a_fresh_baseline_learns_silently() {
        let mut b = Baseline::default();
        let o = look(&mut b, at(0), &[prog("a", "/Applications/A.app/a"), prog("evil", "/tmp/evil")], &[]);
        assert!(o.newly_unusual.is_empty());
        let ov = b.overview(at(0), &S, false);
        assert!(ov.items.is_empty());
        assert!(ov.learning.active);
        assert_eq!(ov.learning.until, Some(at(7)));
        assert_eq!(ov.known, 2);
        assert!(o.changes.contains(&Change::Since(at(0))));
        // Still learning on day 6: a new program is learned, not listed.
        look(&mut b, at(6), &[prog("b", "/tmp/b")], &[]);
        assert!(b.overview(at(6), &S, false).items.is_empty());
        assert_eq!(b.overview(at(6), &S, false).known, 3);
    }

    #[test]
    fn after_learning_a_new_program_is_listed_and_a_known_one_is_not() {
        let mut b = Baseline::default();
        look(&mut b, at(0), &[prog("a", "/Applications/A.app/a")], &[]);
        let o = look(
            &mut b,
            at(8),
            &[prog("a", "/Applications/A.app/a"), prog("new", "/Applications/New.app/new")],
            &[],
        );
        assert!(o.newly_unusual.is_empty(), "first seen alone is new, not unusual");
        let ov = b.overview(at(8), &S, false);
        assert!(!ov.learning.active);
        assert_eq!(ov.items.len(), 1);
        assert_eq!(ov.items[0].name, "new");
        assert_eq!(ov.items[0].level, Level::New);
        assert_eq!(ov.items[0].reasons, vec!["was not seen on this computer before".to_string()]);
        assert!(ov.items[0].running);
    }

    #[test]
    fn an_unusual_new_program_is_reported_once() {
        let mut b = Baseline::default();
        look(&mut b, at(0), &[], &[]);
        let bad = prog("helper", "/private/var/folders/x/T/helper");
        let o = look(&mut b, at(8), std::slice::from_ref(&bad), &[]);
        assert_eq!(o.newly_unusual.len(), 1);
        let again = look(&mut b, at(8).plus(30_000), &[bad], &[]);
        assert!(again.newly_unusual.is_empty());
        assert!(
            again.changes.iter().all(|c| !matches!(c, Change::Entry(_))),
            "nothing changed, nothing to write"
        );
        let item = &b.overview(at(8), &S, false).items[0];
        assert_eq!(item.level, Level::Unusual);
        assert!(item.reasons.iter().any(|r| r.contains("temporary folder")));
    }

    #[test]
    fn a_signature_that_arrives_later_updates_the_reasons() {
        let mut b = Baseline::default();
        look(&mut b, at(0), &[], &[]);
        let p = prog("tool", "/Users/ann/tool");
        look(&mut b, at(8), std::slice::from_ref(&p), &[]);
        assert_eq!(b.overview(at(8), &S, false).items[0].score, 15);
        let key = program_key("tool", Some("/Users/ann/tool"));
        let sigs = HashMap::from([(key, Signing::Unsigned)]);
        let o = b.observe(at(8).plus(10_000), &S, Os::Macos, &[p], &[], &sigs);
        assert_eq!(o.newly_unusual.len(), 1, "15 + 25 is 40, which is unusual");
        let item = &b.overview(at(8), &S, false).items[0];
        assert_eq!(item.score, 40);
        assert_eq!(item.level, Level::Unusual);
    }

    #[test]
    fn a_new_startup_entry_after_learning_is_flagged_even_when_not_running() {
        let mut b = Baseline::default();
        let st = |id: &str, exe: &str| StartupObs {
            id: id.into(),
            name: id.into(),
            exe: Some(exe.into()),
        };
        look(&mut b, at(0), &[], &[st("old", "/Applications/Old.app/old")]);
        let o = look(
            &mut b,
            at(9),
            &[],
            &[
                st("old", "/Applications/Old.app/old"),
                st("dropper", "/Users/ann/Library/x/dropper"),
            ],
        );
        assert_eq!(o.newly_unusual.len(), 1);
        let ov = b.overview(at(9), &S, false);
        assert_eq!(ov.items.len(), 1);
        assert_eq!(ov.items[0].name, "dropper");
        assert!(!ov.items[0].running);
        assert!(ov.items[0]
            .reasons
            .iter()
            .any(|r| r.contains("starts at login") || r.contains("what starts at login")));
        // It is learned: the next look does not flag it again.
        let o2 = look(
            &mut b,
            at(9).plus(60_000),
            &[],
            &[
                st("old", "/Applications/Old.app/old"),
                st("dropper", "/Users/ann/Library/x/dropper"),
            ],
        );
        assert!(o2.newly_unusual.is_empty());
    }

    #[test]
    fn a_new_startup_entry_for_a_running_program_joins_its_flag() {
        let mut b = Baseline::default();
        look(&mut b, at(0), &[], &[]);
        let st = StartupObs {
            id: "agent".into(),
            name: "agent".into(),
            exe: Some("/Applications/A.app/a".into()),
        };
        let o = look(&mut b, at(8), &[prog("a", "/Applications/A.app/a")], &[st]);
        let item = &b.overview(at(8), &S, false).items[0];
        assert!(item.reasons.iter().any(|r| r.contains("login")));
        assert_eq!(item.score, 45);
        assert_eq!(o.newly_unusual.len(), 1);
    }

    #[test]
    fn user_decisions_stick() {
        let mut b = Baseline::default();
        look(&mut b, at(0), &[], &[]);
        let p = prog("helper", "/tmp/helper");
        look(&mut b, at(8), std::slice::from_ref(&p), &[]);
        let key = program_key("helper", Some("/tmp/helper"));
        assert_eq!(b.act(&key, UnusualAction::Ignore).unwrap().status, Status::Ignored);
        look(&mut b, at(9), std::slice::from_ref(&p), &[]);
        assert!(b.overview(at(9), &S, false).items.is_empty());
        let with = b.overview(at(9), &S, true);
        assert_eq!(with.items.len(), 1);
        assert_eq!(with.ignored, 1);
        assert_eq!(b.act(&key, UnusualAction::Protect).unwrap().status, Status::Protected);
        assert_eq!(b.act(&key, UnusualAction::Restore).unwrap().status, Status::New);
        assert_eq!(b.overview(at(9), &S, false).items.len(), 1);
        assert!(b.act("nothing", UnusualAction::Ignore).is_none());
        // Restore on a program that is listed does nothing.
        assert_eq!(b.act(&key, UnusualAction::Restore).unwrap().status, Status::New);
    }

    #[test]
    fn new_programs_age_into_the_baseline() {
        let mut b = Baseline::default();
        look(&mut b, at(0), &[], &[]);
        let p = prog("tool", "/Applications/T.app/t");
        look(&mut b, at(8), std::slice::from_ref(&p), &[]);
        assert_eq!(b.overview(at(8), &S, false).items.len(), 1);
        look(&mut b, at(8 + NEW_FOR_DAYS + 1), &[p], &[]);
        let ov = b.overview(at(30), &S, false);
        assert!(ov.items.is_empty());
        assert_eq!(ov.known, 1);
    }

    #[test]
    fn a_known_program_behaving_oddly_is_listed_again_only_when_unusual_by_itself() {
        let mut b = Baseline::default();
        look(&mut b, at(0), &[prog("builder", "/Applications/B.app/b")], &[]);
        let mut busy = prog("builder", "/Applications/B.app/b");
        busy.children = 80; // 20 alone: not enough
        look(&mut b, at(8), &[busy.clone()], &[]);
        assert!(b.overview(at(8), &S, false).items.is_empty());
        busy.connections = 90; // 20 + 20: enough
        let o = look(&mut b, at(8).plus(1000), &[busy], &[]);
        assert_eq!(o.newly_unusual.len(), 1);
        assert_eq!(b.overview(at(8), &S, false).items.len(), 1);
    }

    #[test]
    fn disabled_does_nothing_and_forget_restarts_learning() {
        let mut b = Baseline::default();
        let off = UnusualSettings {
            enabled: false,
            learning_days: 7,
        };
        let o = b.observe(at(0), &off, Os::Macos, &[prog("a", "/a")], &[], &HashMap::new());
        assert_eq!(o, Outcome::default());
        assert!(b.is_empty());
        assert!(!b.overview(at(0), &off, false).enabled);
        look(&mut b, at(0), &[prog("a", "/a")], &[]);
        assert_eq!(b.len(), 1);
        assert_eq!(b.since(), Some(at(0)));
        b.forget();
        assert!(b.is_empty());
        assert_eq!(b.since(), None);
        assert!(b.overview(at(1), &S, false).learning.until.is_none());
    }

    #[test]
    fn zero_learning_days_flags_straight_away() {
        let mut b = Baseline::default();
        let s = UnusualSettings {
            enabled: true,
            learning_days: 0,
        };
        b.observe(at(0), &s, Os::Macos, &[prog("a", "/Applications/A.app/a")], &[], &HashMap::new());
        assert!(
            b.overview(at(0), &s, false).items.is_empty(),
            "the first look is silent even with no learning period"
        );
        b.observe(
            at(0).plus(10_000),
            &s,
            Os::Macos,
            &[prog("b", "/Applications/B.app/b")],
            &[],
            &HashMap::new(),
        );
        assert_eq!(b.overview(at(0), &s, false).items.len(), 1);
    }

    #[test]
    fn restore_round_trips_and_sorts_by_score() {
        let mut b = Baseline::default();
        look(&mut b, at(0), &[], &[]);
        look(
            &mut b,
            at(8),
            &[prog("plain", "/Applications/P.app/p"), prog("odd", "/tmp/odd")],
            &[],
        );
        let ov = b.overview(at(8), &S, false);
        assert_eq!(ov.items[0].name, "odd");
        let entries: Vec<Entry> = b.entries.values().cloned().collect();
        let restored = Baseline::restore(b.since(), entries, vec!["x".into()]);
        assert_eq!(restored.overview(at(8), &S, false), ov);
        assert!(restored.is_new("nope"));
        assert!(!restored.is_new(&program_key("odd", Some("/tmp/odd"))));
        assert!(restored.entry(&program_key("odd", Some("/tmp/odd"))).is_some());
    }

    #[test]
    fn the_baseline_is_bounded() {
        let many: Vec<Entry> = (0..(MAX_ENTRIES + 50))
            .map(|n| Entry {
                key: format!("k{n}"),
                name: format!("k{n}"),
                exe: None,
                first_seen: at(0),
                last_seen: Timestamp(T0 + n as i64),
                learned: true,
                status: Status::Known,
                signing: Signing::Unchecked,
                flag: None,
            })
            .collect();
        let mut b = Baseline::restore(Some(at(0)), many, vec![]);
        let o = look(&mut b, at(1), &[], &[]);
        assert!(b.len() <= MAX_ENTRIES);
        let removed = o
            .changes
            .iter()
            .find_map(|c| if let Change::Remove(k) = c { Some(k.clone()) } else { None })
            .unwrap();
        assert!(removed.contains(&"k0".to_string()), "the least recently seen go first");
    }
}
