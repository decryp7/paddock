//! The weekly health report: what the week looked like, in numbers and a few sentences.

use crate::forecast::Forecast;
use crate::health::Severity;
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
    /// Health score at the end of the week (none when none was recorded).
    pub score: Option<u32>,
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
    /// The health score at the hours it was recorded this week, oldest first.
    pub score_trend: Vec<ScorePoint>,
    /// Free space and the forecast per disk.
    pub disks: Vec<ReportDisk>,
    /// The week's alerts, newest first (at most [`MAX_LISTED`]).
    pub issues: Vec<ReportIssue>,
    /// What changed on the machine (rule actions, cleanups, startup and tuning changes), newest first (at most [`MAX_LISTED`]).
    pub changes: Vec<ReportChange>,
    /// Hardware notes: time throttled and disks or batteries that need attention, as plain sentences (empty when there
    /// is nothing to say).
    #[serde(default)]
    pub hardware: Vec<String>,
    /// True when the week holds no history, issues, changes or crashes: a valid report that says so.
    pub nothing_to_report: bool,
}

/// Most issues and changes the report lists.
pub const MAX_LISTED: usize = 20;

/// The health score at one moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScorePoint {
    /// When it was recorded.
    pub at: Timestamp,
    /// 0-100.
    pub score: u32,
}

/// One disk's free space at the end of the week and where it is heading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportDisk {
    /// Disk name.
    pub disk: String,
    /// Capacity, bytes.
    pub total: u64,
    /// Free bytes at the last reading.
    pub free: u64,
    /// When it will be full at the current trend.
    pub forecast: Forecast,
}

/// Something that needed attention during the week.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportIssue {
    /// When.
    pub at: Timestamp,
    /// How bad.
    pub severity: Severity,
    /// One line.
    pub title: String,
    /// Detail.
    pub detail: String,
}

/// Something Dyno or the user changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportChange {
    /// When.
    pub at: Timestamp,
    /// One line.
    pub title: String,
    /// Why or what exactly.
    pub detail: String,
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
    /// Score at the end of the week, if recorded.
    pub score: Option<u32>,
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
    /// Recorded scores this week.
    pub score_trend: Vec<ScorePoint>,
    /// Disks with their forecasts.
    pub disks: Vec<ReportDisk>,
    /// Alerts of the week, newest first.
    pub issues: Vec<ReportIssue>,
    /// Changes of the week, newest first.
    pub changes: Vec<ReportChange>,
    /// Hardware notes (see [`hardware_lines`]).
    pub hardware: Vec<String>,
}

/// Builds the report.
pub fn weekly(i: ReportInput<'_>) -> WeeklyReport {
    let n = i.points.len().max(1) as f32;
    let avg_cpu = i.points.iter().map(|p| p.cpu).sum::<f32>() / n;
    let peak_cpu = i.points.iter().map(|p| p.cpu_max).fold(0.0, f32::max);
    let avg_memory = i.points.iter().map(|p| p.memory).sum::<f32>() / n;
    let score_change = match (i.score, i.score_before) {
        (Some(now), Some(before)) => now as i32 - before as i32,
        _ => 0,
    };
    let score_now = i.score.unwrap_or(0);
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
        c if c > 0 => highlights.push(format!("The health score rose by {c} points to {score_now}.")),
        c if c < 0 => highlights.push(format!("The health score fell by {} points to {score_now}.", -c)),
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
    for d in &i.disks {
        if let Forecast::FullInDays(days) = d.forecast {
            if days < 60.0 {
                highlights.push(format!("{} will be full in about {days:.0} days at the current rate.", d.disk));
            }
        }
    }
    if !i.issues.is_empty() {
        let n = i.issues.len();
        highlights.push(format!("{n} thing{} needed attention.", if n == 1 { "" } else { "s" }));
    }
    for h in i.hardware.iter().take(3) {
        highlights.push(h.clone());
    }
    let nothing_to_report = i.points.is_empty()
        && i.issues.is_empty()
        && i.changes.is_empty()
        && i.crashes == 0
        && i.rule_actions == 0
        && i.cleaned_bytes == 0
        && i.alerts == 0
        && i.top_apps.is_empty();
    let (mut issues, mut changes) = (i.issues, i.changes);
    issues.truncate(MAX_LISTED);
    changes.truncate(MAX_LISTED);
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
        score_trend: i.score_trend,
        disks: i.disks,
        issues,
        changes,
        hardware: i.hardware,
        nothing_to_report,
    }
}

/// The week's hardware sentences: minutes spent throttled (by cause) and every disk or battery that needs attention or
/// whose forecast is worth knowing. Nothing alarming without evidence: a forecast with low confidence is mentioned as
/// a hint, never as a warning.
pub fn hardware_lines(
    episodes: &[crate::throttle::ThrottleEpisode],
    from: Timestamp,
    to: Timestamp,
    forecasts: &[crate::lifespan::LifeForecast],
) -> Vec<String> {
    use crate::lifespan::LifeOutcome;
    use crate::time::MINUTE;
    let mut lines = Vec::new();
    let mut by_cause: Vec<(crate::throttle::ThrottleCause, i64)> = Vec::new();
    for e in episodes.iter().filter(|e| e.end.unwrap_or(to) > from && e.start < to) {
        let ms = e.end.unwrap_or(to).min(to).since(e.start.max(from)).max(0);
        match by_cause.iter_mut().find(|(c, _)| *c == e.cause) {
            Some(x) => x.1 += ms,
            None => by_cause.push((e.cause, ms)),
        }
    }
    let total: i64 = by_cause.iter().map(|x| x.1).sum();
    if total >= MINUTE {
        let parts: Vec<String> = by_cause
            .iter()
            .map(|(c, ms)| format!("{} min of {}", ms / MINUTE, c.label().to_lowercase()))
            .collect();
        lines.push(format!(
            "The CPU ran slower than it should for {} min this week ({}).",
            total / MINUTE,
            parts.join(", ")
        ));
    }
    for f in forecasts {
        let name = &f.name;
        match &f.outcome {
            LifeOutcome::AlreadyPastLimit => lines.push(format!("{name}: {}", f.headline)),
            LifeOutcome::Remaining { .. } => lines.push(format!(
                "{name}: {} (confidence {}).",
                f.headline.to_lowercase(),
                f.confidence.word()
            )),
            _ => {}
        }
    }
    lines
}

/// A date as "2026-10-16" in UTC (the engine does not know the user's time zone; the app shows local times).
pub fn utc_date(t: Timestamp) -> String {
    let (y, m, d, _, _) = civil(t);
    format!("{y:04}-{m:02}-{d:02}")
}

/// A moment as "2026-10-16 14:05 UTC".
pub fn utc_time(t: Timestamp) -> String {
    let (y, m, d, hh, mm) = civil(t);
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02} UTC")
}

/// Civil date and time of a timestamp (Howard Hinnant's days-to-date algorithm).
fn civil(t: Timestamp) -> (i64, u32, u32, u32, u32) {
    let secs = t.millis().div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d, (rem / 3_600) as u32, (rem % 3_600 / 60) as u32)
}

fn forecast_text(f: Forecast) -> String {
    match f {
        Forecast::FullInDays(d) => format!("full in about {d:.0} days"),
        Forecast::NotFilling => "not filling up".to_string(),
        Forecast::NotEnoughData => "not enough history for a forecast".to_string(),
    }
}

fn severity_text(s: Severity) -> &'static str {
    match s {
        Severity::Info => "Info",
        Severity::Warning => "Warning",
        Severity::Critical => "Critical",
    }
}

fn score_line(r: &WeeklyReport) -> String {
    let change = match r.score_change {
        0 => String::new(),
        c => format!(" ({c:+} against a week ago)"),
    };
    match r.score {
        Some(s) => format!("Health score {s}{change}."),
        None => "No health score was recorded this week.".to_string(),
    }
}

/// The report as Markdown (dates in UTC).
pub fn to_markdown(r: &WeeklyReport) -> String {
    let mut o = format!("# Dyno weekly report\n\n{} to {} (UTC)\n\n", utc_date(r.from), utc_date(r.to));
    if r.nothing_to_report {
        o.push_str("Nothing to report for this week: Dyno recorded no history, issues or changes.\n");
        return o;
    }
    o.push_str("## Highlights\n\n");
    for h in &r.highlights {
        o.push_str(&format!("- {h}\n"));
    }
    o.push_str(&format!("\n## Health\n\n{}\n", score_line(r)));
    if !r.score_trend.is_empty() {
        let line: Vec<String> = r.score_trend.iter().map(|p| p.score.to_string()).collect();
        o.push_str(&format!("\nRecorded scores, oldest first: {}.\n", line.join(", ")));
    }
    o.push_str(&format!(
        "\n## Resource use\n\n- CPU: {:.0}% on average, peak {:.0}%\n- Memory: {:.0}% in use on average\n- App crashes: {}\n- Alerts: {}\n",
        r.avg_cpu, r.peak_cpu, r.avg_memory, r.crashes, r.alerts
    ));
    if !r.top_apps.is_empty() {
        o.push_str("\n## Busiest apps\n\n| App | CPU (average) | Memory (average) |\n| --- | ---: | ---: |\n");
        for a in &r.top_apps {
            o.push_str(&format!("| {} | {:.1}% | {} |\n", md_cell(&a.app), a.cpu_avg, bytes(a.memory_avg)));
        }
    }
    if !r.disks.is_empty() {
        o.push_str("\n## Disks\n\n");
        for d in &r.disks {
            o.push_str(&format!(
                "- {}: {} free of {}, {}\n",
                d.disk,
                bytes(d.free),
                bytes(d.total),
                forecast_text(d.forecast)
            ));
        }
    }
    if !r.hardware.is_empty() {
        o.push_str("\n## Hardware\n\n");
        for h in &r.hardware {
            o.push_str(&format!("- {h}\n"));
        }
    }
    if !r.issues.is_empty() {
        o.push_str("\n## Issues\n\n");
        for i in &r.issues {
            o.push_str(&format!(
                "- {} {}: {} ({})\n",
                utc_time(i.at),
                severity_text(i.severity),
                i.title,
                i.detail
            ));
        }
    }
    o.push_str("\n## What changed\n\n");
    o.push_str(&format!(
        "Rules stepped in {} time(s); cleanup freed {}.\n",
        r.rule_actions,
        bytes(r.cleaned_bytes)
    ));
    for c in &r.changes {
        o.push_str(&format!("- {} {} ({})\n", utc_time(c.at), c.title, c.detail));
    }
    o
}

fn md_cell(s: &str) -> String {
    s.replace('|', "\\|").replace(['\n', '\r'], " ")
}

/// Escapes text for HTML (the report holds app names and alert text from the machine).
pub fn escape_html(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

/// The report as one self-contained HTML page (no scripts, no remote content; dates in UTC).
pub fn to_html(r: &WeeklyReport) -> String {
    let e = escape_html;
    let mut o = String::from(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>Dyno weekly report</title>\n<style>\n\
         body{font:14px/1.5 system-ui,sans-serif;max-width:46rem;margin:2rem auto;padding:0 1rem;color:#1c1c1e}\n\
         h1{font-size:1.4rem}h2{font-size:1.05rem;margin-top:1.6rem}table{border-collapse:collapse;width:100%}\n\
         th,td{text-align:left;padding:.3rem .5rem;border-bottom:1px solid #ddd}td.n,th.n{text-align:right}\n\
         .muted{color:#666}\n</style>\n</head>\n<body>\n",
    );
    o.push_str(&format!(
        "<h1>Dyno weekly report</h1>\n<p class=\"muted\">{} to {} (UTC)</p>\n",
        utc_date(r.from),
        utc_date(r.to)
    ));
    if r.nothing_to_report {
        o.push_str("<p>Nothing to report for this week: Dyno recorded no history, issues or changes.</p>\n</body>\n</html>\n");
        return o;
    }
    o.push_str("<h2>Highlights</h2>\n<ul>\n");
    for h in &r.highlights {
        o.push_str(&format!("<li>{}</li>\n", e(h)));
    }
    o.push_str(&format!("</ul>\n<h2>Health</h2>\n<p>{}</p>\n", e(&score_line(r))));
    if !r.score_trend.is_empty() {
        let line: Vec<String> = r.score_trend.iter().map(|p| p.score.to_string()).collect();
        o.push_str(&format!(
            "<p class=\"muted\">Recorded scores, oldest first: {}.</p>\n",
            line.join(", ")
        ));
    }
    o.push_str(&format!(
        "<h2>Resource use</h2>\n<ul>\n<li>CPU: {:.0}% on average, peak {:.0}%</li>\n<li>Memory: {:.0}% in use on average</li>\n<li>App crashes: {}</li>\n<li>Alerts: {}</li>\n</ul>\n",
        r.avg_cpu, r.peak_cpu, r.avg_memory, r.crashes, r.alerts
    ));
    if !r.top_apps.is_empty() {
        o.push_str("<h2>Busiest apps</h2>\n<table>\n<tr><th>App</th><th class=\"n\">CPU (average)</th><th class=\"n\">Memory (average)</th></tr>\n");
        for a in &r.top_apps {
            o.push_str(&format!(
                "<tr><td>{}</td><td class=\"n\">{:.1}%</td><td class=\"n\">{}</td></tr>\n",
                e(&a.app),
                a.cpu_avg,
                bytes(a.memory_avg)
            ));
        }
        o.push_str("</table>\n");
    }
    if !r.disks.is_empty() {
        o.push_str("<h2>Disks</h2>\n<ul>\n");
        for d in &r.disks {
            o.push_str(&format!(
                "<li>{}: {} free of {}, {}</li>\n",
                e(&d.disk),
                bytes(d.free),
                bytes(d.total),
                forecast_text(d.forecast)
            ));
        }
        o.push_str("</ul>\n");
    }
    if !r.hardware.is_empty() {
        o.push_str("<h2>Hardware</h2>\n<ul>\n");
        for h in &r.hardware {
            o.push_str(&format!("<li>{}</li>\n", e(h)));
        }
        o.push_str("</ul>\n");
    }
    if !r.issues.is_empty() {
        o.push_str("<h2>Issues</h2>\n<ul>\n");
        for i in &r.issues {
            o.push_str(&format!(
                "<li>{} {}: {} <span class=\"muted\">({})</span></li>\n",
                utc_time(i.at),
                severity_text(i.severity),
                e(&i.title),
                e(&i.detail)
            ));
        }
        o.push_str("</ul>\n");
    }
    o.push_str(&format!(
        "<h2>What changed</h2>\n<p>Rules stepped in {} time(s); cleanup freed {}.</p>\n<ul>\n",
        r.rule_actions,
        bytes(r.cleaned_bytes)
    ));
    for c in &r.changes {
        o.push_str(&format!(
            "<li>{} {} <span class=\"muted\">({})</span></li>\n",
            utc_time(c.at),
            e(&c.title),
            e(&c.detail)
        ));
    }
    o.push_str("</ul>\n</body>\n</html>\n");
    o
}

/// The file formats a report can be exported as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReportFormat {
    /// Markdown (`.md`).
    Markdown,
    /// One HTML page (`.html`).
    Html,
}

impl ReportFormat {
    /// File extension, lower case.
    pub fn extension(self) -> &'static str {
        match self {
            ReportFormat::Markdown => "md",
            ReportFormat::Html => "html",
        }
    }

    /// Renders the report in this format.
    pub fn render(self, r: &WeeklyReport) -> String {
        match self {
            ReportFormat::Markdown => to_markdown(r),
            ReportFormat::Html => to_html(r),
        }
    }
}

/// The answer to a save-report request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedReport {
    /// Where the file was written.
    pub path: String,
    /// Its size, bytes.
    pub bytes: u64,
}

/// Checks the destination the user picked, without touching the disk: absolute, no `..`, a name with the format's extension.
pub fn validate_destination(path: &str, format: ReportFormat) -> Result<std::path::PathBuf, String> {
    if path.trim().is_empty() || path.len() > 1024 || path.contains('\0') {
        return Err("Choose where to save the report.".into());
    }
    let p = std::path::Path::new(path);
    if !p.is_absolute() {
        return Err("Give the full path of the file to save.".into());
    }
    if crate::cleanup::has_dot_segment(p) {
        return Err("The save path may not contain \"..\".".into());
    }
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
    let ext = format.extension();
    if !name.ends_with(&format!(".{ext}")) || name.len() <= ext.len() + 1 {
        return Err(format!("Save the report as a file ending in .{ext}."));
    }
    Ok(p.to_path_buf())
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
            score: Some(80),
            score_before: None,
            rule_actions: 0,
            cleaned_bytes: 0,
            crashes: 0,
            alerts: 0,
            score_trend: vec![],
            disks: vec![],
            issues: vec![],
            changes: vec![],
            hardware: vec![],
        }
    }

    fn issue(n: i64) -> ReportIssue {
        ReportIssue {
            at: Timestamp(n * 3_600_000),
            severity: Severity::Warning,
            title: format!("Issue {n}"),
            detail: "detail".into(),
        }
    }

    fn busy_week() -> WeeklyReport {
        let pts = [HistoryPoint {
            cpu: 20.0,
            cpu_max: 80.0,
            memory: 50.0,
            ..Default::default()
        }];
        let mut i = input(&pts);
        i.from = Timestamp(1_760_000_000_000);
        i.to = Timestamp(1_760_604_800_000);
        i.top_apps = vec![Consumer {
            app: "<b>Evil|App</b>".into(),
            cpu_avg: 12.5,
            memory_avg: 2 << 30,
        }];
        i.score_before = Some(70);
        i.score_trend = vec![
            ScorePoint {
                at: Timestamp(1),
                score: 70,
            },
            ScorePoint {
                at: Timestamp(2),
                score: 80,
            },
        ];
        i.disks = vec![
            ReportDisk {
                disk: "C:".into(),
                total: 500 << 30,
                free: 20 << 30,
                forecast: Forecast::FullInDays(12.0),
            },
            ReportDisk {
                disk: "D:".into(),
                total: 1 << 40,
                free: 1 << 39,
                forecast: Forecast::NotFilling,
            },
            ReportDisk {
                disk: "E:".into(),
                total: 1 << 40,
                free: 1 << 39,
                forecast: Forecast::NotEnoughData,
            },
        ];
        i.issues = vec![issue(5)];
        i.changes = vec![ReportChange {
            at: Timestamp(3_600_000),
            title: "Turned off <Updater> at login".into(),
            detail: "You changed this".into(),
        }];
        i.rule_actions = 2;
        i.cleaned_bytes = 1 << 30;
        weekly(i)
    }

    #[test]
    fn nothing_to_report_is_a_valid_report() {
        let r = weekly(input(&[]));
        assert!(r.nothing_to_report);
        let md = to_markdown(&r);
        assert!(md.contains("Nothing to report"));
        assert!(!md.contains("## Highlights"));
        assert!(to_html(&r).contains("Nothing to report"));
        // Any one thing makes it a report.
        let mut i = input(&[]);
        i.crashes = 1;
        assert!(!weekly(i).nothing_to_report);
    }

    #[test]
    fn issues_and_changes_are_capped_and_disks_forecast() {
        let mut i = input(&[]);
        i.issues = (0..30).map(issue).collect();
        i.changes = (0..30)
            .map(|n| ReportChange {
                at: Timestamp(n),
                title: "x".into(),
                detail: String::new(),
            })
            .collect();
        let r = weekly(i);
        assert_eq!((r.issues.len(), r.changes.len()), (MAX_LISTED, MAX_LISTED));
        assert!(r.highlights.iter().any(|h| h == "30 things needed attention."));
        let r = busy_week();
        assert!(r
            .highlights
            .iter()
            .any(|h| h == "C: will be full in about 12 days at the current rate."));
        assert!(r.highlights.iter().any(|h| h == "1 thing needed attention."));
        assert!(!r.nothing_to_report);
    }

    #[test]
    fn markdown_has_every_section_and_escapes_table_cells() {
        let md = to_markdown(&busy_week());
        for h in [
            "## Highlights",
            "## Health",
            "## Resource use",
            "## Busiest apps",
            "## Disks",
            "## Issues",
            "## What changed",
        ] {
            assert!(md.contains(h), "{h}");
        }
        assert!(md.starts_with("# Dyno weekly report\n\n2025-10-09 to 2025-10-16 (UTC)"));
        assert!(md.contains("Health score 80 (+10 against a week ago)."));
        assert!(md.contains("Recorded scores, oldest first: 70, 80."));
        assert!(md.contains("| <b>Evil\\|App</b> | 12.5% | 2.0 GB |"));
        assert!(md.contains("- C:: 20.0 GB free of 500.0 GB, full in about 12 days"));
        assert!(md.contains("not filling up") && md.contains("not enough history for a forecast"));
        assert!(md.contains("1970-01-01 05:00 UTC Warning: Issue 5 (detail)"));
        assert!(md.contains("Rules stepped in 2 time(s); cleanup freed 1.0 GB."));
    }

    #[test]
    fn html_escapes_text_from_the_machine_and_has_no_script() {
        let html = to_html(&busy_week());
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("&lt;b&gt;Evil|App&lt;/b&gt;"));
        assert!(html.contains("Turned off &lt;Updater&gt; at login"));
        assert!(!html.contains("<b>") && !html.contains("<script"));
        assert!(html.contains("<h2>Disks</h2>") && html.contains("<h2>Issues</h2>"));
        assert_eq!(escape_html("a&\"'<>"), "a&amp;&quot;&#39;&lt;&gt;");
    }

    #[test]
    fn a_week_without_a_recorded_score_says_so() {
        let pts = [HistoryPoint::default()];
        let mut i = input(&pts);
        i.score = None;
        i.score_before = Some(70);
        let r = weekly(i);
        assert_eq!((r.score, r.score_change), (None, 0));
        assert!(to_markdown(&r).contains("No health score was recorded this week."));
    }

    #[test]
    fn dates_are_civil_utc() {
        assert_eq!(utc_date(Timestamp(0)), "1970-01-01");
        assert_eq!(utc_time(Timestamp(951_782_400_000 + 3_660_000)), "2000-02-29 01:01 UTC");
        assert_eq!(utc_date(Timestamp(-86_400_000)), "1969-12-31");
        assert_eq!(utc_date(Timestamp(1_760_000_000_000)), "2025-10-09");
    }

    #[test]
    fn formats_and_destinations() {
        assert_eq!((ReportFormat::Markdown.extension(), ReportFormat::Html.extension()), ("md", "html"));
        assert!(ReportFormat::Html.render(&busy_week()).contains("<html"));
        assert!(ReportFormat::Markdown.render(&busy_week()).starts_with("# "));
        let ok = if cfg!(windows) { "C:\\r\\week.MD" } else { "/r/week.MD" };
        assert!(validate_destination(ok, ReportFormat::Markdown).is_ok());
        let wrong = if cfg!(windows) { "C:\\r\\week.md" } else { "/r/week.md" };
        assert!(validate_destination(wrong, ReportFormat::Html).unwrap_err().contains(".html"));
        for bad in ["", " ", "week.md", "/r/../week.md", "/r/.md", "/r/week.md\0"] {
            assert!(validate_destination(bad, ReportFormat::Markdown).is_err() || cfg!(windows), "{bad}");
        }
        assert!(validate_destination(&format!("/{}.md", "a".repeat(2_000)), ReportFormat::Markdown).is_err());
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

    #[test]
    fn hardware_lines_total_the_week_and_never_overstate() {
        use crate::hardware::Confidence;
        use crate::lifespan::{unavailable, LifeOutcome, LifeSubject};
        use crate::throttle::{ThrottleCause, ThrottleEpisode};
        use crate::time::MINUTE;
        let ep = |s: i64, e: Option<i64>, cause| ThrottleEpisode {
            id: ThrottleEpisode::id_for(Timestamp(s * MINUTE)),
            start: Timestamp(s * MINUTE),
            end: e.map(|m| Timestamp(m * MINUTE)),
            cause,
            avg_lost_percent: 30.0,
            peak_lost_percent: 40.0,
            peak_temperature_c: None,
        };
        let (from, to) = (Timestamp(100 * MINUTE), Timestamp(1_000 * MINUTE));
        let eps = [
            ep(0, Some(110), ThrottleCause::Thermal),     // 10 minutes fall inside the week
            ep(200, Some(220), ThrottleCause::Thermal),   // 20
            ep(990, None, ThrottleCause::Power),          // still running at the end: 10
            ep(2_000, Some(2_100), ThrottleCause::Power), // after the week
        ];
        let mut ssd = unavailable(LifeSubject::Ssd, "Samsung", "x");
        ssd.outcome = LifeOutcome::Remaining {
            low_days: 100.0,
            mid_days: 200.0,
            high_days: 300.0,
        };
        ssd.headline = "About 7 months of writes left".into();
        ssd.confidence = Confidence::Medium;
        let mut bat = unavailable(LifeSubject::Battery, "Battery", "x");
        bat.outcome = LifeOutcome::AlreadyPastLimit;
        bat.headline = "Time to think about a new battery".into();
        let none = unavailable(LifeSubject::Ssd, "Other", "unreadable");
        let lines = hardware_lines(&eps, from, to, &[ssd, bat, none]);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(
            lines[0].contains("40 min") && lines[0].contains("30 min of heat") && lines[0].contains("10 min of power limit"),
            "{}",
            lines[0]
        );
        assert!(
            lines[1].starts_with("Samsung: about 7 months") && lines[1].contains("confidence medium"),
            "{}",
            lines[1]
        );
        assert_eq!(lines[2], "Battery: Time to think about a new battery");
        assert!(hardware_lines(&[], from, to, &[]).is_empty());
        assert!(hardware_lines(&[ep(0, Some(0), ThrottleCause::Power)], from, to, &[]).is_empty());

        let mut i = input(&[]);
        i.hardware = lines.clone();
        i.crashes = 1;
        let r = weekly(i);
        assert!(r.highlights.iter().any(|h| h.starts_with("The CPU ran slower")));
        assert!(to_markdown(&r).contains("## Hardware\n\n- The CPU ran slower"));
        assert!(to_html(&r).contains("<h2>Hardware</h2>"));
        assert!(!to_markdown(&weekly(input(&[]))).contains("## Hardware"));
    }
}
