//! Feeds: turn what Dyno already produces (alerts, rule notifications, custom thresholds, issues, taming and cleanup
//! events) into alarm events through one generic [`Observation`].

use super::AlarmEvent;
use crate::alerts::{Alert, AlertKind};
use crate::health::Severity;
use crate::time::{Timestamp, MINUTE};
use crate::widgets::Metric;

/// How long a one-off event (an alert that fires once, a cleanup that ran) stays an open alarm by itself.
pub const EVENT_TTL_MS: i64 = 30 * MINUTE;

/// Where an observation comes from; it becomes the alarm's rule prefix, so muting and the statistics group by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// One of the built-in alerts.
    Alert,
    /// A health issue.
    Issue,
    /// A custom threshold on a widget or metric.
    Threshold,
    /// A rule that tamed an app.
    Taming,
    /// A cleanup that ran or found something.
    Cleanup,
    /// Anything else (plugins).
    Custom,
}

impl Source {
    fn prefix(self) -> &'static str {
        match self {
            Source::Alert => "alert",
            Source::Issue => "issue",
            Source::Threshold => "threshold",
            Source::Taming => "taming",
            Source::Cleanup => "cleanup",
            Source::Custom => "custom",
        }
    }
}

/// A condition as some part of Dyno sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    /// Where it comes from.
    pub source: Source,
    /// The specific rule within the source ("lowDisk", "my-gpu-limit").
    pub name: String,
    /// The condition's identity within the rule (a disk, an app); empty when the rule has one condition.
    pub subject: String,
    /// How bad.
    pub severity: Severity,
    /// One line.
    pub title: String,
    /// Detail.
    pub detail: String,
    /// The measured value.
    pub value: Option<f64>,
    /// The metric to chart.
    pub metric: Option<Metric>,
    /// The condition currently holds (raise) or has gone (return).
    pub active: bool,
    /// A one-off event rather than a continuing condition: it clears itself after [`EVENT_TTL_MS`].
    pub one_off: bool,
}

impl Observation {
    /// The rule key, e.g. "threshold:my-gpu-limit".
    pub fn rule(&self) -> String {
        format!("{}:{}", self.source.prefix(), self.name)
    }

    /// The condition key, e.g. "threshold:my-gpu-limit:gpu0".
    pub fn key(&self) -> String {
        if self.subject.is_empty() {
            self.rule()
        } else {
            format!("{}:{}", self.rule(), self.subject)
        }
    }

    /// The alarm event this observation stands for at `at`.
    pub fn into_event(self, at: Timestamp) -> AlarmEvent {
        if !self.active {
            return AlarmEvent::Return { key: self.key(), at };
        }
        AlarmEvent::Raise {
            rule: self.rule(),
            key: self.key(),
            severity: self.severity,
            title: self.title,
            detail: self.detail,
            value: self.value,
            metric: self.metric,
            ttl_ms: self.one_off.then_some(EVENT_TTL_MS),
            at,
        }
    }
}

fn alert_name(kind: AlertKind) -> &'static str {
    match kind {
        AlertKind::LowDisk => "lowDisk",
        AlertKind::Overheating => "overheating",
        AlertKind::MemoryPressure => "memoryPressure",
        AlertKind::CrashLoop => "crashLoop",
        AlertKind::LongHighCpu => "longHighCpu",
    }
}

fn alert_metric(kind: AlertKind) -> Option<Metric> {
    match kind {
        AlertKind::LowDisk => Some(Metric::DiskFree),
        AlertKind::Overheating => Some(Metric::Temperature),
        AlertKind::MemoryPressure => Some(Metric::MemoryPressure),
        AlertKind::CrashLoop => None,
        AlertKind::LongHighCpu => Some(Metric::CpuTotal),
    }
}

/// An existing [`Alert`] as an observation (alerts fire once per cool-down, so they are one-off events; the title
/// identifies the disk or app).
pub fn from_alert(alert: &Alert) -> Observation {
    Observation {
        source: Source::Alert,
        name: alert_name(alert.kind).into(),
        subject: alert.title.clone(),
        severity: alert.severity,
        title: alert.title.clone(),
        detail: alert.detail.clone(),
        value: None,
        metric: alert_metric(alert.kind),
        active: true,
        one_off: true,
    }
}

/// A rule's "Notify" step firing for `app`.
pub fn from_rule_notify(rule_id: &str, rule_name: &str, app: &str) -> Observation {
    Observation {
        source: Source::Taming,
        name: rule_id.into(),
        subject: app.into(),
        severity: Severity::Info,
        title: format!("{rule_name}: {app}"),
        detail: format!("The rule \"{rule_name}\" notified you about {app}."),
        value: None,
        metric: Some(Metric::Process(app.into())),
        active: true,
        one_off: true,
    }
}

/// A user threshold crossed (or no longer crossed) by `value`.
pub fn from_threshold(name: &str, metric: Metric, severity: Severity, value: f64, limit: f64, crossed: bool) -> Observation {
    Observation {
        source: Source::Threshold,
        name: name.into(),
        subject: String::new(),
        severity,
        title: format!("{name} is past its limit"),
        detail: format!("The value is {value:.1}; the limit is {limit:.1}."),
        value: Some(value),
        metric: Some(metric),
        active: crossed,
        one_off: false,
    }
}

/// A finished cleanup (or one that found something) as an informational event.
pub fn from_cleanup(title: &str, detail: &str) -> Observation {
    Observation {
        source: Source::Cleanup,
        name: "cleanup".into(),
        subject: title.into(),
        severity: Severity::Info,
        title: title.into(),
        detail: detail.into(),
        value: None,
        metric: None,
        active: true,
        one_off: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alarms::{AlarmEngine, AlarmState};

    fn alert(kind: AlertKind, title: &str) -> Alert {
        Alert {
            id: "alert-1".into(),
            at: Timestamp(5),
            kind,
            severity: Severity::Warning,
            title: title.into(),
            detail: "d".into(),
        }
    }

    #[test]
    fn alerts_become_one_off_alarms_that_expire() {
        let mut e = AlarmEngine::default();
        let o = from_alert(&alert(AlertKind::LowDisk, "C: is almost full"));
        assert_eq!(
            (o.rule().as_str(), o.key().as_str()),
            ("alert:lowDisk", "alert:lowDisk:C: is almost full")
        );
        let c = e.apply(o.clone().into_event(Timestamp(1_000)));
        assert!(c[0].notify);
        assert_eq!(c[0].alarm.metric, Some(Metric::DiskFree));
        assert!(c[0].alarm.expires_at.is_some());
        // The same alert again is a repeat, not a second alarm.
        e.apply(o.into_event(Timestamp(2_000)));
        assert_eq!(e.alarms().len(), 1);
        e.tick(Timestamp(2_000 + EVENT_TTL_MS + 1));
        assert_eq!(e.alarms()[0].state, AlarmState::ClearedUnacknowledged);
        for k in [
            AlertKind::Overheating,
            AlertKind::MemoryPressure,
            AlertKind::CrashLoop,
            AlertKind::LongHighCpu,
        ] {
            let o = from_alert(&alert(k, "t"));
            assert!(o.name.chars().next().unwrap().is_lowercase());
            assert_eq!(o.metric.is_some(), k != AlertKind::CrashLoop);
        }
    }

    #[test]
    fn thresholds_raise_and_return() {
        let mut e = AlarmEngine::default();
        let up = from_threshold("gpu-limit", Metric::GpuLoad, Severity::Warning, 97.0, 90.0, true);
        let c = e.apply(up.into_event(Timestamp(1)));
        assert_eq!(c[0].alarm.current_value, Some(97.0));
        assert!(c[0].alarm.expires_at.is_none());
        assert!(c[0].alarm.detail.contains("97.0") && c[0].alarm.detail.contains("90.0"));
        let down = from_threshold("gpu-limit", Metric::GpuLoad, Severity::Warning, 50.0, 90.0, false);
        let c = e.apply(down.into_event(Timestamp(2)));
        assert_eq!(c[0].alarm.state, AlarmState::ClearedUnacknowledged);
    }

    #[test]
    fn rules_issues_and_cleanup_events() {
        let n = from_rule_notify("tame-chrome", "Tame Chrome", "Chrome");
        assert_eq!(n.key(), "taming:tame-chrome:Chrome");
        assert_eq!(n.metric, Some(Metric::Process("Chrome".into())));
        let c = from_cleanup("Cleanup finished", "Freed 2 GB");
        assert_eq!(c.rule(), "cleanup:cleanup");
        assert_eq!(c.severity, Severity::Info);
        for s in [Source::Issue, Source::Custom, Source::Threshold] {
            let o = Observation {
                source: s,
                subject: String::new(),
                ..from_cleanup("x", "y")
            };
            assert_eq!(o.key(), o.rule());
        }
    }
}
