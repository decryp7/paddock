//! Alarms, ISA-18.2 style: a pure state machine that turns raw conditions into a short list of alarms a person can
//! act on. One condition is one alarm (deduplicated by key, with a repeat count); it must be acknowledged; it can be
//! shelved or muted; a chattering alarm is flagged instead of nagging; quiet hours hold notifications back (critical
//! ones may bypass). The agent persists the changes the engine reports and pushes them to the app. No I/O, no clock:
//! time comes in as values (ADR 0001).

pub mod feed;
pub mod stats;

use crate::health::Severity;
use crate::time::{Timestamp, DAY, HOUR, MINUTE};
use crate::widgets::Metric;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, VecDeque};

pub use feed::Observation;
pub use stats::{AlarmStats, Suggestion, SuggestionKind};

/// Longest key, rule, title, detail and note, in characters.
pub const MAX_TEXT: usize = 200;
/// Most notes on one alarm.
pub const MAX_NOTES: usize = 20;
/// Most acknowledge times kept for the statistics.
const MAX_ACK_SAMPLES: usize = 1_000;

/// Where an alarm is in its life (ISA-18.2: active/unacknowledged, active/acknowledged, return-to-normal/unacknowledged,
/// normal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AlarmState {
    /// The condition holds and nobody has acknowledged it.
    Active,
    /// The condition holds and it was acknowledged.
    Acknowledged,
    /// The condition went away and it was acknowledged: finished.
    Cleared,
    /// The condition went away before anyone acknowledged it.
    ClearedUnacknowledged,
}

impl AlarmState {
    /// Whether the alarm still needs attention (not [`AlarmState::Cleared`]).
    pub fn is_open(self) -> bool {
        self != AlarmState::Cleared
    }
}

/// How long a shelved alarm stays quiet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShelveFor {
    /// One hour.
    OneHour,
    /// Until the next local midnight.
    UntilTomorrow,
    /// Until the machine restarts.
    UntilReboot,
    /// Until this time.
    Custom(Timestamp),
}

/// A resolved shelf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Shelf {
    /// Quiet until this time.
    Until(Timestamp),
    /// Quiet until the machine restarts.
    UntilReboot,
}

/// A note a person attached to an alarm.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlarmNote {
    /// When.
    pub at: Timestamp,
    /// What they wrote.
    pub text: String,
}

/// One alarm.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Alarm {
    /// Unique, increasing.
    pub id: u64,
    /// The source rule or alert kind, e.g. "lowDisk" (what muting and the statistics group by).
    pub rule: String,
    /// The condition's identity, e.g. "lowDisk:C:"; one open alarm per key.
    pub key: String,
    /// How bad.
    pub severity: Severity,
    /// Where it is in its life.
    pub state: AlarmState,
    /// One line.
    pub title: String,
    /// More detail.
    pub detail: String,
    /// When the condition first appeared (the chart opens here).
    pub first_seen: Timestamp,
    /// When it was last observed.
    pub last_seen: Timestamp,
    /// How many times it was observed (deduplicated).
    pub repeat_count: u32,
    /// The latest value of the measured thing, when there is one.
    pub current_value: Option<f64>,
    /// The metric to chart around `first_seen`.
    pub metric: Option<Metric>,
    /// Notes.
    pub notes: Vec<AlarmNote>,
    /// When it was acknowledged.
    pub acknowledged_at: Option<Timestamp>,
    /// When the condition went away.
    pub cleared_at: Option<Timestamp>,
    /// Shelved: kept out of the way until then.
    pub shelved: Option<Shelf>,
    /// The rule is muted: recorded, never announced.
    pub muted: bool,
    /// Flagged because it flips on and off too often.
    pub chattering: bool,
    /// The agent clears the alarm itself at this time (for one-off events).
    pub expires_at: Option<Timestamp>,
}

impl Alarm {
    /// The alarm is shelved at `now`.
    pub fn is_shelved(&self, now: Timestamp) -> bool {
        match self.shelved {
            Some(Shelf::Until(t)) => now < t,
            Some(Shelf::UntilReboot) => true,
            None => false,
        }
    }
}

/// Notifications held back during the night.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct QuietHours {
    /// On.
    pub enabled: bool,
    /// Start, minutes after local midnight (0-1439).
    pub start_minute: u16,
    /// End, minutes after local midnight (0-1439); an end before the start wraps past midnight.
    pub end_minute: u16,
    /// Critical alarms still announce themselves.
    pub critical_bypass: bool,
}

impl Default for QuietHours {
    fn default() -> Self {
        QuietHours {
            enabled: false,
            start_minute: 22 * 60,
            end_minute: 7 * 60,
            critical_bypass: true,
        }
    }
}

impl QuietHours {
    /// Whether the local minute of the day is quiet.
    pub fn contains(&self, minute_of_day: u16) -> bool {
        if !self.enabled || self.start_minute == self.end_minute {
            return false;
        }
        if self.start_minute < self.end_minute {
            (self.start_minute..self.end_minute).contains(&minute_of_day)
        } else {
            minute_of_day >= self.start_minute || minute_of_day < self.end_minute
        }
    }
}

/// The user's alarm settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AlarmConfig {
    /// Quiet hours.
    pub quiet: QuietHours,
    /// This many raise/return transitions inside the window mark an alarm as chattering (3-50).
    pub chatter_transitions: u32,
    /// The chatter window in minutes (1-240).
    pub chatter_window_minutes: u32,
    /// Most alarms kept, open and cleared (50-5000).
    pub max_alarms: u32,
    /// How long cleared alarms stay listed, days (1-90).
    pub keep_cleared_days: u32,
    /// The local offset from UTC in minutes (-840..840); the app fills it in from the user's clock, so quiet hours
    /// and "until tomorrow" follow local time.
    pub utc_offset_minutes: i32,
}

impl Default for AlarmConfig {
    fn default() -> Self {
        AlarmConfig {
            quiet: QuietHours::default(),
            chatter_transitions: 5,
            chatter_window_minutes: 10,
            max_alarms: 500,
            keep_cleared_days: 7,
            utc_offset_minutes: 0,
        }
    }
}

impl AlarmConfig {
    /// Every value in its allowed range.
    pub fn clamped(mut self) -> AlarmConfig {
        self.quiet.start_minute = self.quiet.start_minute.min(1_439);
        self.quiet.end_minute = self.quiet.end_minute.min(1_439);
        self.chatter_transitions = self.chatter_transitions.clamp(3, 50);
        self.chatter_window_minutes = self.chatter_window_minutes.clamp(1, 240);
        self.max_alarms = self.max_alarms.clamp(50, 5_000);
        self.keep_cleared_days = self.keep_cleared_days.clamp(1, 90);
        self.utc_offset_minutes = self.utc_offset_minutes.clamp(-14 * 60, 14 * 60);
        self
    }
}

/// Something that happens to the alarm list.
#[derive(Debug, Clone, PartialEq)]
pub enum AlarmEvent {
    /// A condition was observed.
    Raise {
        /// The source rule.
        rule: String,
        /// The condition's identity.
        key: String,
        /// How bad.
        severity: Severity,
        /// One line.
        title: String,
        /// Detail.
        detail: String,
        /// The measured value.
        value: Option<f64>,
        /// The metric to chart.
        metric: Option<Metric>,
        /// A one-off event: the alarm clears itself after this many milliseconds.
        ttl_ms: Option<i64>,
        /// When.
        at: Timestamp,
    },
    /// The condition went away.
    Return {
        /// The condition's identity.
        key: String,
        /// When.
        at: Timestamp,
    },
    /// Acknowledge these alarms.
    Acknowledge {
        /// The alarm ids.
        ids: Vec<u64>,
        /// When.
        at: Timestamp,
    },
    /// Keep an alarm quiet for a while.
    Shelve {
        /// The alarm.
        id: u64,
        /// For how long.
        how: ShelveFor,
        /// When.
        at: Timestamp,
    },
    /// Bring a shelved alarm back.
    Unshelve {
        /// The alarm.
        id: u64,
    },
    /// Mute or unmute a rule.
    Mute {
        /// The rule.
        rule: String,
        /// Muted (true) or not.
        muted: bool,
    },
    /// Attach a note.
    Note {
        /// The alarm.
        id: u64,
        /// The text.
        text: String,
        /// When.
        at: Timestamp,
    },
    /// The machine restarted: reboot-length shelves end.
    Rebooted {
        /// When.
        at: Timestamp,
    },
}

/// What happened to an alarm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeKind {
    /// A new alarm.
    Raised,
    /// Observed again (or raised again after clearing).
    Repeated,
    /// The condition went away.
    Returned,
    /// Acknowledged.
    Acknowledged,
    /// Shelved.
    Shelved,
    /// Back from the shelf.
    Unshelved,
    /// Its rule was muted or unmuted.
    Muted,
    /// A note was added.
    Noted,
    /// Flagged as chattering.
    Chattering,
    /// Dropped from the list to keep it bounded.
    Pruned,
}

/// One change, for the agent to persist and push.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlarmChange {
    /// What happened.
    pub kind: ChangeKind,
    /// The alarm after the change.
    pub alarm: Alarm,
    /// Announce it to the user (a desktop notification): new or escalated, not muted, shelved, chattering or quiet.
    pub notify: bool,
    /// When.
    pub at: Timestamp,
}

/// The minute of the local day for `at` given the local offset from UTC in minutes.
pub fn local_minute(at: Timestamp, utc_offset_minutes: i32) -> u16 {
    let minutes = at.millis().div_euclid(MINUTE) + i64::from(utc_offset_minutes);
    minutes.rem_euclid(1_440) as u16
}

/// The next local midnight after `at`.
pub fn next_local_midnight(at: Timestamp, utc_offset_minutes: i32) -> Timestamp {
    let local = at.millis().saturating_add(i64::from(utc_offset_minutes) * MINUTE);
    let start_of_day = local.div_euclid(DAY) * DAY;
    Timestamp(
        start_of_day
            .saturating_add(DAY)
            .saturating_sub(i64::from(utc_offset_minutes) * MINUTE),
    )
}

fn clip(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control() || *c == '\n').take(max).collect()
}

/// The alarm list and its rules.
#[derive(Debug)]
pub struct AlarmEngine {
    config: AlarmConfig,
    utc_offset_minutes: i32,
    alarms: Vec<Alarm>,
    muted_rules: BTreeSet<String>,
    transitions: HashMap<String, VecDeque<Timestamp>>,
    raises: HashMap<String, u32>,
    acks: VecDeque<(String, i64)>,
    next_id: u64,
}

impl Default for AlarmEngine {
    fn default() -> Self {
        AlarmEngine::new(AlarmConfig::default())
    }
}

impl AlarmEngine {
    /// An empty engine.
    pub fn new(config: AlarmConfig) -> AlarmEngine {
        let config = config.clamped();
        AlarmEngine {
            utc_offset_minutes: config.utc_offset_minutes,
            config,
            alarms: Vec::new(),
            muted_rules: BTreeSet::new(),
            transitions: HashMap::new(),
            raises: HashMap::new(),
            acks: VecDeque::new(),
            next_id: 1,
        }
    }

    /// An engine continuing from stored alarms and muted rules.
    pub fn restore(config: AlarmConfig, alarms: Vec<Alarm>, muted_rules: Vec<String>) -> AlarmEngine {
        let mut e = AlarmEngine::new(config);
        e.next_id = alarms.iter().map(|a| a.id).max().map_or(1, |m| m.saturating_add(1));
        for a in &alarms {
            *e.raises.entry(a.rule.clone()).or_insert(0) += 1;
            if let Some(ack) = a.acknowledged_at {
                e.note_ack(&a.rule, ack.since(a.first_seen));
            }
        }
        e.alarms = alarms;
        e.muted_rules = muted_rules.into_iter().collect();
        e
    }

    /// Replaces the settings.
    pub fn set_config(&mut self, config: AlarmConfig) {
        self.config = config.clamped();
        self.utc_offset_minutes = self.config.utc_offset_minutes;
    }

    /// The local offset from UTC in minutes (for quiet hours and "until tomorrow").
    pub fn set_utc_offset_minutes(&mut self, minutes: i32) {
        self.utc_offset_minutes = minutes.clamp(-14 * 60, 14 * 60);
    }

    /// All alarms, newest first.
    pub fn alarms(&self) -> Vec<Alarm> {
        let mut v = self.alarms.clone();
        v.sort_by(|a, b| b.last_seen.cmp(&a.last_seen).then(b.id.cmp(&a.id)));
        v
    }

    /// One alarm.
    pub fn alarm(&self, id: u64) -> Option<&Alarm> {
        self.alarms.iter().find(|a| a.id == id)
    }

    /// The muted rules.
    pub fn muted_rules(&self) -> Vec<String> {
        self.muted_rules.iter().cloned().collect()
    }

    /// The statistics over what the engine knows.
    pub fn stats(&self, now: Timestamp) -> AlarmStats {
        stats::compute(&self.alarms, &self.raises, &self.acks, now)
    }

    fn note_ack(&mut self, rule: &str, ms: i64) {
        if self.acks.len() >= MAX_ACK_SAMPLES {
            self.acks.pop_front();
        }
        self.acks.push_back((rule.to_string(), ms.max(0)));
    }

    fn quiet(&self, at: Timestamp, severity: Severity) -> bool {
        let q = self.config.quiet;
        q.contains(local_minute(at, self.utc_offset_minutes)) && !(q.critical_bypass && severity == Severity::Critical)
    }

    fn record_transition(&mut self, key: &str, at: Timestamp) -> bool {
        let window = i64::from(self.config.chatter_window_minutes) * MINUTE;
        let q = self.transitions.entry(key.to_string()).or_default();
        q.push_back(at);
        while q.front().is_some_and(|t| at.since(*t) > window) {
            q.pop_front();
        }
        q.len() as u32 >= self.config.chatter_transitions
    }

    fn change(kind: ChangeKind, alarm: &Alarm, notify: bool, at: Timestamp) -> AlarmChange {
        AlarmChange {
            kind,
            alarm: alarm.clone(),
            notify,
            at,
        }
    }

    /// Applies one event and returns what changed.
    pub fn apply(&mut self, event: AlarmEvent) -> Vec<AlarmChange> {
        let mut out = match event {
            AlarmEvent::Raise {
                rule,
                key,
                severity,
                title,
                detail,
                value,
                metric,
                ttl_ms,
                at,
            } => self.raise(rule, key, severity, title, detail, value, metric, ttl_ms, at),
            AlarmEvent::Return { key, at } => self.returned(&key, at),
            AlarmEvent::Acknowledge { ids, at } => self.acknowledge(&ids, at),
            AlarmEvent::Shelve { id, how, at } => self.shelve(id, how, at),
            AlarmEvent::Unshelve { id } => self.unshelve(id),
            AlarmEvent::Mute { rule, muted } => self.mute(&rule, muted),
            AlarmEvent::Note { id, text, at } => self.note(id, &text, at),
            AlarmEvent::Rebooted { at } => self.rebooted(at),
        };
        let at = out.last().map_or(Timestamp(0), |c| c.at);
        out.extend(self.prune(at));
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn raise(
        &mut self,
        rule: String,
        key: String,
        severity: Severity,
        title: String,
        detail: String,
        value: Option<f64>,
        metric: Option<Metric>,
        ttl_ms: Option<i64>,
        at: Timestamp,
    ) -> Vec<AlarmChange> {
        let (rule, key) = (clip(&rule, MAX_TEXT), clip(&key, MAX_TEXT));
        if rule.is_empty() || key.is_empty() {
            return Vec::new();
        }
        let muted = self.muted_rules.contains(&rule);
        let value = value.filter(|v| v.is_finite());
        let expires = ttl_ms.map(|t| at.plus(t.max(0)));
        let existing = self.alarms.iter().position(|a| a.key == key && a.state.is_open());
        let mut out = Vec::new();
        match existing {
            Some(i) => {
                let reactivated = self.alarms[i].state == AlarmState::ClearedUnacknowledged;
                let escalated = severity > self.alarms[i].severity;
                if reactivated {
                    // A flip: the condition came back before anyone acknowledged it.
                    self.record_transition(&key, at);
                }
                let chatter = self.is_chattering(&key, at);
                let a = &mut self.alarms[i];
                a.repeat_count = a.repeat_count.saturating_add(1);
                a.last_seen = at;
                a.current_value = value.or(a.current_value);
                a.metric = metric.or(a.metric.take());
                a.expires_at = expires.or(a.expires_at);
                a.muted = muted;
                if reactivated {
                    a.state = AlarmState::Active;
                    a.cleared_at = None;
                }
                a.severity = a.severity.max(severity);
                if !title.is_empty() {
                    a.title = clip(&title, MAX_TEXT);
                    a.detail = clip(&detail, MAX_TEXT);
                }
                let newly_chattering = chatter && !a.chattering;
                a.chattering = chatter;
                let snapshot = a.clone();
                let notify = (escalated || reactivated) && self.may_notify(&snapshot, at);
                if reactivated {
                    *self.raises.entry(rule).or_insert(0) += 1;
                }
                out.push(Self::change(ChangeKind::Repeated, &snapshot, notify, at));
                if newly_chattering {
                    out.push(Self::change(ChangeKind::Chattering, &snapshot, false, at));
                }
            }
            None => {
                // The key may have chattered while "cleared": count this appearance as a transition too.
                let chatter = {
                    self.record_transition(&key, at);
                    self.is_chattering(&key, at)
                };
                let id = self.next_id;
                self.next_id += 1;
                let a = Alarm {
                    id,
                    rule: rule.clone(),
                    key,
                    severity,
                    state: AlarmState::Active,
                    title: clip(&title, MAX_TEXT),
                    detail: clip(&detail, MAX_TEXT),
                    first_seen: at,
                    last_seen: at,
                    repeat_count: 1,
                    current_value: value,
                    metric,
                    notes: Vec::new(),
                    acknowledged_at: None,
                    cleared_at: None,
                    shelved: None,
                    muted,
                    chattering: chatter,
                    expires_at: expires,
                };
                let notify = self.may_notify(&a, at);
                *self.raises.entry(rule).or_insert(0) += 1;
                out.push(Self::change(ChangeKind::Raised, &a, notify, at));
                if chatter {
                    out.push(Self::change(ChangeKind::Chattering, &a, false, at));
                }
                self.alarms.push(a);
            }
        }
        out
    }

    fn is_chattering(&self, key: &str, at: Timestamp) -> bool {
        let window = i64::from(self.config.chatter_window_minutes) * MINUTE;
        self.transitions
            .get(key)
            .is_some_and(|q| q.iter().filter(|t| at.since(**t) <= window).count() as u32 >= self.config.chatter_transitions)
    }

    fn may_notify(&self, a: &Alarm, at: Timestamp) -> bool {
        !a.muted && !a.chattering && !a.is_shelved(at) && !self.quiet(at, a.severity)
    }

    fn returned(&mut self, key: &str, at: Timestamp) -> Vec<AlarmChange> {
        let Some(i) = self.alarms.iter().position(|a| a.key == key && a.state.is_open()) else {
            return Vec::new();
        };
        let a = &mut self.alarms[i];
        a.state = match a.state {
            AlarmState::Acknowledged => AlarmState::Cleared,
            _ => AlarmState::ClearedUnacknowledged,
        };
        a.cleared_at = Some(at);
        let snapshot = a.clone();
        self.record_transition(key, at);
        vec![Self::change(ChangeKind::Returned, &snapshot, false, at)]
    }

    fn acknowledge(&mut self, ids: &[u64], at: Timestamp) -> Vec<AlarmChange> {
        let mut out = Vec::new();
        let mut samples = Vec::new();
        for a in self.alarms.iter_mut().filter(|a| ids.contains(&a.id)) {
            let next = match a.state {
                AlarmState::Active => AlarmState::Acknowledged,
                AlarmState::ClearedUnacknowledged => AlarmState::Cleared,
                AlarmState::Acknowledged | AlarmState::Cleared => continue,
            };
            a.state = next;
            a.acknowledged_at = Some(at);
            samples.push((a.rule.clone(), at.since(a.first_seen)));
            out.push(Self::change(ChangeKind::Acknowledged, a, false, at));
        }
        for (rule, ms) in samples {
            self.note_ack(&rule, ms);
        }
        out
    }

    fn shelve(&mut self, id: u64, how: ShelveFor, at: Timestamp) -> Vec<AlarmChange> {
        let shelf = match how {
            ShelveFor::OneHour => Shelf::Until(at.plus(HOUR)),
            ShelveFor::UntilTomorrow => Shelf::Until(next_local_midnight(at, self.utc_offset_minutes)),
            ShelveFor::UntilReboot => Shelf::UntilReboot,
            ShelveFor::Custom(t) if t > at => Shelf::Until(t),
            ShelveFor::Custom(_) => return Vec::new(),
        };
        match self.alarms.iter_mut().find(|a| a.id == id && a.state.is_open()) {
            Some(a) => {
                a.shelved = Some(shelf);
                vec![Self::change(ChangeKind::Shelved, a, false, at)]
            }
            None => Vec::new(),
        }
    }

    fn unshelve(&mut self, id: u64) -> Vec<AlarmChange> {
        match self.alarms.iter_mut().find(|a| a.id == id && a.shelved.is_some()) {
            Some(a) => {
                a.shelved = None;
                let at = a.last_seen;
                vec![Self::change(ChangeKind::Unshelved, a, false, at)]
            }
            None => Vec::new(),
        }
    }

    fn mute(&mut self, rule: &str, muted: bool) -> Vec<AlarmChange> {
        let rule = clip(rule, MAX_TEXT);
        if rule.is_empty() {
            return Vec::new();
        }
        if muted {
            self.muted_rules.insert(rule.clone());
        } else {
            self.muted_rules.remove(&rule);
        }
        let mut out = Vec::new();
        for a in self.alarms.iter_mut().filter(|a| a.rule == rule && a.muted != muted) {
            a.muted = muted;
            let at = a.last_seen;
            out.push(Self::change(ChangeKind::Muted, a, false, at));
        }
        out
    }

    fn note(&mut self, id: u64, text: &str, at: Timestamp) -> Vec<AlarmChange> {
        let text = clip(text.trim(), MAX_TEXT);
        if text.is_empty() {
            return Vec::new();
        }
        match self.alarms.iter_mut().find(|a| a.id == id) {
            Some(a) => {
                if a.notes.len() >= MAX_NOTES {
                    a.notes.remove(0);
                }
                a.notes.push(AlarmNote { at, text });
                vec![Self::change(ChangeKind::Noted, a, false, at)]
            }
            None => Vec::new(),
        }
    }

    fn rebooted(&mut self, at: Timestamp) -> Vec<AlarmChange> {
        let mut out = Vec::new();
        for a in self.alarms.iter_mut().filter(|a| a.shelved == Some(Shelf::UntilReboot)) {
            a.shelved = None;
            out.push(Self::change(ChangeKind::Unshelved, a, false, at));
        }
        out
    }

    /// Time passing: expired shelves end, one-off events clear themselves and old cleared alarms go. Call about once
    /// a minute.
    pub fn tick(&mut self, now: Timestamp) -> Vec<AlarmChange> {
        let mut out = Vec::new();
        for a in &mut self.alarms {
            if matches!(a.shelved, Some(Shelf::Until(t)) if now >= t) {
                a.shelved = None;
                out.push(Self::change(ChangeKind::Unshelved, a, false, now));
            }
        }
        let expired: Vec<String> = self
            .alarms
            .iter()
            .filter(|a| a.state.is_open() && a.expires_at.is_some_and(|t| now >= t))
            .map(|a| a.key.clone())
            .collect();
        for key in expired {
            out.extend(self.returned(&key, now));
        }
        out.extend(self.prune(now));
        out
    }

    fn prune(&mut self, now: Timestamp) -> Vec<AlarmChange> {
        let mut dropped = Vec::new();
        let keep = i64::from(self.config.keep_cleared_days) * DAY;
        let max = self.config.max_alarms as usize;
        if now.millis() > 0 {
            self.alarms.retain(|a| {
                let old = a.state == AlarmState::Cleared && a.cleared_at.is_some_and(|c| now.since(c) > keep);
                if old {
                    dropped.push(a.clone());
                }
                !old
            });
        }
        while self.alarms.len() > max {
            // Drop the oldest cleared alarm; with none cleared, the oldest overall.
            let victim = self
                .alarms
                .iter()
                .enumerate()
                .filter(|(_, a)| a.state == AlarmState::Cleared)
                .min_by_key(|(_, a)| a.last_seen)
                .or_else(|| self.alarms.iter().enumerate().min_by_key(|(_, a)| a.last_seen))
                .map(|(i, _)| i);
            match victim {
                Some(i) => dropped.push(self.alarms.remove(i)),
                None => break,
            }
        }
        // Forget chatter bookkeeping for keys with no open alarm and no recent flips.
        let window = i64::from(self.config.chatter_window_minutes) * MINUTE;
        self.transitions
            .retain(|_, q| q.back().is_some_and(|t| now.since(*t) <= window) || now.millis() == 0);
        dropped
            .into_iter()
            .map(|a| Self::change(ChangeKind::Pruned, &a, false, now))
            .collect()
    }
}

/// Which alarms a list request wants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AlarmFilter {
    /// Only these states; empty means every open alarm.
    pub states: Vec<AlarmState>,
    /// At least this severe.
    pub min_severity: Option<Severity>,
    /// Only this rule.
    pub rule: Option<String>,
    /// Include shelved alarms (they are hidden by default).
    pub include_shelved: bool,
    /// At most this many (1-500; 0 means 100).
    pub limit: u32,
}

impl AlarmFilter {
    /// Whether `a` passes at `now`.
    pub fn matches(&self, a: &Alarm, now: Timestamp) -> bool {
        let state_ok = if self.states.is_empty() {
            a.state.is_open()
        } else {
            self.states.contains(&a.state)
        };
        state_ok
            && self.min_severity.is_none_or(|m| a.severity >= m)
            && self.rule.as_ref().is_none_or(|r| &a.rule == r)
            && (self.include_shelved || !a.is_shelved(now))
    }

    /// Filters and limits `alarms` (already newest first).
    pub fn apply(&self, alarms: Vec<Alarm>, now: Timestamp) -> Vec<Alarm> {
        let limit = if self.limit == 0 { 100 } else { self.limit.clamp(1, 500) } as usize;
        alarms.into_iter().filter(|a| self.matches(a, now)).take(limit).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raise(key: &str, sev: Severity, at: i64) -> AlarmEvent {
        AlarmEvent::Raise {
            rule: key.split(':').next().unwrap().into(),
            key: key.into(),
            severity: sev,
            title: format!("{key} title"),
            detail: "d".into(),
            value: Some(91.5),
            metric: Some(Metric::CpuTotal),
            ttl_ms: None,
            at: Timestamp(at),
        }
    }

    fn ret(key: &str, at: i64) -> AlarmEvent {
        AlarmEvent::Return {
            key: key.into(),
            at: Timestamp(at),
        }
    }

    fn state(e: &AlarmEngine, id: u64) -> AlarmState {
        e.alarm(id).unwrap().state
    }

    #[test]
    fn lifecycle_follows_isa_18_2() {
        let mut e = AlarmEngine::default();
        let c = e.apply(raise("cpu:a", Severity::Warning, 1_000));
        assert_eq!((c[0].kind, c[0].notify), (ChangeKind::Raised, true));
        let id = c[0].alarm.id;
        assert_eq!(state(&e, id), AlarmState::Active);
        // Acknowledge, then return: finished.
        e.apply(AlarmEvent::Acknowledge {
            ids: vec![id],
            at: Timestamp(2_000),
        });
        assert_eq!(state(&e, id), AlarmState::Acknowledged);
        e.apply(ret("cpu:a", 3_000));
        assert_eq!(state(&e, id), AlarmState::Cleared);
        // A new appearance after Cleared is a new alarm.
        let c = e.apply(raise("cpu:a", Severity::Warning, 4_000));
        assert_ne!(c[0].alarm.id, id);
        // Return before acknowledging, then acknowledge: cleared.
        let id2 = c[0].alarm.id;
        e.apply(ret("cpu:a", 5_000));
        assert_eq!(state(&e, id2), AlarmState::ClearedUnacknowledged);
        e.apply(AlarmEvent::Acknowledge {
            ids: vec![id2, id2, 999],
            at: Timestamp(6_000),
        });
        assert_eq!(state(&e, id2), AlarmState::Cleared);
        assert!(e
            .apply(AlarmEvent::Acknowledge {
                ids: vec![id2],
                at: Timestamp(7_000)
            })
            .is_empty());
        assert!(e.apply(ret("nothing", 1)).is_empty());
    }

    #[test]
    fn repeats_are_deduplicated_and_escalation_notifies() {
        let mut e = AlarmEngine::default();
        let id = e.apply(raise("disk:c", Severity::Warning, 1_000))[0].alarm.id;
        let c = e.apply(raise("disk:c", Severity::Warning, 2_000));
        assert_eq!((c[0].kind, c[0].notify, c[0].alarm.repeat_count), (ChangeKind::Repeated, false, 2));
        let c = e.apply(raise("disk:c", Severity::Critical, 3_000));
        assert!(c[0].notify);
        assert_eq!(c[0].alarm.severity, Severity::Critical);
        assert_eq!(e.alarms().len(), 1);
        let a = e.alarm(id).unwrap();
        assert_eq!((a.first_seen, a.last_seen, a.repeat_count), (Timestamp(1_000), Timestamp(3_000), 3));
        // A returned-unacknowledged alarm that comes back is announced again, and keeps its history.
        e.apply(ret("disk:c", 4_000));
        let c = e.apply(raise("disk:c", Severity::Critical, 5_000));
        assert!(c[0].notify);
        assert_eq!(c[0].alarm.state, AlarmState::Active);
        assert_eq!(c[0].alarm.cleared_at, None);
        // Non-finite values and empty keys are ignored.
        let mut ev = raise("disk:c", Severity::Info, 6_000);
        if let AlarmEvent::Raise { value, .. } = &mut ev {
            *value = Some(f64::NAN);
        }
        assert_eq!(e.apply(ev)[0].alarm.current_value, Some(91.5));
        assert!(e.apply(raise("", Severity::Info, 1)).is_empty());
    }

    #[test]
    fn shelving_muting_and_notes() {
        let mut e = AlarmEngine::default();
        e.set_utc_offset_minutes(60);
        let id = e.apply(raise("cpu:a", Severity::Warning, 0))[0].alarm.id;
        assert!(e
            .apply(AlarmEvent::Shelve {
                id,
                how: ShelveFor::Custom(Timestamp(0)),
                at: Timestamp(10)
            })
            .is_empty());
        e.apply(AlarmEvent::Shelve {
            id,
            how: ShelveFor::OneHour,
            at: Timestamp(1_000),
        });
        assert_eq!(e.alarm(id).unwrap().shelved, Some(Shelf::Until(Timestamp(1_000 + HOUR))));
        // Shelved: an escalation stays quiet.
        let c = e.apply(raise("cpu:a", Severity::Critical, 2_000));
        assert!(!c[0].notify);
        assert!(e.tick(Timestamp(HOUR)).is_empty());
        let c = e.tick(Timestamp(HOUR + 1_000));
        assert_eq!(c[0].kind, ChangeKind::Unshelved);
        assert!(!e.alarm(id).unwrap().is_shelved(Timestamp(HOUR + 1_000)));
        // Until tomorrow: next local midnight (UTC+1 => 23:00 UTC).
        e.apply(AlarmEvent::Shelve {
            id,
            how: ShelveFor::UntilTomorrow,
            at: Timestamp(2 * HOUR),
        });
        assert_eq!(e.alarm(id).unwrap().shelved, Some(Shelf::Until(Timestamp(23 * HOUR))));
        e.apply(AlarmEvent::Unshelve { id });
        assert!(e.apply(AlarmEvent::Unshelve { id }).is_empty());
        // Until reboot.
        e.apply(AlarmEvent::Shelve {
            id,
            how: ShelveFor::UntilReboot,
            at: Timestamp(0),
        });
        assert!(e.alarm(id).unwrap().is_shelved(Timestamp(i64::MAX)));
        assert!(e.tick(Timestamp(100 * DAY)).is_empty());
        assert_eq!(e.apply(AlarmEvent::Rebooted { at: Timestamp(5) })[0].kind, ChangeKind::Unshelved);
        assert!(e
            .apply(AlarmEvent::Shelve {
                id: 999,
                how: ShelveFor::OneHour,
                at: Timestamp(1)
            })
            .is_empty());
        // Muting.
        let c = e.apply(AlarmEvent::Mute {
            rule: "cpu".into(),
            muted: true,
        });
        assert_eq!((c.len(), c[0].kind), (1, ChangeKind::Muted));
        assert_eq!(e.muted_rules(), vec!["cpu".to_string()]);
        let c = e.apply(raise("cpu:b", Severity::Critical, 9_000));
        assert!(c[0].alarm.muted && !c[0].notify);
        e.apply(AlarmEvent::Mute {
            rule: "cpu".into(),
            muted: false,
        });
        assert!(!e.alarm(id).unwrap().muted);
        assert!(e
            .apply(AlarmEvent::Mute {
                rule: String::new(),
                muted: true
            })
            .is_empty());
        // Notes are trimmed, bounded and cleaned.
        assert!(e
            .apply(AlarmEvent::Note {
                id,
                text: "  ".into(),
                at: Timestamp(1)
            })
            .is_empty());
        assert!(e
            .apply(AlarmEvent::Note {
                id: 999,
                text: "x".into(),
                at: Timestamp(1)
            })
            .is_empty());
        for i in 0..MAX_NOTES + 3 {
            e.apply(AlarmEvent::Note {
                id,
                text: format!(" note {i}\u{7} "),
                at: Timestamp(i as i64),
            });
        }
        let notes = &e.alarm(id).unwrap().notes;
        assert_eq!(notes.len(), MAX_NOTES);
        assert_eq!(notes[MAX_NOTES - 1].text, format!("note {}", MAX_NOTES + 2));
    }

    #[test]
    fn quiet_hours_with_critical_bypass() {
        let mut cfg = AlarmConfig::default();
        cfg.quiet.enabled = true; // 22:00 - 07:00
        let mut e = AlarmEngine::new(cfg);
        let night = 23 * HOUR;
        assert!(!e.apply(raise("a:1", Severity::Warning, night))[0].notify);
        assert!(e.apply(raise("b:1", Severity::Critical, night))[0].notify, "critical bypasses");
        assert!(e.apply(raise("c:1", Severity::Warning, 12 * HOUR))[0].notify);
        cfg.quiet.critical_bypass = false;
        e.set_config(cfg);
        assert!(!e.apply(raise("d:1", Severity::Critical, night))[0].notify);
        // With a UTC offset, 20:00 UTC is 23:00 local at +3.
        e.set_utc_offset_minutes(3 * 60);
        assert!(!e.apply(raise("e:1", Severity::Warning, 20 * HOUR))[0].notify);
        let q = QuietHours {
            enabled: true,
            start_minute: 60,
            end_minute: 120,
            critical_bypass: true,
        };
        assert!(q.contains(61) && !q.contains(120) && !q.contains(0));
        assert!(!QuietHours {
            start_minute: 5,
            end_minute: 5,
            ..q
        }
        .contains(5));
        assert!(!QuietHours { enabled: false, ..q }.contains(61));
        assert_eq!(local_minute(Timestamp(-MINUTE), 0), 1_439);
        assert_eq!(next_local_midnight(Timestamp(0), -60), Timestamp(HOUR));
    }

    #[test]
    fn chattering_alarms_are_flagged_and_quiet() {
        let mut e = AlarmEngine::default();
        let mut notified = 0;
        let mut flagged = false;
        for i in 0..8 {
            let at = i * 20_000;
            for c in e.apply(raise("net:x", Severity::Warning, at)) {
                notified += i32::from(c.notify);
                flagged |= c.kind == ChangeKind::Chattering;
            }
            e.apply(ret("net:x", at + 10_000));
        }
        assert!(flagged);
        assert!(e.alarms().iter().any(|a| a.chattering));
        assert!(notified < 8, "chatter stops announcements ({notified})");
        // Outside the window the flag clears for a new alarm.
        let c = e.apply(raise("net:x", Severity::Warning, 5 * HOUR));
        assert!(!c[0].alarm.chattering);
    }

    #[test]
    fn one_off_events_expire_and_old_alarms_are_pruned() {
        let mut e = AlarmEngine::new(AlarmConfig {
            keep_cleared_days: 1,
            ..AlarmConfig::default()
        });
        let mut ev = raise("cleanup:done", Severity::Info, 1_000);
        if let AlarmEvent::Raise { ttl_ms, .. } = &mut ev {
            *ttl_ms = Some(5_000);
        }
        let id = e.apply(ev)[0].alarm.id;
        assert!(e.tick(Timestamp(5_000)).is_empty());
        let c = e.tick(Timestamp(6_000));
        assert_eq!(c[0].kind, ChangeKind::Returned);
        e.apply(AlarmEvent::Acknowledge {
            ids: vec![id],
            at: Timestamp(7_000),
        });
        assert_eq!(state(&e, id), AlarmState::Cleared);
        let c = e.tick(Timestamp(7_000 + 2 * DAY));
        assert_eq!(c[0].kind, ChangeKind::Pruned);
        assert!(e.alarm(id).is_none());
    }

    #[test]
    fn the_list_is_bounded() {
        let mut e = AlarmEngine::new(AlarmConfig {
            max_alarms: 50,
            ..AlarmConfig::default()
        });
        for i in 0..60 {
            e.apply(raise(&format!("r{i}:k"), Severity::Info, 1_000 + i));
        }
        assert_eq!(e.alarms().len(), 50);
        // Cleared alarms go first.
        let mut e = AlarmEngine::new(AlarmConfig {
            max_alarms: 50,
            ..AlarmConfig::default()
        });
        let first = e.apply(raise("old:k", Severity::Info, 1))[0].alarm.id;
        e.apply(AlarmEvent::Acknowledge {
            ids: vec![first],
            at: Timestamp(2),
        });
        e.apply(ret("old:k", 3));
        for i in 0..50 {
            e.apply(raise(&format!("r{i}:k"), Severity::Info, 1_000 + i));
        }
        assert!(e.alarm(first).is_none());
        assert_eq!(e.alarms().len(), 50);
    }

    #[test]
    fn restore_continues_ids_and_clamps_config() {
        let mut e = AlarmEngine::default();
        e.apply(raise("a:1", Severity::Info, 1));
        e.apply(AlarmEvent::Mute {
            rule: "a".into(),
            muted: true,
        });
        let r = AlarmEngine::restore(AlarmConfig::default(), e.alarms(), e.muted_rules());
        assert_eq!(r.alarms().len(), 1);
        assert_eq!(r.muted_rules(), vec!["a".to_string()]);
        let mut r = r;
        assert_eq!(r.apply(raise("b:1", Severity::Info, 2))[0].alarm.id, 2);
        let c = AlarmConfig {
            chatter_transitions: 0,
            chatter_window_minutes: 0,
            max_alarms: 0,
            keep_cleared_days: 0,
            utc_offset_minutes: 99_999,
            quiet: QuietHours {
                start_minute: 9_999,
                end_minute: 9_999,
                ..QuietHours::default()
            },
        }
        .clamped();
        assert_eq!(
            (c.chatter_transitions, c.chatter_window_minutes, c.max_alarms, c.keep_cleared_days),
            (3, 1, 50, 1)
        );
        assert_eq!((c.quiet.start_minute, c.quiet.end_minute), (1_439, 1_439));
        assert_eq!(c.utc_offset_minutes, 840);
        assert_eq!(c.utc_offset_minutes, 840);
        assert_eq!(AlarmEngine::default().alarm(1), None);
    }

    #[test]
    fn filters_and_serde() {
        let mut e = AlarmEngine::default();
        let a = e.apply(raise("cpu:a", Severity::Warning, 1))[0].alarm.id;
        e.apply(raise("disk:b", Severity::Critical, 2));
        e.apply(AlarmEvent::Shelve {
            id: a,
            how: ShelveFor::OneHour,
            at: Timestamp(3),
        });
        let now = Timestamp(4);
        let all = e.alarms();
        assert_eq!(all[0].key, "disk:b", "newest first");
        assert_eq!(AlarmFilter::default().apply(all.clone(), now).len(), 1, "shelved hidden");
        assert_eq!(
            AlarmFilter {
                include_shelved: true,
                ..Default::default()
            }
            .apply(all.clone(), now)
            .len(),
            2
        );
        assert_eq!(
            AlarmFilter {
                include_shelved: true,
                min_severity: Some(Severity::Critical),
                ..Default::default()
            }
            .apply(all.clone(), now)
            .len(),
            1
        );
        assert_eq!(
            AlarmFilter {
                include_shelved: true,
                rule: Some("cpu".into()),
                ..Default::default()
            }
            .apply(all.clone(), now)
            .len(),
            1
        );
        assert_eq!(
            AlarmFilter {
                include_shelved: true,
                states: vec![AlarmState::Cleared],
                ..Default::default()
            }
            .apply(all.clone(), now)
            .len(),
            0
        );
        assert_eq!(
            AlarmFilter {
                include_shelved: true,
                limit: 1,
                ..Default::default()
            }
            .apply(all.clone(), now)
            .len(),
            1
        );
        let j = serde_json::to_value(&all[0]).unwrap();
        assert_eq!(j["state"], "active");
        assert_eq!(j["firstSeen"], 2);
        assert_eq!(j["metric"], "cpuTotal");
        assert_eq!(
            serde_json::to_value(ShelveFor::Custom(Timestamp(5))).unwrap(),
            serde_json::json!({"custom": 5})
        );
        assert_eq!(serde_json::to_value(Shelf::UntilReboot).unwrap(), "untilReboot");
        let back: Alarm = serde_json::from_value(j).unwrap();
        assert_eq!(back, all[0]);
        let p = postcard::to_stdvec(&all[1]).unwrap();
        assert_eq!(postcard::from_bytes::<Alarm>(&p).unwrap(), all[1]);
        assert!(AlarmState::Active.is_open() && !AlarmState::Cleared.is_open());
    }
}
