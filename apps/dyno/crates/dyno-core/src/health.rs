//! The health score: 100 minus the points each problem costs, with the reasons.

use crate::alarms::{Alarm, AlarmState};
use crate::anomaly::Anomaly;
use crate::facts::{DiskHealthStatus, SystemFacts};
use crate::forecast::Forecast;
use crate::metrics::{derive_pressure, percent, MemoryPressure, SystemSample};
use crate::startup::{ImpactRating, StartupEntry};
use crate::time::Timestamp;
use crate::units::{bytes, duration, storage};
use serde::{Deserialize, Serialize};

/// The score in a word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Grade {
    /// 80–100.
    Good,
    /// 60–79.
    Fair,
    /// Below 60.
    Poor,
}

impl Grade {
    /// The highest score in this band: the one place that maps a band to a number (`grade` is its inverse).
    pub fn top_score(self) -> u32 {
        match self {
            Grade::Good => 100,
            Grade::Fair => 79,
            Grade::Poor => 59,
        }
    }
}

/// How bad a reason is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    /// Worth knowing.
    Info,
    /// Should be fixed.
    Warning,
    /// Fix it now.
    Critical,
}

/// One reason the score is not 100.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthReason {
    /// Id.
    pub id: String,
    /// One line.
    pub title: String,
    /// Detail.
    pub detail: String,
    /// Points it costs.
    pub impact: u32,
    /// Severity.
    pub severity: Severity,
    /// For an active alarm: when its condition first appeared. Performance reasons have none.
    pub since: Option<Timestamp>,
}

/// The report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthReport {
    /// When.
    pub at: Timestamp,
    /// 0–100.
    pub score: u32,
    /// In a word.
    pub grade: Grade,
    /// What performance alone scores, before active alarms cap it (equals `score` when none did).
    pub performance_score: u32,
    /// Set when an active alarm of this severity lowered the score: Warning caps it at Fair, Critical at Poor.
    pub capped_by: Option<Severity>,
    /// Reasons: each active alarm first (most severe, then oldest), then performance reasons, biggest impact first.
    pub reasons: Vec<HealthReason>,
}

/// Everything the score looks at.
pub struct HealthInput<'a> {
    /// Now.
    pub now: Timestamp,
    /// The latest sample.
    pub sample: &'a SystemSample,
    /// Slow facts.
    pub facts: &'a SystemFacts,
    /// The system volume's id.
    pub system_disk: &'a str,
    /// Disk forecasts (disk id, forecast).
    pub forecasts: &'a [(String, Forecast)],
    /// Startup entries.
    pub startup: &'a [StartupEntry],
    /// Current anomalies.
    pub anomalies: &'a [Anomaly],
    /// App crashes in the last 7 days.
    pub crashes_7d: u32,
    /// Disk and battery life forecasts (only ones that need attention cost points).
    pub lifespans: &'a [crate::lifespan::LifeForecast],
    /// The alarm list; those in [`AlarmState::Active`] (condition holds, not acknowledged, not shelved) cap the score.
    pub alarms: &'a [Alarm],
}

/// The band an active alarm of `severity` caps the score at, if any (Info does not cap).
fn cap_for(severity: Severity) -> Option<Grade> {
    match severity {
        Severity::Critical => Some(Grade::Poor),
        Severity::Warning => Some(Grade::Fair),
        Severity::Info => None,
    }
}

/// The grade for a score.
pub fn grade(score: u32) -> Grade {
    match score {
        80.. => Grade::Good,
        60..=79 => Grade::Fair,
        _ => Grade::Poor,
    }
}

/// Scores the machine.
pub fn assess(i: &HealthInput<'_>) -> HealthReport {
    let mut r: Vec<HealthReason> = Vec::new();
    let mut add = |id: &str, title: String, detail: String, impact: u32, severity: Severity| {
        r.push(HealthReason {
            id: id.into(),
            title,
            detail,
            impact,
            severity,
            since: None,
        });
    };

    if let Some(disks) = i.facts.disks.value() {
        for d in disks {
            let (impact, severity, word) = match d.status {
                DiskHealthStatus::Unhealthy => (40, Severity::Critical, "is failing"),
                DiskHealthStatus::Warning => (25, Severity::Critical, "reports problems"),
                _ => continue,
            };
            add(
                &format!("disk:{}", d.name),
                format!("The disk {} {word}", d.name),
                "Back up your files now and plan to replace the disk; its own health check reports trouble.".into(),
                impact,
                severity,
            );
        }
    }

    if let Some(d) = i.sample.disks.iter().find(|d| d.id.eq_ignore_ascii_case(i.system_disk)) {
        let free = percent(d.free, d.total);
        if free < 10.0 {
            let (impact, severity) = if free < 5.0 {
                (25, Severity::Critical)
            } else {
                (15, Severity::Warning)
            };
            add(
                "lowDisk",
                format!("{} has {} free ({free:.0}%)", d.name, storage(d.free)),
                "Windows and apps need free space for updates, the page file and temporary files.".into(),
                impact,
                severity,
            );
        }
        if let Some((_, Forecast::FullInDays(days))) = i.forecasts.iter().find(|(id, _)| id.eq_ignore_ascii_case(&d.id)) {
            if *days < 30.0 {
                add(
                    "diskFilling",
                    format!("{} will be full in about {days:.0} days", d.name),
                    "At the rate free space shrank over the last days.".into(),
                    if *days < 7.0 { 15 } else { 8 },
                    Severity::Warning,
                );
            }
        }
    }

    if let Some(b) = i.sample.battery.value() {
        if b.health < 80.0 {
            let (impact, severity) = if b.health < 60.0 {
                (20, Severity::Warning)
            } else {
                (10, Severity::Info)
            };
            add(
                "battery",
                format!("The battery holds {:.0}% of its original charge", b.health),
                match b.cycles {
                    Some(c) => format!("After {c} charge cycles. A worn battery runs out sooner and may slow the CPU."),
                    None => "A worn battery runs out sooner and may slow the CPU.".into(),
                },
                impact,
                severity,
            );
        }
    }

    for f in i.lifespans.iter().filter(|f| f.needs_attention()) {
        use crate::lifespan::{LifeOutcome, LifeSubject};
        // A battery already below 80 % is the battery reason above.
        if f.subject == LifeSubject::Battery && f.outcome == LifeOutcome::AlreadyPastLimit {
            continue;
        }
        let id = match f.subject {
            LifeSubject::Ssd => "ssdLife",
            LifeSubject::Battery => "batteryLife",
        };
        let severity = if f.outcome == LifeOutcome::AlreadyPastLimit {
            Severity::Warning
        } else {
            Severity::Info
        };
        add(
            &format!("{id}:{}", f.name),
            format!("{}: {}", f.name, f.headline),
            format!("{} Confidence: {}.", f.explanation, f.confidence.word()),
            if severity == Severity::Warning { 12 } else { 5 },
            severity,
        );
    }

    let pressure = i
        .sample
        .memory
        .pressure
        .value()
        .copied()
        .unwrap_or_else(|| derive_pressure(&i.sample.memory));
    if pressure != MemoryPressure::Normal {
        add(
            "memory",
            "Memory is under pressure".into(),
            format!(
                "{} of {} in use. Close apps you don't need, or add memory.",
                bytes(i.sample.memory.used),
                bytes(i.sample.memory.total)
            ),
            if pressure == MemoryPressure::Critical { 15 } else { 8 },
            Severity::Warning,
        );
    }

    if let Some(t) = i.sample.hottest().filter(|t| t.celsius >= 90.0) {
        add(
            "heat",
            format!("{} is at {:.0} °C", t.label, t.celsius),
            "Hot components slow themselves down and wear faster. Check the vents and fans.".into(),
            10,
            Severity::Warning,
        );
    }

    let restart = i.facts.restart_pending.value() == Some(&true);
    if restart {
        add(
            "restartPending",
            "A restart is needed to finish installing updates".into(),
            "Updates are only half installed until you restart.".into(),
            8,
            Severity::Warning,
        );
    }
    if let Some(n) = i.facts.pending_updates.value().filter(|n| **n > 0) {
        add(
            "updates",
            format!("{n} update{} waiting", if *n == 1 { "" } else { "s" }),
            "Security and driver fixes are waiting to be installed.".into(),
            5,
            Severity::Info,
        );
    }
    let uptime_days = i.sample.uptime_secs / 86_400;
    if uptime_days >= 7 && !restart {
        add(
            "uptime",
            format!("Running for {} without a restart", duration(i.sample.uptime_secs)),
            "A restart clears memory leaks and finishes pending maintenance.".into(),
            5,
            Severity::Info,
        );
    }

    let heavy: Vec<&StartupEntry> = i
        .startup
        .iter()
        .filter(|e| e.impact.rating == ImpactRating::High && e.state == crate::startup::StartupState::Enabled)
        .collect();
    if heavy.len() >= 3 {
        add(
            "startup",
            format!("{} apps slow down your login", heavy.len()),
            format!(
                "High startup impact: {}.",
                heavy.iter().take(4).map(|e| e.name.as_str()).collect::<Vec<_>>().join(", ")
            ),
            5,
            Severity::Info,
        );
    }

    if i.crashes_7d >= 5 {
        add(
            "crashes",
            format!("{} app crashes this week", i.crashes_7d),
            "Frequent crashes point at a broken update, a driver or failing hardware.".into(),
            10,
            Severity::Warning,
        );
    }
    for (n, a) in i.anomalies.iter().enumerate() {
        add(
            &a.id,
            a.title.clone(),
            a.explanation.clone(),
            if n < 3 { 5 } else { 0 },
            Severity::Info,
        );
    }

    r.sort_by(|a, b| b.impact.cmp(&a.impact).then(b.severity.cmp(&a.severity)));
    let lost: u32 = r.iter().map(|x| x.impact).sum();
    let performance_score = 100u32.saturating_sub(lost);

    let mut active: Vec<&Alarm> = i
        .alarms
        .iter()
        .filter(|a| a.state == AlarmState::Active && !a.is_shelved(i.now))
        .collect();
    active.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then(a.first_seen.cmp(&b.first_seen))
            .then(a.id.cmp(&b.id))
    });
    // Sorted most severe first, so the first alarm decides the cap.
    let worst = active.first().map(|a| a.severity);
    let cap = worst.and_then(cap_for);
    let score = cap.map_or(performance_score, |c| performance_score.min(c.top_score()));
    let capped_by = worst.filter(|_| score < performance_score);
    let mut reasons: Vec<HealthReason> = active
        .iter()
        .map(|a| HealthReason {
            id: format!("alarm:{}", a.id),
            title: a.title.clone(),
            detail: a.detail.clone(),
            impact: 0,
            severity: a.severity,
            since: Some(a.first_seen),
        })
        .collect();
    reasons.extend(r);
    HealthReport {
        at: i.now,
        score,
        grade: grade(score),
        performance_score,
        capped_by,
        reasons,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anomaly::AnomalyKind;
    use crate::facts::DiskHealth;
    use crate::metrics::fixtures::sample;
    use crate::metrics::{BatterySample, PowerSource, Reading, Temperature};
    use crate::startup::fixtures::entry;

    fn run(
        sample: &SystemSample,
        facts: &SystemFacts,
        startup: &[StartupEntry],
        anomalies: &[Anomaly],
        crashes: u32,
        forecasts: &[(String, Forecast)],
    ) -> HealthReport {
        assess(&HealthInput {
            now: Timestamp(1),
            sample,
            facts,
            system_disk: "C:",
            forecasts,
            startup,
            anomalies,
            crashes_7d: crashes,
            lifespans: &[],
            alarms: &[],
        })
    }

    #[test]
    fn a_healthy_machine_scores_100() {
        let r = run(&sample(0), &SystemFacts::unknown("w"), &[], &[], 0, &[]);
        assert_eq!(r.score, 100);
        assert_eq!(r.grade, Grade::Good);
        assert!(r.reasons.is_empty());
    }

    #[test]
    fn everything_wrong_scores_poor() {
        let mut s = sample(0);
        s.disks[0].free = 10 << 30; // 2%
        s.battery = Reading::Available(BatterySample {
            charge: 50.0,
            source: PowerSource::Ac,
            health: 55.0,
            cycles: Some(900),
            seconds_to_empty: None,
        });
        s.memory.available = 256 << 20;
        s.memory.pressure = Reading::unavailable("n/a");
        s.temperatures = Reading::Available(vec![Temperature {
            label: "CPU".into(),
            celsius: 96.0,
            critical: None,
        }]);
        s.uptime_secs = 10 * 86_400;
        let mut f = SystemFacts::unknown("w");
        f.disks = Reading::Available(vec![
            DiskHealth {
                name: "Samsung".into(),
                status: DiskHealthStatus::Warning,
                wear: None,
                temperature: None,
            },
            DiskHealth {
                name: "Old HDD".into(),
                status: DiskHealthStatus::Unhealthy,
                wear: None,
                temperature: None,
            },
            DiskHealth {
                name: "Fine".into(),
                status: DiskHealthStatus::Healthy,
                wear: None,
                temperature: None,
            },
        ]);
        f.restart_pending = Reading::Available(true);
        f.pending_updates = Reading::Available(1);
        let heavy = vec![
            entry("A", "C:\\a.exe", ImpactRating::High),
            entry("B", "C:\\b.exe", ImpactRating::High),
            entry("C", "C:\\c.exe", ImpactRating::High),
        ];
        let anomaly = |id: &str| Anomaly {
            id: id.into(),
            kind: AnomalyKind::MemoryGrowth,
            app: "x".into(),
            since: Timestamp(0),
            title: "t".into(),
            explanation: "e".into(),
            evidence: vec![],
        };
        let anomalies: Vec<Anomaly> = (0..4).map(|n| anomaly(&n.to_string())).collect();
        let r = run(&s, &f, &heavy, &anomalies, 7, &[("C:".into(), Forecast::FullInDays(3.0))]);
        assert_eq!(r.score, 0);
        assert_eq!(r.grade, Grade::Poor);
        assert_eq!(r.reasons[0].id, "disk:Old HDD");
        let ids: Vec<&str> = r.reasons.iter().map(|r| r.id.as_str()).collect();
        for want in [
            "lowDisk",
            "diskFilling",
            "battery",
            "memory",
            "heat",
            "restartPending",
            "updates",
            "startup",
            "crashes",
        ] {
            assert!(ids.contains(&want), "{want} in {ids:?}");
        }
        assert!(!ids.contains(&"uptime"), "restart pending already says it");
    }

    #[test]
    fn milder_cases() {
        let mut s = sample(0);
        s.disks[0].free = 40 << 30; // 8%
        s.battery = Reading::Available(BatterySample {
            charge: 50.0,
            source: PowerSource::Battery,
            health: 75.0,
            cycles: None,
            seconds_to_empty: None,
        });
        s.memory.pressure = Reading::Available(MemoryPressure::Elevated);
        s.uptime_secs = 9 * 86_400;
        let mut f = SystemFacts::unknown("w");
        f.pending_updates = Reading::Available(3);
        let r = run(&s, &f, &[], &[], 0, &[("C:".into(), Forecast::FullInDays(20.0))]);
        let get = |id: &str| r.reasons.iter().find(|x| x.id == id).unwrap();
        assert_eq!(get("lowDisk").impact, 15);
        assert_eq!(get("diskFilling").impact, 8);
        assert_eq!(get("battery").impact, 10);
        assert!(get("battery").detail.starts_with("A worn"));
        assert_eq!(get("memory").impact, 8);
        assert_eq!(get("updates").title, "3 updates waiting");
        assert!(get("uptime").title.contains("9d"));
        assert_eq!(r.score, 100 - 15 - 8 - 10 - 8 - 5 - 5);
        assert_eq!(r.grade, Grade::Poor);
        assert_eq!(grade(60), Grade::Fair);
        assert_eq!(grade(80), Grade::Good);
        assert_eq!(
            (grade(79), grade(59), grade(100), grade(0)),
            (Grade::Fair, Grade::Poor, Grade::Good, Grade::Poor)
        );
        let r = run(
            &sample(0),
            &SystemFacts::unknown("w"),
            &[],
            &[],
            0,
            &[("C:".into(), Forecast::FullInDays(90.0))],
        );
        assert_eq!(r.score, 100);
    }

    #[test]
    fn disk_and_battery_life_cost_points_only_with_evidence() {
        use crate::hardware::Confidence;
        use crate::lifespan::{unavailable, LifeOutcome, LifeSubject};
        let facts = SystemFacts::unknown("w");
        let s = sample(0);
        let mut ssd = unavailable(LifeSubject::Ssd, "Samsung 990", "x");
        ssd.outcome = LifeOutcome::Remaining {
            low_days: 100.0,
            mid_days: 200.0,
            high_days: 300.0,
        };
        ssd.headline = "About 7 months of writes left".into();
        ssd.confidence = Confidence::Low;
        let mut bat = unavailable(LifeSubject::Battery, "Battery", "x");
        bat.outcome = LifeOutcome::AlreadyPastLimit;
        let run = |l: &[crate::lifespan::LifeForecast]| {
            assess(&HealthInput {
                now: Timestamp(1),
                sample: &s,
                facts: &facts,
                system_disk: "C:",
                forecasts: &[],
                startup: &[],
                anomalies: &[],
                crashes_7d: 0,
                lifespans: l,
                alarms: &[],
            })
        };
        assert_eq!(run(&[ssd.clone()]).score, 100, "a low-confidence hint costs nothing");
        assert_eq!(run(&[bat.clone()]).score, 100, "the battery reason above covers a worn battery");
        ssd.confidence = Confidence::Medium;
        let r = run(&[ssd.clone()]);
        assert_eq!(r.score, 95);
        assert_eq!(r.reasons[0].id, "ssdLife:Samsung 990");
        assert!(r.reasons[0].detail.contains("Confidence: medium"));
        ssd.outcome = LifeOutcome::AlreadyPastLimit;
        let r = run(&[ssd]);
        assert_eq!((r.score, r.reasons[0].severity), (88, Severity::Warning));
        bat.outcome = LifeOutcome::Remaining {
            low_days: 30.0,
            mid_days: 60.0,
            high_days: 90.0,
        };
        bat.confidence = Confidence::High;
        let r = run(&[bat]);
        assert_eq!(r.reasons[0].id, "batteryLife:Battery");
    }

    fn alarm(id: u64, severity: Severity, state: AlarmState, first_seen: i64) -> Alarm {
        Alarm {
            id,
            rule: "r".into(),
            key: format!("k{id}"),
            severity,
            state,
            title: format!("Alarm {id}"),
            detail: "d".into(),
            first_seen: Timestamp(first_seen),
            last_seen: Timestamp(first_seen),
            repeat_count: 1,
            current_value: None,
            metric: None,
            notes: vec![],
            acknowledged_at: None,
            cleared_at: None,
            shelved: None,
            muted: false,
            chattering: false,
            expires_at: None,
        }
    }

    fn with_alarms(sample: &SystemSample, alarms: &[Alarm]) -> HealthReport {
        let facts = SystemFacts::unknown("w");
        assess(&HealthInput {
            now: Timestamp(1_000),
            sample,
            facts: &facts,
            system_disk: "C:",
            forecasts: &[],
            startup: &[],
            anomalies: &[],
            crashes_7d: 0,
            lifespans: &[],
            alarms,
        })
    }

    #[test]
    fn an_active_warning_caps_a_perfect_score_at_fair() {
        let r = with_alarms(&sample(0), &[alarm(1, Severity::Warning, AlarmState::Active, 5)]);
        assert_eq!((r.performance_score, r.score, r.grade), (100, 79, Grade::Fair));
        assert_eq!(r.capped_by, Some(Severity::Warning));
    }

    #[test]
    fn an_active_critical_caps_a_perfect_score_at_poor() {
        let r = with_alarms(
            &sample(0),
            &[
                alarm(1, Severity::Warning, AlarmState::Active, 5),
                alarm(2, Severity::Critical, AlarmState::Active, 6),
            ],
        );
        assert_eq!((r.score, r.grade, r.capped_by), (59, Grade::Poor, Some(Severity::Critical)));
    }

    #[test]
    fn an_already_low_score_is_unaffected() {
        let mut s = sample(0);
        s.disks[0].free = 10 << 30;
        s.memory.available = 256 << 20;
        s.memory.pressure = Reading::unavailable("n/a");
        s.temperatures = Reading::Available(vec![Temperature {
            label: "CPU".into(),
            celsius: 96.0,
            critical: None,
        }]);
        let none = with_alarms(&s, &[]);
        assert!(none.score < 59, "{}", none.score);
        let r = with_alarms(&s, &[alarm(1, Severity::Critical, AlarmState::Active, 5)]);
        assert_eq!((r.score, r.grade, r.capped_by), (none.score, none.grade, None));
        let r = with_alarms(&sample(0), &[alarm(1, Severity::Warning, AlarmState::Active, 5)]);
        assert_eq!(r.score, 79);
    }

    #[test]
    fn resolved_acknowledged_shelved_and_info_alarms_do_not_cap() {
        let mut shelved = alarm(4, Severity::Critical, AlarmState::Active, 5);
        shelved.shelved = Some(crate::alarms::Shelf::Until(Timestamp(9_999)));
        let r = with_alarms(
            &sample(0),
            &[
                alarm(1, Severity::Critical, AlarmState::Cleared, 5),
                alarm(2, Severity::Critical, AlarmState::ClearedUnacknowledged, 5),
                alarm(3, Severity::Critical, AlarmState::Acknowledged, 5),
                shelved,
                alarm(5, Severity::Info, AlarmState::Active, 5),
            ],
        );
        assert_eq!((r.score, r.grade, r.capped_by), (100, Grade::Good, None));
        assert_eq!(r.reasons.len(), 1, "only the active info alarm is named");
    }

    #[test]
    fn alarm_reasons_come_first_most_severe_then_oldest_then_performance() {
        let mut s = sample(0);
        s.disks[0].free = 10 << 30;
        let r = with_alarms(
            &s,
            &[
                alarm(1, Severity::Warning, AlarmState::Active, 10),
                alarm(2, Severity::Critical, AlarmState::Active, 30),
                alarm(3, Severity::Critical, AlarmState::Active, 20),
            ],
        );
        let ids: Vec<&str> = r.reasons.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(&ids[..4], ["alarm:3", "alarm:2", "alarm:1", "lowDisk"]);
        assert_eq!(r.reasons[0].since, Some(Timestamp(20)));
        assert_eq!(
            (r.reasons[0].title.as_str(), r.reasons[0].severity),
            ("Alarm 3", Severity::Critical)
        );
        assert_eq!(r.reasons[3].since, None);
    }

    #[test]
    fn the_grade_and_its_top_score_agree() {
        for g in [Grade::Good, Grade::Fair, Grade::Poor] {
            assert_eq!(grade(g.top_score()), g);
        }
    }
}
