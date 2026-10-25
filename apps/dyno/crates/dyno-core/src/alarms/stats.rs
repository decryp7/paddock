//! Alarm statistics: which rules are noisiest, how fast alarms get acknowledged, and which rules to tune or retire.

use super::{Alarm, AlarmState};
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

/// A rule is only judged after this many occurrences.
pub const MIN_OCCURRENCES: u32 = 10;

/// What to do about a noisy rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SuggestionKind {
    /// It fires often: raise its threshold or the time it must hold.
    Tune,
    /// Nobody acts on it: turn it off.
    Retire,
}

/// Advice about one rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    /// The rule.
    pub rule: String,
    /// What to do.
    pub kind: SuggestionKind,
    /// A sentence for the user.
    pub message: String,
}

/// How often a rule produced an alarm.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleCount {
    /// The rule.
    pub rule: String,
    /// Occurrences (new alarms and returns of an unacknowledged one).
    pub occurrences: u32,
}

/// The statistics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AlarmStats {
    /// Alarms not cleared.
    pub open: u32,
    /// Alarms waiting for acknowledgement.
    pub unacknowledged: u32,
    /// Keys of alarms flagged as chattering.
    pub chattering: Vec<String>,
    /// The five noisiest rules.
    pub noisiest: Vec<RuleCount>,
    /// Mean time from first seen to acknowledged, milliseconds.
    pub mean_time_to_acknowledge_ms: Option<i64>,
    /// Median of the same.
    pub median_time_to_acknowledge_ms: Option<i64>,
    /// Rules to tune or retire.
    pub suggestions: Vec<Suggestion>,
}

/// Computes the statistics from the alarm list, the per-rule occurrence counts and the acknowledge times.
pub fn compute(alarms: &[Alarm], raises: &HashMap<String, u32>, acks: &VecDeque<(String, i64)>, _now: Timestamp) -> AlarmStats {
    let mut noisiest: Vec<RuleCount> = raises
        .iter()
        .map(|(rule, n)| RuleCount {
            rule: rule.clone(),
            occurrences: *n,
        })
        .collect();
    noisiest.sort_by(|a, b| b.occurrences.cmp(&a.occurrences).then(a.rule.cmp(&b.rule)));
    let mut times: Vec<i64> = acks.iter().map(|(_, ms)| *ms).collect();
    times.sort_unstable();
    let mean = (!times.is_empty()).then(|| times.iter().sum::<i64>() / times.len() as i64);
    let median = times.get(times.len() / 2).copied();
    let mut chattering: Vec<String> = alarms
        .iter()
        .filter(|a| a.chattering && a.state.is_open())
        .map(|a| a.key.clone())
        .collect();
    chattering.sort();

    let mut suggestions = Vec::new();
    for rc in noisiest.iter().filter(|r| r.occurrences >= MIN_OCCURRENCES) {
        let acked = acks.iter().filter(|(r, _)| *r == rc.rule).count() as u32;
        let flaps = alarms.iter().any(|a| a.rule == rc.rule && a.chattering);
        let (kind, message) = if flaps {
            (
                SuggestionKind::Tune,
                format!(
                    "\"{}\" keeps switching on and off. Raise its threshold or make it hold for longer before it alarms.",
                    rc.rule
                ),
            )
        } else if acked * 5 < rc.occurrences {
            (
                SuggestionKind::Retire,
                format!(
                    "\"{}\" went off {} times and you acknowledged {acked}. If it is not useful, turn it off.",
                    rc.rule, rc.occurrences
                ),
            )
        } else {
            (
                SuggestionKind::Tune,
                format!(
                    "\"{}\" went off {} times. A higher threshold would make it worth more.",
                    rc.rule, rc.occurrences
                ),
            )
        };
        suggestions.push(Suggestion {
            rule: rc.rule.clone(),
            kind,
            message,
        });
    }
    noisiest.truncate(5);
    AlarmStats {
        open: alarms.iter().filter(|a| a.state.is_open()).count() as u32,
        unacknowledged: alarms
            .iter()
            .filter(|a| matches!(a.state, AlarmState::Active | AlarmState::ClearedUnacknowledged))
            .count() as u32,
        chattering,
        noisiest,
        mean_time_to_acknowledge_ms: mean,
        median_time_to_acknowledge_ms: median,
        suggestions,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{AlarmEngine, AlarmEvent};
    use super::*;
    use crate::health::Severity;
    use crate::time::MINUTE;

    fn raise(rule: &str, key: &str, at: i64) -> AlarmEvent {
        AlarmEvent::Raise {
            rule: rule.into(),
            key: key.into(),
            severity: Severity::Warning,
            title: "t".into(),
            detail: String::new(),
            value: None,
            metric: None,
            ttl_ms: None,
            at: Timestamp(at),
        }
    }

    #[test]
    fn empty_engine_has_empty_stats() {
        let s = AlarmEngine::default().stats(Timestamp(0));
        assert_eq!(s, AlarmStats::default());
    }

    #[test]
    fn times_noise_and_suggestions() {
        let mut e = AlarmEngine::default();
        // "noisy": 12 distinct alarms, none acknowledged -> retire. "tuned": 10, all acknowledged -> tune.
        for i in 0..12 {
            e.apply(raise("noisy", &format!("noisy:{i}"), i * 1_000));
        }
        for i in 0..10 {
            let id = e.apply(raise("tuned", &format!("tuned:{i}"), 100_000 + i * MINUTE))[0].alarm.id;
            e.apply(AlarmEvent::Acknowledge {
                ids: vec![id],
                at: Timestamp(100_000 + i * MINUTE + 30_000),
            });
        }
        e.apply(raise("rare", "rare:1", 1));
        let s = e.stats(Timestamp(0));
        assert_eq!((s.open, s.unacknowledged), (23, 13));
        assert_eq!(
            s.noisiest[0],
            RuleCount {
                rule: "noisy".into(),
                occurrences: 12
            }
        );
        assert_eq!(s.noisiest.len(), 3);
        assert_eq!(s.mean_time_to_acknowledge_ms, Some(30_000));
        assert_eq!(s.median_time_to_acknowledge_ms, Some(30_000));
        let kinds: Vec<(&str, SuggestionKind)> = s.suggestions.iter().map(|x| (x.rule.as_str(), x.kind)).collect();
        assert_eq!(kinds, [("noisy", SuggestionKind::Retire), ("tuned", SuggestionKind::Tune)]);
        assert!(s.suggestions[0].message.contains("acknowledged 0"));
    }

    #[test]
    fn flapping_rules_get_a_tune_suggestion() {
        let mut e = AlarmEngine::default();
        for i in 0..12 {
            e.apply(raise("flap", "flap:1", i * 10_000));
            e.apply(AlarmEvent::Return {
                key: "flap:1".into(),
                at: Timestamp(i * 10_000 + 5_000),
            });
        }
        let s = e.stats(Timestamp(0));
        assert_eq!(s.chattering, vec!["flap:1".to_string()]);
        let f = s.suggestions.iter().find(|x| x.rule == "flap").unwrap();
        assert_eq!(f.kind, SuggestionKind::Tune);
        assert!(f.message.contains("switching on and off"));
    }
}
