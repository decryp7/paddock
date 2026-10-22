//! Which events the user is told about (native notification and the tray badge), and the words.
//!
//! The service decides on transitions only — a runner that stays stalled is announced once — and pushes the
//! notifications to the app, which shows them.

use crate::health::RunnerStatus;
use crate::housekeeping::CleanupReport;
use crate::names::RunnerName;
use crate::time::Timestamp;
use crate::update::{UpdateOutcome, UpdatePlan};
use crate::version::RunnerVersion;
use serde::{Deserialize, Serialize};

/// What a notification is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotificationKind {
    /// The runner stalled and is being restarted.
    Stalled,
    /// The runner keeps crashing; restarts are paused.
    CrashLoop,
    /// The runner cannot reach its server.
    ServerUnreachable,
    /// The server does not know the runner.
    NotRegistered,
    /// An update was installed.
    Updated,
    /// An update was rolled back.
    RolledBack,
    /// An update failed.
    UpdateFailed,
    /// A new version is available (notify-only policy).
    UpdateAvailable,
    /// The disk is low on space.
    DiskLow,
    /// Housekeeping freed space.
    CleanedUp,
    /// Housekeeping could not delete some files.
    CleanupProblems,
}

/// How serious.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    /// Good to know.
    Info,
    /// Needs a look.
    Warning,
    /// Needs the user.
    Error,
}

/// One notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    /// What about.
    pub kind: NotificationKind,
    /// How serious.
    pub severity: Severity,
    /// The runner.
    pub runner: RunnerName,
    /// The title.
    pub title: String,
    /// The text.
    pub body: String,
    /// When.
    pub at: Timestamp,
}

fn n(kind: NotificationKind, severity: Severity, runner: &RunnerName, title: String, body: String, at: Timestamp) -> Notification {
    Notification {
        kind,
        severity,
        runner: runner.clone(),
        title,
        body,
        at,
    }
}

/// The notification for a status change, if the new status deserves one and is new.
pub fn status_change(runner: &RunnerName, previous: Option<RunnerStatus>, next: RunnerStatus, at: Timestamp) -> Option<Notification> {
    if previous == Some(next) {
        return None;
    }
    let (kind, severity, title, body) = match next {
        RunnerStatus::Stalled => (
            NotificationKind::Stalled,
            Severity::Warning,
            format!("{runner} stalled"),
            "Its polls for jobs kept failing while it ran no job. Pitcrew is restarting it.".to_string(),
        ),
        RunnerStatus::CrashLoop => (
            NotificationKind::CrashLoop,
            Severity::Error,
            format!("{runner} keeps crashing"),
            "It exited too often in a short time, so Pitcrew paused restarts. Check its log, then start it again.".to_string(),
        ),
        RunnerStatus::Degraded => (
            NotificationKind::ServerUnreachable,
            Severity::Warning,
            format!("{runner} cannot reach its server"),
            "Its polls for jobs fail. Check the network and the server; it keeps trying.".to_string(),
        ),
        RunnerStatus::NotRegistered => (
            NotificationKind::NotRegistered,
            Severity::Error,
            format!("{runner} is not registered"),
            "The server does not know this runner. Register it again from Pitcrew.".to_string(),
        ),
        _ => return None,
    };
    Some(n(kind, severity, runner, title, body, at))
}

fn versions(plan: &UpdatePlan) -> String {
    match &plan.from {
        Some(from) => format!("{from} → {}", plan.to),
        None => plan.to.to_string(),
    }
}

/// The notification for a finished update (postponed and cancelled updates are not announced).
pub fn update_finished(runner: &RunnerName, outcome: &UpdateOutcome, at: Timestamp) -> Option<Notification> {
    let (kind, severity, title, body) = match outcome {
        UpdateOutcome::Updated { plan } => (
            NotificationKind::Updated,
            Severity::Info,
            format!("{runner} updated"),
            format!("Now running {} ({}).", plan.to, versions(plan)),
        ),
        UpdateOutcome::RolledBack { plan, .. } => (
            NotificationKind::RolledBack,
            Severity::Warning,
            format!("{runner}: update to {} rolled back", plan.to),
            "The new version did not come up, so the previous one runs again.".to_string(),
        ),
        UpdateOutcome::Failed { plan, reason, .. } => (
            NotificationKind::UpdateFailed,
            Severity::Error,
            format!("{runner}: update to {} failed", plan.to),
            format!("{reason}."),
        ),
        UpdateOutcome::Postponed { .. } | UpdateOutcome::Cancelled { .. } => return None,
    };
    Some(n(kind, severity, runner, title, body, at))
}

/// A new version is available.
pub fn update_available(runner: &RunnerName, version: &RunnerVersion, at: Timestamp) -> Notification {
    n(
        NotificationKind::UpdateAvailable,
        Severity::Info,
        runner,
        format!("{runner}: version {version} is available"),
        "Install it from Pitcrew's Updates page; it is installed between jobs.".to_string(),
        at,
    )
}

/// The disk is low (announce on the change from enough to low).
pub fn disk_low(runner: &RunnerName, free_bytes: u64, at: Timestamp) -> Notification {
    n(
        NotificationKind::DiskLow,
        Severity::Warning,
        runner,
        "Disk space is low".to_string(),
        format!(
            "{} free on {runner}'s disk. Pitcrew cleans up between jobs; check the Disk page.",
            human_bytes(free_bytes)
        ),
        at,
    )
}

/// The notification for a cleanup that freed something or had problems.
pub fn cleanup(runner: &RunnerName, report: &CleanupReport) -> Option<Notification> {
    if !report.failures.is_empty() {
        return Some(n(
            NotificationKind::CleanupProblems,
            Severity::Warning,
            runner,
            format!("{runner}: some files could not be cleaned up"),
            format!(
                "Freed {}; {} item(s) could not be deleted and are tried again next time.",
                human_bytes(report.freed_bytes),
                report.failures.len()
            ),
            report.at,
        ));
    }
    (report.freed_bytes > 0).then(|| {
        n(
            NotificationKind::CleanedUp,
            Severity::Info,
            runner,
            format!("{runner}: cleaned up {}", human_bytes(report.freed_bytes)),
            format!("{} item(s) deleted between jobs.", report.deleted),
            report.at,
        )
    })
}

/// `1.5 GB`, `320 MB`, `12 KB`, `0 B`.
pub fn human_bytes(b: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 || v >= 100.0 {
        format!("{v:.0} {}", UNITS[i])
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::housekeeping::{CleanupFailure, CleanupTrigger};
    use crate::update::machine::{PostponeReason, RollbackReason};

    fn r() -> RunnerName {
        RunnerName::parse("build-pc").unwrap()
    }

    fn plan(from: Option<&str>) -> UpdatePlan {
        UpdatePlan {
            from: from.map(|f| RunnerVersion::parse(f).unwrap()),
            to: RunnerVersion::parse("5.2.0").unwrap(),
        }
    }

    #[test]
    fn only_new_bad_states_are_announced() {
        let now = Timestamp(1);
        for (s, k) in [
            (RunnerStatus::Stalled, NotificationKind::Stalled),
            (RunnerStatus::CrashLoop, NotificationKind::CrashLoop),
            (RunnerStatus::Degraded, NotificationKind::ServerUnreachable),
            (RunnerStatus::NotRegistered, NotificationKind::NotRegistered),
        ] {
            let got = status_change(&r(), Some(RunnerStatus::Idle), s, now).unwrap();
            assert_eq!(got.kind, k);
            assert!(got.title.contains("build-pc"));
            assert_eq!(status_change(&r(), Some(s), s, now), None);
        }
        assert_eq!(status_change(&r(), None, RunnerStatus::Idle, now), None);
        assert!(status_change(&r(), None, RunnerStatus::CrashLoop, now).is_some());
    }

    #[test]
    fn update_results() {
        let at = Timestamp(2);
        let u = update_finished(&r(), &UpdateOutcome::Updated { plan: plan(Some("5.1.0")) }, at).unwrap();
        assert_eq!((u.kind, u.severity), (NotificationKind::Updated, Severity::Info));
        assert!(u.body.contains("5.1.0 → 5.2.0"));
        let first = update_finished(&r(), &UpdateOutcome::Updated { plan: plan(None) }, at).unwrap();
        assert!(first.body.contains("(5.2.0)"));
        let rb = UpdateOutcome::RolledBack {
            plan: plan(Some("5.1.0")),
            reason: RollbackReason::ExitedEarly,
        };
        assert_eq!(update_finished(&r(), &rb, at).unwrap().kind, NotificationKind::RolledBack);
        let failed = UpdateOutcome::Failed {
            plan: plan(Some("5.1.0")),
            reason: "the checksum sources disagree".into(),
            needs_attention: false,
        };
        assert_eq!(update_finished(&r(), &failed, at).unwrap().severity, Severity::Error);
        let postponed = UpdateOutcome::Postponed {
            plan: plan(None),
            reason: PostponeReason::Busy,
        };
        assert_eq!(update_finished(&r(), &postponed, at), None);
        assert_eq!(update_finished(&r(), &UpdateOutcome::Cancelled { plan: plan(None) }, at), None);
        let v = RunnerVersion::parse("5.3.0").unwrap();
        assert!(update_available(&r(), &v, at).title.contains("5.3.0"));
    }

    #[test]
    fn disk_and_cleanup() {
        let at = Timestamp(3);
        assert!(disk_low(&r(), 3 * 1024 * 1024 * 1024, at).body.starts_with("3.0 GB free"));
        let mut report = CleanupReport {
            trigger: CleanupTrigger::Schedule,
            at,
            freed_bytes: 0,
            deleted: 0,
            failures: vec![],
        };
        assert_eq!(cleanup(&r(), &report), None);
        report.freed_bytes = 2048;
        report.deleted = 2;
        assert_eq!(cleanup(&r(), &report).unwrap().kind, NotificationKind::CleanedUp);
        report.failures.push(CleanupFailure {
            path: "work/x".into(),
            reason: "in use".into(),
        });
        assert_eq!(cleanup(&r(), &report).unwrap().kind, NotificationKind::CleanupProblems);
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(1536), "1.5 KB");
        assert_eq!(human_bytes(300 * 1024 * 1024), "300 MB");
        assert_eq!(human_bytes(u64::MAX), "16777216 TB");
    }
}
