//! Issues: the problems the operating system itself recorded (application crashes and hangs, bad shutdowns, hardware
//! warnings, failing services, memory exhaustion, update and driver failures), normalised into one [`Issue`] type.
//! Per-OS readers live in `dyno-platform` behind [`IssueSource`]; this module is the pure part: the types, text
//! hygiene, the daily stability score, leaderboards and trends ([`stats`]). Observe-only: Dyno reads logs the OS
//! already keeps, never writes them, and shows the event text as plain text (ADR 0008, WP-06).

pub mod stats;

use crate::health::Severity;
use crate::ports::PlatformError;
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};

pub use stats::{
    build_overview, collapse_across_sources, crash_days, crash_loops, daily_scores, day_score, stability, DayScore, IssuesOverview,
    Stability, Tally, Trend,
};

/// Longest app name kept, in characters.
pub const MAX_APP: usize = 120;
/// Longest title kept, in characters.
pub const MAX_TITLE: usize = 160;
/// Longest evidence text kept, in characters.
pub const MAX_EVIDENCE: usize = 1_200;
/// Most issues one reader may return per read (a log storm must not flood the store).
pub const MAX_PER_READ: usize = 500;
/// Most issues the page lists.
pub const MAX_LISTED: usize = 500;
/// The longest window the page can ask for, in days.
pub const MAX_DAYS: u32 = 90;

/// What went wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IssueKind {
    /// An application ended abnormally.
    AppCrash,
    /// An application stopped responding.
    AppHang,
    /// The machine shut down without warning (power loss, kernel panic, a forced reset).
    BadShutdown,
    /// The disk or file system reported an error.
    DiskError,
    /// The memory or processor reported a hardware error.
    MemoryError,
    /// The machine got hot enough to slow down or shut down.
    Overheating,
    /// A service failed to start or stopped unexpectedly.
    ServiceFailure,
    /// The system ran out of memory and ended programs.
    MemoryExhaustion,
    /// An update failed to install.
    UpdateFailure,
    /// A driver failed or had to be reset.
    DriverFailure,
}

impl IssueKind {
    /// Every kind, in the order the page lists them.
    pub const ALL: [IssueKind; 10] = [
        IssueKind::AppCrash,
        IssueKind::AppHang,
        IssueKind::BadShutdown,
        IssueKind::DiskError,
        IssueKind::MemoryError,
        IssueKind::Overheating,
        IssueKind::ServiceFailure,
        IssueKind::MemoryExhaustion,
        IssueKind::UpdateFailure,
        IssueKind::DriverFailure,
    ];

    /// The user-facing name.
    pub fn label(self) -> &'static str {
        match self {
            IssueKind::AppCrash => "App crash",
            IssueKind::AppHang => "App not responding",
            IssueKind::BadShutdown => "Unexpected shutdown",
            IssueKind::DiskError => "Disk error",
            IssueKind::MemoryError => "Memory error",
            IssueKind::Overheating => "Overheating",
            IssueKind::ServiceFailure => "Service failure",
            IssueKind::MemoryExhaustion => "Out of memory",
            IssueKind::UpdateFailure => "Update failure",
            IssueKind::DriverFailure => "Driver failure",
        }
    }

    /// How serious one occurrence is.
    pub fn severity(self) -> Severity {
        match self {
            IssueKind::BadShutdown | IssueKind::DiskError | IssueKind::MemoryError | IssueKind::Overheating => Severity::Critical,
            IssueKind::AppCrash | IssueKind::AppHang | IssueKind::MemoryExhaustion | IssueKind::DriverFailure => Severity::Warning,
            IssueKind::ServiceFailure | IssueKind::UpdateFailure => Severity::Info,
        }
    }

    /// How many stability points one occurrence costs (see [`stats::day_score`]).
    pub fn weight(self) -> u32 {
        match self {
            IssueKind::BadShutdown | IssueKind::MemoryError => 20,
            IssueKind::DiskError => 15,
            IssueKind::Overheating | IssueKind::MemoryExhaustion => 10,
            IssueKind::AppCrash | IssueKind::DriverFailure => 8,
            IssueKind::AppHang | IssueKind::ServiceFailure => 5,
            IssueKind::UpdateFailure => 3,
        }
    }
}

/// Where an issue was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IssueSourceId {
    /// macOS `~/Library/Logs/DiagnosticReports` (the current user's crash and hang reports).
    MacReports,
    /// The macOS unified log, through `log show`.
    MacLog,
    /// The Windows Event Log (Application, System and Windows Update channels).
    WindowsEvents,
    /// The systemd journal.
    Journal,
    /// `coredumpctl`.
    Coredumps,
}

impl IssueSourceId {
    /// The user-facing name.
    pub fn label(self) -> &'static str {
        match self {
            IssueSourceId::MacReports => "Crash reports",
            IssueSourceId::MacLog => "System log",
            IssueSourceId::WindowsEvents => "Windows Event Log",
            IssueSourceId::Journal => "System journal",
            IssueSourceId::Coredumps => "Core dumps",
        }
    }
}

/// One problem the OS recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    /// Stable id: the same event read twice has the same id (so re-reading a log never duplicates it).
    pub id: String,
    /// When it happened.
    pub at: Timestamp,
    /// What kind.
    pub kind: IssueKind,
    /// How serious one occurrence of its kind is (set from the kind, so every reader and the page agree).
    pub severity: Severity,
    /// The application, service or device it concerns; empty when none applies.
    pub app: String,
    /// One line.
    pub title: String,
    /// The OS's own words (bounded, plain text: shown, never interpreted as markup).
    pub evidence: String,
    /// Where it was read.
    pub source: IssueSourceId,
}

impl Issue {
    /// An issue with text cleaned and bounded. `key` is whatever distinguishes this event from another of the same kind,
    /// app and time (a record number, a report's incident id); it only feeds the id.
    pub fn new(source: IssueSourceId, at: Timestamp, kind: IssueKind, app: &str, title: &str, evidence: &str, key: &str) -> Issue {
        let app = clean(app, MAX_APP);
        Issue {
            id: format!("{:016x}", fnv1a(&format!("{source:?}|{kind:?}|{}|{app}|{key}", at.millis()))),
            at,
            kind,
            severity: kind.severity(),
            title: clean(title, MAX_TITLE),
            evidence: clean(evidence, MAX_EVIDENCE),
            app,
            source,
        }
    }
}

/// Plain text: control characters become spaces (newlines are kept as `\n` in evidence), runs of blanks collapse and the
/// result is cut to `max` characters with an ellipsis.
pub fn clean(text: &str, max: usize) -> String {
    let mut out = String::with_capacity(text.len().min(max + 4));
    let mut blank = false;
    let mut newline = false;
    for c in text.trim().chars() {
        if c == '\n' || c == '\r' {
            newline = true;
            blank = false;
            continue;
        }
        if c.is_control() || c.is_whitespace() {
            blank = true;
            continue;
        }
        if newline && !out.is_empty() {
            out.push('\n');
        } else if blank && !out.is_empty() {
            out.push(' ');
        }
        newline = false;
        blank = false;
        out.push(c);
    }
    if out.chars().count() > max {
        let cut: String = out.chars().take(max.saturating_sub(1)).collect();
        return format!("{}…", cut.trim_end());
    }
    out
}

/// FNV-1a, 64 bit: a stable, dependency-free hash for ids (not a security measure).
pub fn fnv1a(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// A reader of one OS problem log. Observe-only. Contract (like every port): no panics; a source the OS refuses or lacks
/// is a [`PlatformError`] with a reason the user can read (it becomes an "unavailable: reason" entry, never an error for
/// the page); external tools run with timeouts; at most [`MAX_PER_READ`] issues are returned, newest first.
pub trait IssueSource: Send + Sync {
    /// Which source this is.
    fn id(&self) -> IssueSourceId;
    /// The issues that happened in `[since, until]`.
    fn read(&self, since: Timestamp, until: Timestamp) -> Result<Vec<Issue>, PlatformError>;
}

/// What the last read of a source found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceState {
    /// Read; this many issues were found in the last read.
    Read(u32),
    /// Could not be read, and why.
    Unavailable(String),
    /// Not read yet.
    Pending,
}

/// One source and how its last read went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceStatus {
    /// Which.
    pub source: IssueSourceId,
    /// Its name.
    pub label: String,
    /// How the last read went.
    pub state: SourceState,
    /// When it was last tried.
    pub at: Option<Timestamp>,
}

impl SourceStatus {
    /// A source that has not been read yet.
    pub fn pending(source: IssueSourceId) -> SourceStatus {
        SourceStatus {
            source,
            label: source.label().into(),
            state: SourceState::Pending,
            at: None,
        }
    }

    /// The status after reading `result` at `at`.
    pub fn after(source: IssueSourceId, at: Timestamp, result: &Result<Vec<Issue>, PlatformError>) -> SourceStatus {
        SourceStatus {
            source,
            label: source.label().into(),
            state: match result {
                Ok(v) => SourceState::Read(v.len() as u32),
                Err(e) => SourceState::Unavailable(clean(&e.to_string(), MAX_TITLE)),
            },
            at: Some(at),
        }
    }

    /// Whether the last read worked.
    pub fn is_readable(&self) -> bool {
        matches!(self.state, SourceState::Read(_))
    }
}

/// What the page asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct IssuesQuery {
    /// How many days back (1 to [`MAX_DAYS`]).
    pub days: u32,
}

impl Default for IssuesQuery {
    fn default() -> Self {
        IssuesQuery { days: 14 }
    }
}

impl IssuesQuery {
    /// The query with `days` in range.
    pub fn normalised(self) -> IssuesQuery {
        IssuesQuery {
            days: self.days.clamp(1, MAX_DAYS),
        }
    }
}

/// A written issues file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedIssues {
    /// Where it was written.
    pub path: String,
    /// Its size in bytes.
    pub bytes: u64,
    /// How many issues it holds.
    pub count: u32,
}

/// Checks the destination of an issues export: absolute, a `.jsonl` file name, no dot segments.
pub fn validate_export_path(path: &str) -> Result<std::path::PathBuf, String> {
    if path.trim().is_empty() || path.len() > 1024 || path.contains('\0') {
        return Err("Choose where to save the issues.".into());
    }
    let p = std::path::Path::new(path);
    if !p.is_absolute() {
        return Err("Give the full path of the file to save.".into());
    }
    if crate::disk::has_dot_segment(p) {
        return Err("The save path may not contain \"..\".".into());
    }
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
    if !name.ends_with(".jsonl") || name.len() <= ".jsonl".len() {
        return Err("Save the issues as a file ending in .jsonl.".into());
    }
    Ok(p.to_path_buf())
}

/// Renders issues as JSON lines (one object per line, oldest first) for Debrief: `ts` (RFC 3339 UTC), `level`, `source`,
/// `app`, `kind` and `message`. Debrief reads this shape with its generic JSON-lines importer.
pub fn to_jsonl(issues: &[Issue]) -> String {
    let mut sorted: Vec<&Issue> = issues.iter().collect();
    sorted.sort_by_key(|i| (i.at, i.id.clone()));
    let mut out = String::new();
    for i in sorted {
        let level = match i.severity {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Critical => "error",
        };
        let line = serde_json::json!({
            "ts": rfc3339(i.at),
            "level": level,
            "source": i.source.label(),
            "app": i.app,
            "kind": i.kind.label(),
            "message": if i.evidence.is_empty() { i.title.clone() } else { format!("{} - {}", i.title, i.evidence) },
        });
        out.push_str(&line.to_string());
        out.push('\n');
    }
    out
}

/// `2026-10-22T10:11:12.123Z` for a timestamp (proleptic Gregorian, UTC).
pub fn rfc3339(at: Timestamp) -> String {
    let ms = at.millis();
    let days = ms.div_euclid(crate::time::DAY);
    let rem = ms.rem_euclid(crate::time::DAY);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / crate::time::HOUR,
        rem % crate::time::HOUR / crate::time::MINUTE,
        rem % crate::time::MINUTE / crate::time::SECOND,
        rem % crate::time::SECOND
    )
}

/// Year, month and day for days since 1970-01-01 (Howard Hinnant's algorithm).
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Days since 1970-01-01 for a civil date.
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = i64::from(if m > 2 { m - 3 } else { m + 9 });
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(at: i64, kind: IssueKind, app: &str) -> Issue {
        Issue::new(IssueSourceId::MacReports, Timestamp(at), kind, app, "t", "e", "k")
    }

    #[test]
    fn ids_are_stable_and_distinguish_events() {
        let a = issue(1_000, IssueKind::AppCrash, "Safari");
        assert_eq!(a.id, issue(1_000, IssueKind::AppCrash, "Safari").id);
        assert_ne!(a.id, issue(1_001, IssueKind::AppCrash, "Safari").id);
        assert_ne!(a.id, issue(1_000, IssueKind::AppHang, "Safari").id);
        assert_ne!(a.id, issue(1_000, IssueKind::AppCrash, "Mail").id);
        assert_eq!(a.id.len(), 16);
    }

    #[test]
    fn text_is_cleaned_and_bounded() {
        assert_eq!(clean("  a \t b\u{7}c  ", 50), "a b c");
        assert_eq!(clean("line one\r\n\r\n  line two", 50), "line one\nline two");
        assert_eq!(clean(&"x".repeat(300), 10), "xxxxxxxxx…");
        assert_eq!(clean("", 5), "");
        let i = Issue::new(
            IssueSourceId::MacLog,
            Timestamp(0),
            IssueKind::AppHang,
            &"a".repeat(500),
            &"t".repeat(500),
            &"e".repeat(5_000),
            "",
        );
        assert_eq!(i.app.chars().count(), MAX_APP);
        assert_eq!(i.title.chars().count(), MAX_TITLE);
        assert_eq!(i.evidence.chars().count(), MAX_EVIDENCE);
        // Markup stays as text: nothing is interpreted here, and the UI never binds it as HTML.
        assert_eq!(clean("<b>x</b>", 50), "<b>x</b>");
    }

    #[test]
    fn every_kind_has_a_label_severity_and_weight() {
        for k in IssueKind::ALL {
            assert!(!k.label().is_empty());
            assert!(k.weight() > 0 && k.weight() <= 20);
            let i = Issue::new(IssueSourceId::Journal, Timestamp(0), k, "a", "t", "e", "k");
            assert_eq!(i.severity, k.severity());
        }
        assert_eq!(IssueKind::ALL.len(), 10);
        assert_eq!(IssueKind::BadShutdown.severity(), Severity::Critical);
        assert_eq!(IssueKind::UpdateFailure.severity(), Severity::Info);
        assert_eq!(IssueKind::AppCrash.severity(), Severity::Warning);
    }

    #[test]
    fn source_status_reports_why_a_source_is_unavailable() {
        let ok = SourceStatus::after(IssueSourceId::Journal, Timestamp(5), &Ok(vec![issue(1, IssueKind::AppCrash, "a")]));
        assert_eq!(ok.state, SourceState::Read(1));
        assert!(ok.is_readable());
        let bad = SourceStatus::after(
            IssueSourceId::Journal,
            Timestamp(5),
            &Err(PlatformError::AccessDenied("not in the journal group".into())),
        );
        assert_eq!(bad.state, SourceState::Unavailable("not in the journal group".into()));
        assert!(!bad.is_readable());
        assert_eq!(SourceStatus::pending(IssueSourceId::MacLog).state, SourceState::Pending);
        assert_eq!(bad.label, "System journal");
    }

    #[test]
    fn query_is_clamped() {
        assert_eq!(IssuesQuery { days: 0 }.normalised().days, 1);
        assert_eq!(IssuesQuery { days: 9_999 }.normalised().days, MAX_DAYS);
        assert_eq!(IssuesQuery::default().days, 14);
    }

    #[test]
    fn dates_round_trip() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(days_from_civil(2026, 10, 22)), (2026, 10, 22));
        assert_eq!(civil_from_days(days_from_civil(2024, 2, 29)), (2024, 2, 29));
        assert_eq!(days_from_civil(2000, 3, 1) - days_from_civil(2000, 2, 28), 2);
        assert_eq!(rfc3339(Timestamp(0)), "1970-01-01T00:00:00.000Z");
        assert_eq!(rfc3339(Timestamp(1_761_127_872_123)), "2025-10-22T10:11:12.123Z");
    }

    #[test]
    fn export_paths_are_checked() {
        #[cfg(unix)]
        {
            assert!(validate_export_path("/tmp/issues.jsonl").is_ok());
            assert!(validate_export_path("/tmp/ISSUES.JSONL").is_ok());
            assert!(validate_export_path("/tmp/../etc/x.jsonl").is_err());
            assert!(validate_export_path("/tmp/issues.txt").is_err());
        }
        assert!(validate_export_path("issues.jsonl").is_err());
    }

    #[test]
    fn jsonl_export_is_one_object_per_line_oldest_first() {
        let a = issue(2_000, IssueKind::AppCrash, "Safari");
        let b = issue(1_000, IssueKind::BadShutdown, "");
        let text = to_jsonl(&[a, b]);
        let lines: Vec<serde_json::Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["kind"], "Unexpected shutdown");
        assert_eq!(lines[0]["level"], "error");
        assert_eq!(lines[1]["app"], "Safari");
        assert_eq!(lines[1]["level"], "warning");
        assert_eq!(lines[1]["message"], "t - e");
        assert!(lines[1]["ts"].as_str().unwrap().ends_with('Z'));
        assert_eq!(to_jsonl(&[]), "");
    }
}
