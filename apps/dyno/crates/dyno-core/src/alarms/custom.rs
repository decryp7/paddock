//! Custom alert rules: "raise an alarm when this metric is above (or below) a limit for this long". Pure: the agent
//! passes each system sample and the time in; the evaluator returns the observations (raise and return) the alarm
//! engine turns into alarms. Rules ship none and every rule can be switched off.

use super::feed::{from_threshold, Observation};
use crate::health::Severity;
use crate::metrics::{MemoryPressure, SystemSample};
use crate::time::{Timestamp, SECOND};
use crate::widgets::Metric;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Most rules kept.
pub const MAX_RULES: usize = 50;
/// Longest rule name and id, in characters.
pub const MAX_NAME: usize = 80;
/// Longest "for" time, seconds.
pub const MAX_FOR_SECS: u32 = 3_600;

/// What a rule measures. Every value is a plain number: percent, or degrees Celsius for the temperature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CustomMetric {
    /// Total CPU load, percent.
    CpuTotal,
    /// Memory in use, percent of installed.
    MemoryUsed,
    /// The OS's memory pressure as 0 (normal), 50 (elevated) or 100 (critical).
    MemoryPressure,
    /// Swap or page file in use, percent of its size.
    Swap,
    /// Free space on the emptiest volume, percent.
    DiskFree,
    /// GPU load, percent.
    GpuLoad,
    /// The hottest sensor, degrees Celsius.
    Temperature,
    /// Battery charge, percent.
    Battery,
}

impl CustomMetric {
    /// The widget metric this rule's alarm charts.
    pub fn widget(self) -> Metric {
        match self {
            CustomMetric::CpuTotal => Metric::CpuTotal,
            CustomMetric::MemoryUsed => Metric::Memory,
            CustomMetric::MemoryPressure => Metric::MemoryPressure,
            CustomMetric::Swap => Metric::Swap,
            CustomMetric::DiskFree => Metric::DiskFree,
            CustomMetric::GpuLoad => Metric::GpuLoad,
            CustomMetric::Temperature => Metric::Temperature,
            CustomMetric::Battery => Metric::Battery,
        }
    }

    /// The metric's current value in `s`, or `None` when this machine does not report it.
    pub fn value(self, s: &SystemSample) -> Option<f64> {
        let pct = |part: u64, whole: u64| (whole > 0).then(|| part as f64 * 100.0 / whole as f64);
        match self {
            CustomMetric::CpuTotal => Some(f64::from(s.cpu.total)),
            CustomMetric::MemoryUsed => pct(s.memory.used, s.memory.total),
            CustomMetric::MemoryPressure => s.memory.pressure.value().map(|p| match p {
                MemoryPressure::Normal => 0.0,
                MemoryPressure::Elevated => 50.0,
                MemoryPressure::Critical => 100.0,
            }),
            CustomMetric::Swap => pct(s.memory.swap_used, s.memory.swap_total),
            CustomMetric::DiskFree => s
                .disks
                .iter()
                .filter(|d| !d.removable)
                .filter_map(|d| pct(d.free, d.total))
                .min_by(|a, b| a.total_cmp(b)),
            CustomMetric::GpuLoad => s.gpu.value().map(|g| f64::from(g.utilization)),
            CustomMetric::Temperature => s.hottest().map(|t| f64::from(t.celsius)),
            CustomMetric::Battery => s.battery.value().map(|b| f64::from(b.charge)),
        }
    }
}

/// Which side of the limit raises the alarm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CustomOp {
    /// The value is above the limit.
    Above,
    /// The value is below the limit.
    Below,
}

/// One custom rule (`Settings.customAlerts`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomAlertRule {
    /// Stable id (the alarm's rule name): letters, digits, `-` and `_`.
    pub id: String,
    /// What the person calls it.
    pub name: String,
    /// What it measures.
    pub metric: CustomMetric,
    /// Above or below.
    pub op: CustomOp,
    /// The limit.
    pub limit: f64,
    /// Seconds the condition must hold before the alarm is raised (0..=3600).
    pub for_secs: u32,
    /// How bad.
    pub severity: Severity,
    /// Switched on.
    pub enabled: bool,
}

impl CustomAlertRule {
    /// Whether the rule is well formed (what `normalise_rules` keeps).
    fn is_valid(&self) -> bool {
        !self.id.is_empty()
            && self.id.chars().count() <= MAX_NAME
            && self.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            && !self.name.trim().is_empty()
            && self.limit.is_finite()
    }

    fn crossed(&self, value: f64) -> bool {
        match self.op {
            CustomOp::Above => value > self.limit,
            CustomOp::Below => value < self.limit,
        }
    }
}

/// The rules with every value in range: malformed rules and duplicate ids are dropped, names trimmed and bounded,
/// times clamped, and at most [`MAX_RULES`] kept.
pub fn normalise_rules(rules: Vec<CustomAlertRule>) -> Vec<CustomAlertRule> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for mut r in rules {
        r.name = r.name.trim().chars().take(MAX_NAME).collect();
        r.for_secs = r.for_secs.min(MAX_FOR_SECS);
        if r.is_valid() && seen.insert(r.id.clone()) {
            out.push(r);
        }
        if out.len() >= MAX_RULES {
            break;
        }
    }
    out
}

/// Adds `rule`, or replaces the rule with the same id. The rule is checked (a malformed one is refused with what to fix)
/// and its name and time are bounded like `normalise_rules` does; at most [`MAX_RULES`] rules are kept.
pub fn upsert_rule(rules: &mut Vec<CustomAlertRule>, mut rule: CustomAlertRule) -> Result<(), String> {
    rule.name = rule.name.trim().chars().take(MAX_NAME).collect();
    rule.for_secs = rule.for_secs.min(MAX_FOR_SECS);
    if !rule.is_valid() {
        return Err("The rule needs a name, a limit that is a number, and an id of letters, digits, - and _.".to_string());
    }
    if let Some(i) = rules.iter().position(|r| r.id == rule.id) {
        rules[i] = rule;
    } else if rules.len() >= MAX_RULES {
        return Err(format!("There can be at most {MAX_RULES} custom rules; delete one first."));
    } else {
        rules.push(rule);
    }
    Ok(())
}

/// Removes the rule with `id`; refuses an id no rule has.
pub fn delete_rule(rules: &mut Vec<CustomAlertRule>, id: &str) -> Result<(), String> {
    let before = rules.len();
    rules.retain(|r| r.id != id);
    if rules.len() == before {
        return Err("That rule no longer exists.".to_string());
    }
    Ok(())
}

/// Remembers how long each rule's condition has held and which rules are raised.
#[derive(Debug, Default)]
pub struct CustomEvaluator {
    held_since: HashMap<String, Timestamp>,
    raised: HashMap<String, f64>,
}

impl CustomEvaluator {
    /// An evaluator with nothing held.
    pub fn new() -> CustomEvaluator {
        CustomEvaluator::default()
    }

    /// Evaluates `rules` against `sample` at `now`: a rule whose condition has held for its time raises (every tick,
    /// which the alarm engine counts as a repeat), one whose condition ended (or that was switched off or removed)
    /// returns. A metric the machine does not report never raises.
    pub fn evaluate(&mut self, now: Timestamp, rules: &[CustomAlertRule], sample: &SystemSample) -> Vec<Observation> {
        let mut out = Vec::new();
        let mut active: Vec<&str> = Vec::new();
        for r in rules.iter().filter(|r| r.enabled) {
            active.push(&r.id);
            let value = r.metric.value(sample);
            let crossed = value.is_some_and(|v| r.crossed(v));
            if crossed {
                let since = *self.held_since.entry(r.id.clone()).or_insert(now);
                if now.since(since) >= i64::from(r.for_secs) * SECOND {
                    let v = value.unwrap_or_default();
                    self.raised.insert(r.id.clone(), r.limit);
                    out.push(from_threshold(&r.id, r.metric.widget(), r.severity, v, r.limit, true).titled(&r.name));
                }
            } else {
                self.held_since.remove(&r.id);
                if self.raised.remove(&r.id).is_some() {
                    let v = value.unwrap_or(r.limit);
                    out.push(from_threshold(&r.id, r.metric.widget(), r.severity, v, r.limit, false).titled(&r.name));
                }
            }
        }
        // Rules switched off or deleted while raised: let their alarms return.
        let gone: Vec<String> = self.raised.keys().filter(|id| !active.contains(&id.as_str())).cloned().collect();
        for id in gone {
            if let Some(limit) = self.raised.remove(&id) {
                out.push(from_threshold(&id, Metric::CpuTotal, Severity::Info, limit, limit, false));
            }
            self.held_since.remove(&id);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::{CpuSample, DiskIo, MemorySample, Reading};

    fn sample(cpu: f32) -> SystemSample {
        SystemSample {
            at: Timestamp(0),
            cpu: CpuSample {
                total: cpu,
                ..Default::default()
            },
            memory: MemorySample {
                total: 1000,
                used: 900,
                available: 100,
                commit: Reading::Unavailable("n".into()),
                swap_used: 10,
                swap_total: 0,
                pressure: Reading::Available(MemoryPressure::Elevated),
            },
            disks: vec![],
            disk_io: DiskIo {
                read_bps: 0.0,
                write_bps: 0.0,
                queue: Reading::Unavailable("n".into()),
                busy: Reading::Unavailable("n".into()),
            },
            network: vec![],
            gpu: Reading::Unavailable("none".into()),
            battery: Reading::Unavailable("none".into()),
            temperatures: Reading::Unavailable("none".into()),
            fans: Reading::Unavailable("none".into()),
            uptime_secs: 0,
        }
    }

    fn rule(id: &str, metric: CustomMetric, op: CustomOp, limit: f64, for_secs: u32) -> CustomAlertRule {
        CustomAlertRule {
            id: id.into(),
            name: format!("Rule {id}"),
            metric,
            op,
            limit,
            for_secs,
            severity: Severity::Warning,
            enabled: true,
        }
    }

    #[test]
    fn upserting_adds_replaces_and_refuses_what_is_malformed() {
        let mut rules = Vec::new();
        upsert_rule(&mut rules, rule("a", CustomMetric::CpuTotal, CustomOp::Above, 90.0, 5)).unwrap();
        let mut again = rule("a", CustomMetric::CpuTotal, CustomOp::Above, 80.0, 99_999);
        again.name = "  Busy  ".into();
        upsert_rule(&mut rules, again).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(
            (rules[0].limit, rules[0].name.as_str(), rules[0].for_secs),
            (80.0, "Busy", MAX_FOR_SECS)
        );
        assert!(upsert_rule(&mut rules, rule("bad id", CustomMetric::CpuTotal, CustomOp::Above, 1.0, 0)).is_err());
        assert!(upsert_rule(&mut rules, rule("n", CustomMetric::CpuTotal, CustomOp::Above, f64::NAN, 0)).is_err());
        assert_eq!(rules.len(), 1);
        for i in 0..MAX_RULES {
            let _ = upsert_rule(&mut rules, rule(&format!("r{i}"), CustomMetric::CpuTotal, CustomOp::Above, 1.0, 0));
        }
        assert_eq!(rules.len(), MAX_RULES);
        assert!(upsert_rule(&mut rules, rule("one-more", CustomMetric::CpuTotal, CustomOp::Above, 1.0, 0)).is_err());
    }

    #[test]
    fn deleting_removes_one_rule_and_refuses_an_unknown_id() {
        let mut rules = vec![rule("a", CustomMetric::CpuTotal, CustomOp::Above, 1.0, 0)];
        assert!(delete_rule(&mut rules, "b").is_err());
        delete_rule(&mut rules, "a").unwrap();
        assert!(rules.is_empty());
    }

    #[test]
    fn a_rule_raises_only_after_the_condition_held_long_enough_and_returns_when_it_ends() {
        let rules = [rule("hot-cpu", CustomMetric::CpuTotal, CustomOp::Above, 90.0, 10)];
        let mut e = CustomEvaluator::new();
        assert!(e.evaluate(Timestamp(0), &rules, &sample(95.0)).is_empty(), "just started holding");
        assert!(e.evaluate(Timestamp(9_000), &rules, &sample(95.0)).is_empty());
        let raised = e.evaluate(Timestamp(10_000), &rules, &sample(96.0));
        assert_eq!(raised.len(), 1);
        assert!(raised[0].active && raised[0].title.contains("Rule hot-cpu"));
        assert_eq!(raised[0].value, Some(96.0));
        assert_eq!(
            e.evaluate(Timestamp(11_000), &rules, &sample(97.0)).len(),
            1,
            "still raised: a repeat"
        );
        let back = e.evaluate(Timestamp(12_000), &rules, &sample(10.0));
        assert_eq!(back.len(), 1);
        assert!(!back[0].active);
        assert!(e.evaluate(Timestamp(13_000), &rules, &sample(10.0)).is_empty());
        // A dip resets the clock.
        e.evaluate(Timestamp(20_000), &rules, &sample(95.0));
        e.evaluate(Timestamp(25_000), &rules, &sample(5.0));
        assert!(e.evaluate(Timestamp(31_000), &rules, &sample(95.0)).is_empty());
    }

    #[test]
    fn below_rules_unreported_metrics_and_zero_wait() {
        let rules = [
            rule("low-mem", CustomMetric::MemoryUsed, CustomOp::Below, 95.0, 0),
            rule("gpu", CustomMetric::GpuLoad, CustomOp::Above, 1.0, 0),
            rule("pressure", CustomMetric::MemoryPressure, CustomOp::Above, 40.0, 0),
            rule("swap", CustomMetric::Swap, CustomOp::Above, 0.0, 0),
        ];
        let out = CustomEvaluator::new().evaluate(Timestamp(0), &rules, &sample(1.0));
        let names: Vec<_> = out.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["low-mem", "pressure"],
            "no GPU reading and no swap size: those never raise"
        );
    }

    #[test]
    fn switching_a_raised_rule_off_or_deleting_it_returns_its_alarm() {
        let mut rules = vec![rule("a", CustomMetric::CpuTotal, CustomOp::Above, 1.0, 0)];
        let mut e = CustomEvaluator::new();
        assert_eq!(e.evaluate(Timestamp(0), &rules, &sample(50.0)).len(), 1);
        rules[0].enabled = false;
        let out = e.evaluate(Timestamp(1_000), &rules, &sample(50.0));
        assert_eq!(out.len(), 1);
        assert!(!out[0].active);
        assert!(e.evaluate(Timestamp(2_000), &[], &sample(50.0)).is_empty());
        rules[0].enabled = true;
        e.evaluate(Timestamp(3_000), &rules, &sample(50.0));
        let out = e.evaluate(Timestamp(4_000), &[], &sample(50.0));
        assert!(!out[0].active, "a deleted rule's alarm returns");
    }

    #[test]
    fn rules_are_normalised() {
        let mut long = rule("ok", CustomMetric::Battery, CustomOp::Below, 20.0, 99_999);
        long.name = format!("  {}  ", "n".repeat(200));
        let mut rules = vec![
            long,
            rule("ok", CustomMetric::Battery, CustomOp::Below, 5.0, 0),
            rule("bad id!", CustomMetric::Battery, CustomOp::Below, 5.0, 0),
            rule("nan", CustomMetric::Battery, CustomOp::Below, f64::NAN, 0),
            rule("", CustomMetric::Battery, CustomOp::Below, 5.0, 0),
        ];
        rules.push(CustomAlertRule {
            name: "  ".into(),
            ..rule("blank", CustomMetric::CpuTotal, CustomOp::Above, 1.0, 0)
        });
        let n = normalise_rules(rules);
        assert_eq!(n.len(), 1);
        assert_eq!((n[0].for_secs, n[0].name.chars().count()), (MAX_FOR_SECS, MAX_NAME));
        let many: Vec<_> = (0..80)
            .map(|i| rule(&format!("r{i}"), CustomMetric::CpuTotal, CustomOp::Above, 1.0, 0))
            .collect();
        assert_eq!(normalise_rules(many).len(), MAX_RULES);
    }

    #[test]
    fn serde_names_match_the_ui() {
        let r = rule("x", CustomMetric::GpuLoad, CustomOp::Below, 1.0, 5);
        let j = serde_json::to_value(&r).unwrap();
        assert_eq!(j["metric"], "gpuLoad");
        assert_eq!(j["op"], "below");
        assert_eq!(j["forSecs"], 5);
        assert_eq!(serde_json::from_value::<CustomAlertRule>(j).unwrap(), r);
    }
}
