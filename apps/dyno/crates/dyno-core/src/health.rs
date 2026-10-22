//! The health score: 100 minus the points each problem costs, with the reasons and a fix for each where one exists.

use crate::anomaly::Anomaly;
use crate::facts::{DiskHealthStatus, SystemFacts};
use crate::forecast::Forecast;
use crate::metrics::{derive_pressure, percent, MemoryPressure, SystemSample};
use crate::startup::{ImpactRating, StartupEntry};
use crate::time::Timestamp;
use crate::tuning::Plan;
use crate::units::{bytes, duration};
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

/// What a fix does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FixKind {
    /// Apply a tuning suggestion (by id).
    Tuning(String),
    /// Open cleanup.
    OpenCleanup,
    /// Open the startup manager.
    OpenStartup,
    /// Restart the computer (the UI asks first).
    Restart,
    /// Open the task manager.
    OpenTaskManager,
}

/// A one-click fix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fix {
    /// Button label.
    pub label: String,
    /// What it does and why.
    pub explanation: String,
    /// What will change, line by line.
    pub preview: Vec<String>,
    /// Needs the helper's admin prompt.
    pub needs_admin: bool,
    /// Can be undone from Activity.
    pub undoable: bool,
    /// What it is.
    pub kind: FixKind,
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
    /// A fix, if there is one.
    pub fix: Option<Fix>,
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
    /// Reasons, biggest impact first.
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
    /// Tuning plans that fit now (fixes link to them).
    pub tuning: &'a [Plan],
}

/// The grade for a score.
pub fn grade(score: u32) -> Grade {
    match score {
        80.. => Grade::Good,
        60..=79 => Grade::Fair,
        _ => Grade::Poor,
    }
}

fn simple_fix(label: &str, explanation: &str, kind: FixKind) -> Option<Fix> {
    Some(Fix {
        label: label.into(),
        explanation: explanation.into(),
        preview: vec![],
        needs_admin: false,
        undoable: false,
        kind,
    })
}

fn tuning_fix(plans: &[Plan], id: &str) -> Option<Fix> {
    let p = plans.iter().find(|p| p.suggestion.id == id && !p.suggestion.applied)?;
    Some(Fix {
        label: p.suggestion.title.clone(),
        explanation: p.suggestion.explanation.clone(),
        preview: p.apply.preview(),
        needs_admin: p.suggestion.needs_admin,
        undoable: true,
        kind: FixKind::Tuning(id.into()),
    })
}

/// Scores the machine.
pub fn assess(i: &HealthInput<'_>) -> HealthReport {
    let mut r: Vec<HealthReason> = Vec::new();
    let mut add = |id: &str, title: String, detail: String, impact: u32, severity: Severity, fix: Option<Fix>| {
        r.push(HealthReason {
            id: id.into(),
            title,
            detail,
            impact,
            severity,
            fix,
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
                None,
            );
        }
    }

    if let Some(d) = i.sample.disks.iter().find(|d| d.id.eq_ignore_ascii_case(i.system_disk)) {
        let free = percent(d.free, d.total);
        if free < 10.0 {
            let (impact, severity) = if free < 5.0 { (25, Severity::Critical) } else { (15, Severity::Warning) };
            add(
                "lowDisk",
                format!("{} has {} free ({free:.0}%)", d.name, bytes(d.free)),
                "Windows and apps need free space for updates, the page file and temporary files.".into(),
                impact,
                severity,
                simple_fix("Free up space", "Opens Cleanup with a preview of what can go.", FixKind::OpenCleanup),
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
                    simple_fix("See what's taking space", "Opens Cleanup and the disk map.", FixKind::OpenCleanup),
                );
            }
        }
    }

    if let Some(b) = i.sample.battery.value() {
        if b.health < 80.0 {
            let (impact, severity) = if b.health < 60.0 { (20, Severity::Warning) } else { (10, Severity::Info) };
            add(
                "battery",
                format!("The battery holds {:.0}% of its original charge", b.health),
                match b.cycles {
                    Some(c) => format!("After {c} charge cycles. A worn battery runs out sooner and may slow the CPU."),
                    None => "A worn battery runs out sooner and may slow the CPU.".into(),
                },
                impact,
                severity,
                None,
            );
        }
    }

    let pressure = i.sample.memory.pressure.value().copied().unwrap_or_else(|| derive_pressure(&i.sample.memory));
    if pressure != MemoryPressure::Normal {
        add(
            "memory",
            "Memory is under pressure".into(),
            format!("{} of {} in use. Close apps you don't need, or add memory.", bytes(i.sample.memory.used), bytes(i.sample.memory.total)),
            if pressure == MemoryPressure::Critical { 15 } else { 8 },
            Severity::Warning,
            simple_fix("See what uses memory", "Opens the task manager sorted by memory.", FixKind::OpenTaskManager),
        );
    }

    if let Some(t) = i.sample.hottest().filter(|t| t.celsius >= 90.0) {
        add(
            "heat",
            format!("{} is at {:.0} °C", t.label, t.celsius),
            "Hot components slow themselves down and wear faster. Check the vents and fans.".into(),
            10,
            Severity::Warning,
            None,
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
            simple_fix("Restart now", "Restarts the computer after you confirm. Save your work first.", FixKind::Restart),
        );
    }
    if let Some(n) = i.facts.pending_updates.value().filter(|n| **n > 0) {
        add(
            "updates",
            format!("{n} update{} waiting", if *n == 1 { "" } else { "s" }),
            "Security and driver fixes are waiting to be installed.".into(),
            5,
            Severity::Info,
            None,
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
            simple_fix("Restart now", "Restarts the computer after you confirm. Save your work first.", FixKind::Restart),
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
            simple_fix("Review startup apps", "Opens the startup manager to disable or delay them.", FixKind::OpenStartup),
        );
    }

    if i.crashes_7d >= 5 {
        add(
            "crashes",
            format!("{} app crashes this week", i.crashes_7d),
            "Frequent crashes point at a broken update, a driver or failing hardware.".into(),
            10,
            Severity::Warning,
            None,
        );
    }
    for (n, a) in i.anomalies.iter().enumerate() {
        add(&a.id, a.title.clone(), a.explanation.clone(), if n < 3 { 5 } else { 0 }, Severity::Info, None);
    }

    if let Some(fix) = tuning_fix(i.tuning, "powerPlan").or_else(|| tuning_fix(i.tuning, "lowPowerMode")) {
        add(
            "powerSaver",
            "A power-saving mode slows the CPU while plugged in".into(),
            fix.explanation.clone(),
            3,
            Severity::Info,
            Some(fix),
        );
    }
    if let Some(fix) = tuning_fix(i.tuning, "indexDevFolders") {
        add("indexing", "Search indexing slows your builds".into(), fix.explanation.clone(), 2, Severity::Info, Some(fix));
    }

    r.sort_by(|a, b| b.impact.cmp(&a.impact).then(b.severity.cmp(&a.severity)));
    let lost: u32 = r.iter().map(|x| x.impact).sum();
    let score = 100u32.saturating_sub(lost);
    HealthReport {
        at: i.now,
        score,
        grade: grade(score),
        reasons: r,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anomaly::AnomalyKind;
    use crate::cleanup::Os;
    use crate::facts::{DevFolder, DiskHealth, PowerPlan};
    use crate::helper::POWER_SAVER_SCHEME;
    use crate::metrics::fixtures::sample;
    use crate::metrics::{BatterySample, PowerSource, Reading, Temperature};
    use crate::startup::fixtures::entry;
    use crate::tuning::plans;

    fn run(sample: &SystemSample, facts: &SystemFacts, startup: &[StartupEntry], anomalies: &[Anomaly], crashes: u32, forecasts: &[(String, Forecast)]) -> HealthReport {
        let tuning = plans(facts, Os::Windows, Some(PowerSource::Ac), &[]);
        assess(&HealthInput {
            now: Timestamp(1),
            sample,
            facts,
            system_disk: "C:",
            forecasts,
            startup,
            anomalies,
            crashes_7d: crashes,
            tuning: &tuning,
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
    fn everything_wrong_scores_poor_with_fixes() {
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
        f.power_plan = Reading::Available(PowerPlan {
            id: POWER_SAVER_SCHEME.into(),
            name: "Power saver".into(),
            power_saver: true,
        });
        f.dev_folders = vec![DevFolder {
            path: "C:\\src\\x".into(),
            indexed: Some(true),
            av_excluded: Some(true),
        }];
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
        for want in ["lowDisk", "diskFilling", "battery", "memory", "heat", "restartPending", "updates", "startup", "crashes", "powerSaver", "indexing"] {
            assert!(ids.contains(&want), "{want} in {ids:?}");
        }
        assert!(!ids.contains(&"uptime"), "restart pending already says it");
        let power = r.reasons.iter().find(|r| r.id == "powerSaver").unwrap().fix.as_ref().unwrap();
        assert!(power.undoable && !power.preview.is_empty());
        assert_eq!(power.kind, FixKind::Tuning("powerPlan".into()));
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
        assert_eq!((grade(79), grade(59), grade(100), grade(0)), (Grade::Fair, Grade::Poor, Grade::Good, Grade::Poor));
        let r = run(&sample(0), &SystemFacts::unknown("w"), &[], &[], 0, &[("C:".into(), Forecast::FullInDays(90.0))]);
        assert_eq!(r.score, 100);
    }
}
