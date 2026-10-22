//! The weekly health report: what the week looked like, in numbers and a few sentences.

use crate::history::{Consumer, HistoryPoint};
use crate::time::Timestamp;
use crate::units::bytes;
use serde::{Deserialize, Serialize};

/// The report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyReport {
    /// Start.
    pub from: Timestamp,
    /// End.
    pub to: Timestamp,
    /// Health score now.
    pub score: u32,
    /// Change against the score a week ago (0 when unknown).
    pub score_change: i32,
    /// Average CPU.
    pub avg_cpu: f32,
    /// Highest CPU.
    pub peak_cpu: f32,
    /// Average memory in use, percent.
    pub avg_memory: f32,
    /// Biggest consumers.
    pub top_apps: Vec<Consumer>,
    /// Rule actions taken.
    pub rule_actions: u32,
    /// Bytes cleaned up.
    pub cleaned_bytes: u64,
    /// App crashes.
    pub crashes: u32,
    /// Alerts raised.
    pub alerts: u32,
    /// A few sentences.
    pub highlights: Vec<String>,
}

/// What the report is built from.
pub struct ReportInput<'a> {
    /// Start.
    pub from: Timestamp,
    /// End.
    pub to: Timestamp,
    /// The week's points (any resolution).
    pub points: &'a [HistoryPoint],
    /// Consumers over the week.
    pub top_apps: Vec<Consumer>,
    /// Score now.
    pub score: u32,
    /// Score a week ago, if recorded.
    pub score_before: Option<u32>,
    /// Rule actions.
    pub rule_actions: u32,
    /// Bytes cleaned.
    pub cleaned_bytes: u64,
    /// Crashes.
    pub crashes: u32,
    /// Alerts.
    pub alerts: u32,
}

/// Builds the report.
pub fn weekly(i: ReportInput<'_>) -> WeeklyReport {
    let n = i.points.len().max(1) as f32;
    let avg_cpu = i.points.iter().map(|p| p.cpu).sum::<f32>() / n;
    let peak_cpu = i.points.iter().map(|p| p.cpu_max).fold(0.0, f32::max);
    let avg_memory = i.points.iter().map(|p| p.memory).sum::<f32>() / n;
    let score_change = i.score_before.map_or(0, |b| i.score as i32 - b as i32);
    let mut highlights = Vec::new();
    if i.points.is_empty() {
        highlights.push("Dyno has no history for this week yet.".to_string());
    } else {
        highlights.push(format!(
            "Your PC averaged {avg_cpu:.0}% CPU and {avg_memory:.0}% memory, peaking at {peak_cpu:.0}% CPU."
        ));
    }
    if let Some(top) = i.top_apps.first() {
        highlights.push(format!("{} was the busiest app ({:.0}% CPU on average).", top.app, top.cpu_avg));
    }
    match score_change {
        c if c > 0 => highlights.push(format!("The health score rose by {c} points to {}.", i.score)),
        c if c < 0 => highlights.push(format!("The health score fell by {} points to {}.", -c, i.score)),
        _ => {}
    }
    if i.rule_actions > 0 {
        highlights.push(format!(
            "Rules stepped in {} time{}.",
            i.rule_actions,
            if i.rule_actions == 1 { "" } else { "s" }
        ));
    }
    if i.cleaned_bytes > 0 {
        highlights.push(format!("Cleanup freed {}.", bytes(i.cleaned_bytes)));
    }
    if i.crashes > 0 {
        let one = i.crashes == 1;
        highlights.push(format!(
            "{} app crash{} {} recorded.",
            i.crashes,
            if one { "" } else { "es" },
            if one { "was" } else { "were" }
        ));
    }
    WeeklyReport {
        from: i.from,
        to: i.to,
        score: i.score,
        score_change,
        avg_cpu,
        peak_cpu,
        avg_memory,
        top_apps: i.top_apps,
        rule_actions: i.rule_actions,
        cleaned_bytes: i.cleaned_bytes,
        crashes: i.crashes,
        alerts: i.alerts,
        highlights,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(points: &[HistoryPoint]) -> ReportInput<'_> {
        ReportInput {
            from: Timestamp(0),
            to: Timestamp(1),
            points,
            top_apps: vec![],
            score: 80,
            score_before: None,
            rule_actions: 0,
            cleaned_bytes: 0,
            crashes: 0,
            alerts: 0,
        }
    }

    #[test]
    fn an_empty_week() {
        let r = weekly(input(&[]));
        assert_eq!(r.highlights, vec!["Dyno has no history for this week yet.".to_string()]);
        assert_eq!(r.score_change, 0);
    }

    #[test]
    fn a_full_week() {
        let pts = [
            HistoryPoint {
                cpu: 10.0,
                cpu_max: 50.0,
                memory: 40.0,
                ..Default::default()
            },
            HistoryPoint {
                cpu: 30.0,
                cpu_max: 90.0,
                memory: 60.0,
                ..Default::default()
            },
        ];
        let mut i = input(&pts);
        i.top_apps = vec![Consumer {
            app: "Chrome".into(),
            cpu_avg: 12.0,
            memory_avg: 0,
        }];
        i.score_before = Some(70);
        i.rule_actions = 1;
        i.cleaned_bytes = 3 << 30;
        i.crashes = 2;
        let r = weekly(i);
        assert_eq!((r.avg_cpu, r.peak_cpu, r.avg_memory, r.score_change), (20.0, 90.0, 50.0, 10));
        assert_eq!(r.highlights.len(), 6);
        assert!(r.highlights[2].contains("rose by 10"));
        assert!(r.highlights[3].contains("1 time."));
        assert!(r.highlights[5].contains("2 app crashes"));
        let mut i = input(&pts);
        i.score_before = Some(90);
        i.rule_actions = 3;
        i.crashes = 1;
        let r = weekly(i);
        assert!(r.highlights.iter().any(|h| h.contains("fell by 10")));
        assert!(r.highlights.iter().any(|h| h.contains("3 times")));
        assert!(r.highlights.iter().any(|h| h.contains("1 app crash was")));
    }
}
