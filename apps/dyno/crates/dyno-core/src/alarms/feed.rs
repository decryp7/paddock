//! Feeds: turn what Dyno already produces (alerts, custom thresholds, issues and hardware findings) into alarm events through one generic [`Observation`].

use super::AlarmEvent;
use crate::alerts::{Alert, AlertKind};
use crate::health::Severity;
use crate::time::{Timestamp, MINUTE};
use crate::widgets::Metric;

/// How long a one-off event (an alert that fires once) stays an open alarm by itself.
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
    /// A network event: a new exposed listener or a new remote host.
    Network,
    /// The CPU running slower than it should (heat, a power limit or battery saver).
    Throttling,
    /// A disk or battery near the end of its life.
    Lifespan,
    /// A value far outside what is usual for this time of the week.
    Baseline,
    /// Anything else (plugins).
    Custom,
}

impl Source {
    fn prefix(self) -> &'static str {
        match self {
            Source::Alert => "alert",
            Source::Issue => "issue",
            Source::Threshold => "threshold",
            Source::Network => "network",
            Source::Throttling => "throttling",
            Source::Lifespan => "lifespan",
            Source::Baseline => "baseline",
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
    /// The same observation with the person's own name for the rule in its title.
    pub fn titled(mut self, name: &str) -> Observation {
        self.title = format!("{name} is past its limit");
        self
    }

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

/// A serious problem the OS logged (an unexpected shutdown, a disk, memory or heat error, running out of memory) as a
/// one-off event; one alarm per issue, so each one is acknowledged on its own. Ordinary crashes are not alarms by
/// themselves: a crash *loop* is, through the crash-loop alert.
pub fn from_issue(issue: &crate::issues::Issue) -> Observation {
    Observation {
        source: Source::Issue,
        name: format!("{:?}", issue.kind).to_lowercase(),
        subject: issue.id.clone(),
        severity: issue.severity,
        title: issue.title.clone(),
        detail: issue.evidence.clone(),
        value: None,
        metric: None,
        active: true,
        one_off: true,
    }
}

/// Whether an issue of this kind raises an alarm of its own.
pub fn alarms_on(kind: crate::issues::IssueKind) -> bool {
    use crate::issues::IssueKind as K;
    matches!(
        kind,
        K::BadShutdown | K::DiskError | K::MemoryError | K::Overheating | K::MemoryExhaustion
    )
}

/// A throttling episode that has lasted long enough to count; `active` is false once it ended. Heat is a warning (it can
/// damage a machine over time), the other causes are information.
pub fn from_throttle(
    e: &crate::throttle::ThrottleEpisode,
    active: bool,
    confidence: crate::hardware::Confidence,
    zone: &dyn crate::time::LocalZone,
) -> Observation {
    use crate::throttle::ThrottleCause;
    Observation {
        source: Source::Throttling,
        name: "cpu".into(),
        subject: String::new(),
        severity: if e.cause == ThrottleCause::Thermal {
            Severity::Warning
        } else {
            Severity::Info
        },
        title: format!(
            "The CPU is {:.0}% slower than it should be ({})",
            e.avg_lost_percent,
            e.cause.label().to_lowercase()
        ),
        detail: format!(
            "Throttling since {} with {} confidence. Open Throttling for the cause and what could help.",
            crate::report::local_time(e.start, zone),
            confidence.word()
        ),
        value: Some(f64::from(e.avg_lost_percent)),
        metric: Some(Metric::CpuTotal),
        active,
        one_off: false,
    }
}

/// A disk or battery that needs attention (`active`) or no longer does.
pub fn from_lifespan(f: &crate::lifespan::LifeForecast, active: bool) -> Observation {
    Observation {
        source: Source::Lifespan,
        name: match f.subject {
            crate::lifespan::LifeSubject::Ssd => "ssd".into(),
            crate::lifespan::LifeSubject::Battery => "battery".into(),
        },
        subject: f.name.clone(),
        severity: if matches!(f.outcome, crate::lifespan::LifeOutcome::AlreadyPastLimit) {
            Severity::Warning
        } else {
            Severity::Info
        },
        title: format!("{}: {}", f.name, f.headline),
        detail: format!("{} Confidence: {}.", f.explanation, f.confidence.word()),
        value: None,
        metric: matches!(f.subject, crate::lifespan::LifeSubject::Battery).then_some(Metric::Battery),
        active,
        one_off: false,
    }
}

/// A value that stays far outside what is usual for this time of the week.
pub fn from_baseline(
    subject: &str,
    metric: crate::baseline::BaselineMetric,
    current: f64,
    j: &crate::baseline::Judgement,
    active: bool,
) -> Observation {
    use crate::baseline::BaselineMetric as M;
    let app = subject != "System";
    Observation {
        source: Source::Baseline,
        name: format!("{metric:?}").to_lowercase(),
        subject: subject.into(),
        severity: Severity::Info,
        title: if app {
            format!("{subject}: {} {}", metric.label(), j.phrase.to_lowercase())
        } else {
            format!("{} - {}", metric.label(), j.phrase.to_lowercase())
        },
        detail: j.explanation.clone(),
        value: Some(current),
        metric: match (metric, app) {
            (_, true) => Some(Metric::Process(subject.into())),
            (M::Cpu, _) => Some(Metric::CpuTotal),
            (M::Memory, _) => Some(Metric::Memory),
            (M::Gpu, _) => Some(Metric::GpuLoad),
            (M::Temperature, _) => Some(Metric::Temperature),
            (M::DiskWrite, _) => Some(Metric::DiskActivity),
            (M::Network, _) => Some(Metric::NetworkDown),
            (M::AppCpu | M::AppMemory, _) => Some(Metric::Process(subject.into())),
        },
        active,
        one_off: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alarms::{AlarmConfig, AlarmEngine, AlarmState};

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
        let mut e = AlarmEngine::new(AlarmConfig::default(), crate::time::FixedZone::shared(0));
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
        let mut e = AlarmEngine::new(AlarmConfig::default(), crate::time::FixedZone::shared(0));
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
    fn serious_issues_become_one_alarm_each() {
        use crate::issues::{Issue, IssueKind, IssueSourceId};
        let mk = |k, key: &str| Issue::new(IssueSourceId::MacLog, Timestamp(5), k, "", "Disk error", "I/O error on disk3", key);
        let a = mk(IssueKind::DiskError, "1");
        let b = mk(IssueKind::DiskError, "2");
        let o = from_issue(&a);
        assert_eq!(o.rule(), "issue:diskerror");
        assert_ne!(o.key(), from_issue(&b).key());
        assert_eq!(o.severity, Severity::Critical);
        assert!(o.one_off && o.active);
        assert_eq!(o.detail, "I/O error on disk3");
        let mut e = AlarmEngine::new(AlarmConfig::default(), crate::time::FixedZone::shared(0));
        e.apply(o.into_event(Timestamp(5)));
        assert_eq!(e.alarms().len(), 1);
        assert!(alarms_on(IssueKind::BadShutdown));
        assert!(!alarms_on(IssueKind::AppCrash));
        assert!(!alarms_on(IssueKind::UpdateFailure));
    }

    #[test]
    fn observation_keys() {
        let o = from_threshold("hot", Metric::Process("a".into()), Severity::Warning, 5.0, 4.0, true);
        assert_eq!(o.rule(), "threshold:hot");
        for s in [Source::Issue, Source::Custom, Source::Threshold] {
            let o = Observation {
                source: s,
                subject: String::new(),
                ..o.clone()
            };
            assert_eq!(o.key(), o.rule());
        }
    }

    #[test]
    fn hardware_findings_become_observations() {
        use crate::baseline::{BaselineMetric, Judgement};
        use crate::hardware::Confidence;
        use crate::lifespan::{unavailable, LifeOutcome, LifeSubject};
        use crate::throttle::{ThrottleCause, ThrottleEpisode};
        let e = ThrottleEpisode {
            id: ThrottleEpisode::id_for(Timestamp(0)),
            start: Timestamp(0),
            end: None,
            cause: ThrottleCause::Thermal,
            avg_lost_percent: 31.6,
            peak_lost_percent: 40.0,
            peak_temperature_c: Some(99.0),
        };
        let o = from_throttle(&e, true, Confidence::High, &crate::time::FixedZone::UTC);
        assert_eq!((o.rule().as_str(), o.key().as_str()), ("throttling:cpu", "throttling:cpu"));
        assert_eq!(o.severity, Severity::Warning);
        assert!(
            o.title.contains("32% slower") && o.title.contains("heat") && o.detail.contains("high confidence"),
            "{o:?}"
        );
        let power = from_throttle(
            &ThrottleEpisode {
                cause: ThrottleCause::Power,
                ..e.clone()
            },
            false,
            Confidence::Medium,
            &crate::time::FixedZone::UTC,
        );
        assert_eq!(power.severity, Severity::Info);
        assert!(!power.active);

        let mut f = unavailable(LifeSubject::Battery, "Battery", "n/a");
        f.outcome = LifeOutcome::AlreadyPastLimit;
        f.headline = "Time to think about a new battery".into();
        let o = from_lifespan(&f, true);
        assert_eq!(o.key(), "lifespan:battery:Battery");
        assert_eq!(o.severity, Severity::Warning);
        assert_eq!(o.metric, Some(Metric::Battery));
        f.subject = LifeSubject::Ssd;
        assert_eq!(from_lifespan(&f, false).rule(), "lifespan:ssd");

        let j = Judgement::not_judged("Higher than usual for Monday nights", "CPU is 72% now.");
        let o = from_baseline("System", BaselineMetric::Cpu, 72.0, &j, true);
        assert_eq!(o.key(), "baseline:cpu:System");
        assert_eq!(o.metric, Some(Metric::CpuTotal));
        assert!(o.title.starts_with("CPU - higher than usual"));
        let app = from_baseline("Chrome", BaselineMetric::Memory, 40.0, &j, true);
        assert_eq!(app.metric, Some(Metric::Process("Chrome".into())));
        assert!(app.title.starts_with("Chrome: Memory"));
        for (m, want) in [
            (BaselineMetric::Memory, Metric::Memory),
            (BaselineMetric::Gpu, Metric::GpuLoad),
            (BaselineMetric::Temperature, Metric::Temperature),
            (BaselineMetric::DiskWrite, Metric::DiskActivity),
            (BaselineMetric::Network, Metric::NetworkDown),
        ] {
            assert_eq!(from_baseline("System", m, 1.0, &j, true).metric, Some(want));
        }
    }
}
