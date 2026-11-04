//! macOS: the current user's crash and hang reports (`~/Library/Logs/DiagnosticReports`) and the unified log through
//! `log show`. Both are the user's own data; Dyno reads report headers and a bounded part of each report, keeps a short
//! plain-text summary and never logs report contents.

use super::{hours_back, parse_timestamp, timeout_for, ToolRunner};
use dyno_core::issues::{Issue, IssueKind, IssueSource, IssueSourceId, NoControl, ReadControl, MAX_PER_READ};
use dyno_core::ports::PlatformError;
use dyno_core::time::Timestamp;
use std::path::{Path, PathBuf};

/// Most bytes read from one report.
const MAX_REPORT_BYTES: usize = 512 * 1024;
/// Most report files looked at per read.
const MAX_FILES: usize = 2_000;
/// Files older than the window by more than this are not opened (the name carries a local time, the file time is exact).
const SLACK_MS: i64 = 36 * 3_600_000;

/// The crash and hang reports in a folder.
pub struct MacReports {
    dir: PathBuf,
}

impl MacReports {
    /// Reads reports from `dir`.
    pub fn new(dir: PathBuf) -> MacReports {
        MacReports { dir }
    }

    /// The current user's `~/Library/Logs/DiagnosticReports`.
    pub fn for_user() -> MacReports {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        MacReports::new(home.join("Library").join("Logs").join("DiagnosticReports"))
    }
}

impl IssueSource for MacReports {
    fn id(&self) -> IssueSourceId {
        IssueSourceId::MacReports
    }

    fn read(&self, since: Timestamp, until: Timestamp) -> Result<Vec<Issue>, PlatformError> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(e) => e,
            // No folder means nothing has ever crashed (or reports are off): that is a valid, empty answer.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                return Err(PlatformError::AccessDenied(
                    "Dyno may not read your crash reports. Allow it in System Settings, Privacy & Security, Files and Folders.".into(),
                ))
            }
            Err(e) => return Err(PlatformError::Failed(format!("The crash reports folder could not be read ({e})."))),
        };
        let mut out = Vec::new();
        for entry in entries.filter_map(Result::ok).take(MAX_FILES) {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
                continue;
            };
            if !path.is_file() || !is_report(&name) {
                continue;
            }
            let Some(mtime) = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| Timestamp(i64::try_from(d.as_millis()).unwrap_or(i64::MAX)))
            else {
                continue;
            };
            if mtime.plus(SLACK_MS) < since || mtime > until.plus(SLACK_MS) {
                continue;
            }
            let (head, complete) = read_head(&path);
            if let Some(issue) = report_issue(&name, mtime, &head, complete) {
                if issue.at >= since && issue.at <= until {
                    out.push(issue);
                }
            }
        }
        out.sort_by(|a, b| b.at.cmp(&a.at).then(a.id.cmp(&b.id)));
        out.truncate(MAX_PER_READ);
        Ok(out)
    }
}

/// Reads at most [`MAX_REPORT_BYTES`] of a file; the flag says the whole file was read.
fn read_head(path: &Path) -> (Vec<u8>, bool) {
    use std::io::Read;
    let Ok(file) = std::fs::File::open(path) else {
        return (Vec::new(), false);
    };
    let mut buf = Vec::new();
    let mut limited = file.take(MAX_REPORT_BYTES as u64 + 1);
    if limited.read_to_end(&mut buf).is_err() {
        return (Vec::new(), false);
    }
    let complete = buf.len() <= MAX_REPORT_BYTES;
    buf.truncate(MAX_REPORT_BYTES);
    (buf, complete)
}

fn is_report(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".ips", ".crash", ".hang", ".spin", ".panic"].iter().any(|e| lower.ends_with(e))
}

/// The app part of a report's file name: everything before the `-YYYY-MM-DD-HHMMSS` suffix (and a `_host` or `.nnn` tail).
pub fn app_from_file_name(name: &str) -> String {
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    let bytes = stem.as_bytes();
    // `-2026-10-22-204857` is 18 bytes, optionally followed by `.000` style duplicates already removed by the extension cut.
    let mut end = stem.len();
    for i in (0..bytes.len().saturating_sub(16)).rev() {
        let tail = &stem[i..];
        if tail.len() >= 16
            && tail.starts_with('-')
            && tail.as_bytes()[5] == b'-'
            && tail.as_bytes()[8] == b'-'
            && tail[1..5].bytes().all(|c| c.is_ascii_digit())
        {
            end = i;
            break;
        }
    }
    let app = &stem[..end];
    // Duplicated reports from one second are `name-2026-10-22-194919.0002.ips`: the stem still ends in `.0002`.
    let app = app.split("_2026-").next().unwrap_or(app);
    app.trim_start_matches("ExcUserFault_").to_string()
}

/// One report as an issue, or `None` when it is not a crash, hang or panic. `head` is the start of the file, `complete`
/// says it is the whole file. Pure.
pub fn report_issue(file_name: &str, mtime: Timestamp, head: &[u8], complete: bool) -> Option<Issue> {
    let lower = file_name.to_ascii_lowercase();
    let text = String::from_utf8_lossy(head);
    let (kind, app, at, evidence, key) = if lower.ends_with(".ips") {
        let (first, body) = text.split_once('\n').unwrap_or((&text, ""));
        let header: serde_json::Value = serde_json::from_str(first.trim()).ok()?;
        let bug = header
            .get("bug_type")
            .and_then(|v| v.as_str().map(str::to_string).or_else(|| v.as_i64().map(|n| n.to_string())))?;
        let kind = match bug.as_str() {
            "309" => IssueKind::AppCrash,
            _ => return None,
        };
        let app = header
            .get("app_name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map_or_else(|| app_from_file_name(file_name), str::to_string);
        let at = header
            .get("timestamp")
            .and_then(|v| v.as_str())
            .and_then(parse_timestamp)
            .unwrap_or(mtime);
        let mut evidence = Vec::new();
        if let Some(v) = header.get("app_version").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            evidence.push(format!("Version {v}."));
        }
        if complete {
            if let Ok(b) = serde_json::from_str::<serde_json::Value>(body.trim()) {
                evidence.extend(crash_evidence(&b));
            }
        }
        if let Some(os) = header.get("os_version").and_then(|v| v.as_str()) {
            evidence.push(os.to_string());
        }
        let key = header.get("incident_id").and_then(|v| v.as_str()).unwrap_or(file_name).to_string();
        (kind, app, at, evidence.join(" "), key)
    } else {
        let kind = if lower.ends_with(".crash") {
            IssueKind::AppCrash
        } else if lower.ends_with(".hang") || lower.ends_with(".spin") {
            IssueKind::AppHang
        } else if lower.ends_with(".panic") {
            IssueKind::BadShutdown
        } else {
            return None;
        };
        let app = if kind == IssueKind::BadShutdown {
            String::new()
        } else {
            app_from_file_name(file_name)
        };
        (kind, app, mtime, format!("Report: {file_name}"), file_name.to_string())
    };
    let title = match kind {
        IssueKind::AppCrash => format!("{} quit unexpectedly", name_or(&app, "An app")),
        IssueKind::AppHang => format!("{} stopped responding", name_or(&app, "An app")),
        _ => "The Mac restarted because of a problem".to_string(),
    };
    Some(Issue::new(IssueSourceId::MacReports, at, kind, &app, &title, &evidence, &key))
}

fn name_or<'a>(app: &'a str, fallback: &'a str) -> &'a str {
    if app.is_empty() {
        fallback
    } else {
        app
    }
}

/// A short account of why a process crashed, from the report's exception and termination records.
fn crash_evidence(body: &serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(ex) = body.get("exception") {
        let ty = ex.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let sig = ex.get("signal").and_then(|v| v.as_str()).unwrap_or("");
        match (ty.is_empty(), sig.is_empty()) {
            (false, false) => out.push(format!("Exception {ty} ({sig}).")),
            (false, true) => out.push(format!("Exception {ty}.")),
            (true, false) => out.push(format!("Signal {sig}.")),
            _ => {}
        }
    }
    if let Some(t) = body.get("termination") {
        let reasons: Vec<&str> = t
            .get("reasons")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str()).take(2).collect())
            .unwrap_or_default();
        if !reasons.is_empty() {
            out.push(format!("{}.", reasons.join("; ").trim_end_matches('.')));
        } else if let Some(i) = t.get("indicator").and_then(|v| v.as_str()) {
            out.push(format!("{i}."));
        }
    }
    out
}

/// What the unified log reader asks for. Kernel shutdown causes, memory pressure kills, storage errors and launchd
/// services that exit abnormally; everything else in the log is none of Dyno's business.
pub const LOG_PREDICATE: &str = "(process == \"kernel\" AND (eventMessage CONTAINS \"Previous shutdown cause\" OR eventMessage CONTAINS \"low swap: killing\" OR eventMessage CONTAINS \"VM compressor space shortage\" OR eventMessage CONTAINS \"I/O error\")) OR (process == \"launchd\" AND eventMessage CONTAINS \"Service exited with abnormal code\")";

/// The unified log through `/usr/bin/log show`, run at lower priority (reading a day of log is slow and must not slow the user's work).
pub struct MacLog {
    runner: Box<dyn ToolRunner>,
}

impl MacLog {
    /// A reader running `log` through `runner`.
    pub fn new(runner: Box<dyn ToolRunner>) -> MacLog {
        MacLog { runner }
    }
}

impl IssueSource for MacLog {
    fn id(&self) -> IssueSourceId {
        IssueSourceId::MacLog
    }

    fn read(&self, since: Timestamp, until: Timestamp) -> Result<Vec<Issue>, PlatformError> {
        self.read_controlled(since, until, &NoControl)
    }

    fn controllable(&self) -> bool {
        true
    }

    /// Streams `log show`: every record parsed so far is reported, and Cancel kills the child process at once.
    fn read_controlled(&self, since: Timestamp, until: Timestamp, control: &dyn ReadControl) -> Result<Vec<Issue>, PlatformError> {
        let hours = hours_back(since, until).min(72);
        let args: Vec<String> = ["show", "--last"]
            .iter()
            .map(|s| s.to_string())
            .chain([
                format!("{hours}h"),
                "--style".into(),
                "ndjson".into(),
                "--predicate".into(),
                LOG_PREDICATE.into(),
            ])
            .collect();
        let mut out: Vec<Issue> = Vec::new();
        let mut records = 0u64;
        self.runner
            .run_lines("/usr/bin/log", &args, timeout_for(hours), control, &mut |line| {
                records += 1;
                if records.is_multiple_of(PROGRESS_EVERY) || records == 1 {
                    control.parsed(records);
                }
                if let Some(i) = log_issue(line) {
                    if i.at >= since && i.at <= until {
                        out.push(i);
                    }
                }
            })?;
        control.parsed(records);
        out.sort_by(|a, b| b.at.cmp(&a.at).then(a.id.cmp(&b.id)));
        out.dedup_by(|a, b| a.id == b.id);
        out.truncate(MAX_PER_READ);
        Ok(out)
    }
}

/// How many records are parsed between two reports of progress (the owner throttles what reaches the screen).
const PROGRESS_EVERY: u64 = 64;

/// One `log show --style ndjson` line as an issue, when it is one of the events Dyno curates. Pure.
pub fn log_issue(line: &str) -> Option<Issue> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let message = v.get("eventMessage")?.as_str()?;
    let process = v
        .get("processImagePath")
        .and_then(|p| p.as_str())
        .map(|p| dyno_core::grouping::file_name(p).to_string())
        .unwrap_or_default();
    let at = parse_timestamp(v.get("timestamp")?.as_str()?)?;
    let (kind, app, title) = classify_log(&process, message)?;
    let key = v.get("machTimestamp").map(|m| m.to_string()).unwrap_or_default();
    Some(Issue::new(IssueSourceId::MacLog, at, kind, &app, &title, message, &key))
}

/// What a log message from `process` means, if anything. Pure.
pub fn classify_log(process: &str, message: &str) -> Option<(IssueKind, String, String)> {
    match process {
        "kernel" => {
            if let Some(rest) = message.split("Previous shutdown cause:").nth(1) {
                let cause: i64 = rest
                    .trim()
                    .split(|c: char| !(c.is_ascii_digit() || c == '-'))
                    .next()?
                    .parse()
                    .ok()?;
                // 5 is a normal shutdown. Zero and negative causes (power loss, a panic, a hardware trip) and 3 (the power
                // button held down) are not; other positive values are left alone because their meaning is not documented.
                return (cause <= 0 || cause == 3).then(|| {
                    (
                        IssueKind::BadShutdown,
                        String::new(),
                        format!("The Mac shut down without warning (cause {cause})"),
                    )
                });
            }
            if let Some(rest) = message.split("low swap: killing").nth(1) {
                let app = rest
                    .rsplit_once('(')
                    .and_then(|(_, r)| r.split(')').next())
                    .unwrap_or("")
                    .to_string();
                return Some((
                    IssueKind::MemoryExhaustion,
                    app,
                    "The Mac ran out of memory and ended a program".into(),
                ));
            }
            if message.contains("VM compressor space shortage") {
                return Some((
                    IssueKind::MemoryExhaustion,
                    String::new(),
                    "The Mac ran out of memory and ended a program".into(),
                ));
            }
            if message.contains("I/O error") {
                return Some((IssueKind::DiskError, String::new(), "A disk reported an input/output error".into()));
            }
            None
        }
        "launchd" => {
            let (label, rest) = message.split_once(": ")?;
            if !rest.contains("Service exited with abnormal code") {
                return None;
            }
            let label = label.split('[').next().unwrap_or(label).trim().to_string();
            Some((
                IssueKind::ServiceFailure,
                label.clone(),
                format!("{} stopped with an error", name_or(&label, "A background service")),
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::Scripted;
    use super::*;

    const HEADER: &str = r#"{"app_name":"bench_reads_base","timestamp":"2026-10-22 20:48:57.00 +0800","app_version":"1.2","bug_type":"309","os_version":"macOS 27.0.1 (26A434)","incident_id":"C439B0A0","name":"bench_reads_base"}"#;
    const BODY: &str = r#"{"exception":{"type":"EXC_CRASH","signal":"SIGABRT"},"termination":{"namespace":"DYLD","indicator":"Library missing","reasons":["Library not loaded: @rpath/libduckdb.dylib","Referenced from: x"]}}"#;

    #[test]
    fn a_crash_report_becomes_an_issue_with_evidence() {
        let text = format!("{HEADER}\n{BODY}");
        let i = report_issue("bench_reads_base-2026-10-22-204857.ips", Timestamp(1), text.as_bytes(), true).unwrap();
        assert_eq!(i.kind, IssueKind::AppCrash);
        assert_eq!(i.app, "bench_reads_base");
        assert_eq!(i.at, parse_timestamp("2026-10-22T12:48:57Z").unwrap());
        assert_eq!(i.title, "bench_reads_base quit unexpectedly");
        assert!(i.evidence.contains("Exception EXC_CRASH (SIGABRT)."), "{}", i.evidence);
        assert!(
            i.evidence
                .contains("Library not loaded: @rpath/libduckdb.dylib; Referenced from: x."),
            "{}",
            i.evidence
        );
        assert!(i.evidence.contains("Version 1.2."));
        assert!(i.evidence.contains("macOS 27.0.1"));
        assert_eq!(i.source, IssueSourceId::MacReports);
    }

    #[test]
    fn a_truncated_or_odd_report_still_yields_the_header_facts() {
        let cut = format!("{HEADER}\n{{\"exception\":{{\"type\":\"EXC_");
        let i = report_issue("a-2026-10-22-204857.ips", Timestamp(1), cut.as_bytes(), false).unwrap();
        assert!(!i.evidence.contains("Exception"));
        let bad_body = format!("{HEADER}\nnot json");
        assert!(report_issue("a-2026-10-22-204857.ips", Timestamp(1), bad_body.as_bytes(), true).is_some());
        // Header alone, no body.
        assert!(report_issue("a-2026-10-22-204857.ips", Timestamp(1), HEADER.as_bytes(), true).is_some());
    }

    #[test]
    fn reports_that_are_not_crashes_are_skipped() {
        let other = HEADER.replace("309", "298");
        assert!(report_issue("a-2026-10-22-204857.ips", Timestamp(1), other.as_bytes(), true).is_none());
        assert!(report_issue("a.ips", Timestamp(1), b"not json at all", true).is_none());
        assert!(report_issue("a.ips", Timestamp(1), b"", true).is_none());
        assert!(report_issue("notes.txt", Timestamp(1), b"{}", true).is_none());
        let numeric = HEADER.replace("\"309\"", "309");
        assert!(report_issue("a.ips", Timestamp(1), numeric.as_bytes(), true).is_some());
    }

    #[test]
    fn text_reports_use_the_file_name_and_time() {
        let c = report_issue("Safari_2026-10-22-101112_host.crash", Timestamp(5), b"", true).unwrap();
        assert_eq!((c.kind, c.app.as_str(), c.at), (IssueKind::AppCrash, "Safari", Timestamp(5)));
        let h = report_issue("Mail_2026-10-22-101112_host.hang", Timestamp(5), b"", true).unwrap();
        assert_eq!(h.kind, IssueKind::AppHang);
        assert_eq!(h.title, "Mail stopped responding");
        let s = report_issue("Mail_2026-10-22-101112_host.spin", Timestamp(5), b"", true).unwrap();
        assert_eq!(s.kind, IssueKind::AppHang);
        let p = report_issue("Kernel_2026-10-22-101112_host.panic", Timestamp(5), b"", true).unwrap();
        assert_eq!(p.kind, IssueKind::BadShutdown);
        assert!(p.app.is_empty());
    }

    #[test]
    fn file_names_give_the_app() {
        assert_eq!(app_from_file_name("bench_reads_base-2026-10-22-204857.ips"), "bench_reads_base");
        assert_eq!(
            app_from_file_name("debrief-ai-fake-worker-2026-10-23-101318.0002.ips"),
            "debrief-ai-fake-worker"
        );
        assert_eq!(
            app_from_file_name("ExcUserFault_com.apple.WebKit.Networking.Development-2026-10-22-191000.ips"),
            "com.apple.WebKit.Networking.Development"
        );
        assert_eq!(app_from_file_name("Safari_2026-10-22-101112_host.crash"), "Safari");
        assert_eq!(app_from_file_name("plain.ips"), "plain");
    }

    #[test]
    fn the_reports_folder_is_read_with_the_window_and_missing_means_empty() {
        let dir = tempfile::tempdir().unwrap();
        let text = format!("{HEADER}\n{BODY}");
        let file = dir.path().join("bench_reads_base-2026-10-22-204857.ips");
        std::fs::write(&file, &text).unwrap();
        let day = parse_timestamp("2026-10-22T12:48:57Z").unwrap();
        let modified = std::time::UNIX_EPOCH + std::time::Duration::from_millis(day.millis() as u64);
        std::fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        std::fs::write(dir.path().join("readme.txt"), "x").unwrap();
        std::fs::create_dir(dir.path().join("Retired")).unwrap();
        let src = MacReports::new(dir.path().to_path_buf());
        let found = src.read(day.minus(60_000), day.plus(60_000)).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].app, "bench_reads_base");
        assert_eq!(src.id(), IssueSourceId::MacReports);
        // A window days away neither opens the file nor returns it.
        assert!(src.read(day.plus(10 * 86_400_000), day.plus(11 * 86_400_000)).unwrap().is_empty());
        assert!(src.read(day.minus(11 * 86_400_000), day.minus(10 * 86_400_000)).unwrap().is_empty());
        // The report's own time decides inside the slack: a window just after it finds nothing.
        assert!(src.read(day.plus(120_000), day.plus(240_000)).unwrap().is_empty());
        let missing = MacReports::new(dir.path().join("nope"));
        assert_eq!(missing.read(Timestamp(0), day).unwrap(), vec![]);
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_folder_says_what_to_allow() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let r = MacReports::new(locked.clone()).read(Timestamp(0), Timestamp(1));
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        // A privileged test runner can read anything; otherwise the reason names the setting to change.
        if let Err(e) = r {
            assert!(
                matches!(e, PlatformError::AccessDenied(ref m) if m.contains("Privacy & Security")),
                "{e}"
            );
        }
    }

    #[test]
    fn log_lines_are_classified() {
        let line = |process: &str, msg: &str| {
            serde_json::json!({
                "processImagePath": format!("/usr/bin/{process}"),
                "eventMessage": msg,
                "timestamp": "2026-10-22 20:48:57.123456+0800",
                "machTimestamp": 99
            })
            .to_string()
        };
        let bad = log_issue(&line("kernel", "AppleSMC: Previous shutdown cause: 0")).unwrap();
        assert_eq!(bad.kind, IssueKind::BadShutdown);
        assert!(bad.title.contains("cause 0"));
        assert_eq!(
            log_issue(&line("kernel", "Previous shutdown cause: -128")).unwrap().kind,
            IssueKind::BadShutdown
        );
        assert_eq!(
            log_issue(&line("kernel", "Previous shutdown cause: 3")).unwrap().kind,
            IssueKind::BadShutdown
        );
        assert!(log_issue(&line("kernel", "Previous shutdown cause: 5")).is_none());
        assert!(log_issue(&line("kernel", "Previous shutdown cause: nonsense")).is_none());
        let mem = log_issue(&line(
            "kernel",
            "low swap: killing largest compressed process with pid 123 (Photoshop)",
        ))
        .unwrap();
        assert_eq!((mem.kind, mem.app.as_str()), (IssueKind::MemoryExhaustion, "Photoshop"));
        assert_eq!(
            log_issue(&line("kernel", "memorystatus: VM compressor space shortage"))
                .unwrap()
                .kind,
            IssueKind::MemoryExhaustion
        );
        assert_eq!(log_issue(&line("kernel", "disk3: I/O error.")).unwrap().kind, IssueKind::DiskError);
        let svc = log_issue(&line("launchd", "com.example.helper[412]: Service exited with abnormal code: 78")).unwrap();
        assert_eq!((svc.kind, svc.app.as_str()), (IssueKind::ServiceFailure, "com.example.helper"));
        assert!(log_issue(&line("launchd", "com.example.helper[412]: Service exited normally")).is_none());
        assert!(log_issue(&line("launchd", "no colon in this one")).is_none());
        assert!(log_issue(&line("someone", "I/O error")).is_none());
        assert!(log_issue("not json").is_none());
        assert!(log_issue(r#"{"eventMessage":"x"}"#).is_none());
        assert_eq!(bad.at.millis() % 1000, 123);
    }

    #[test]
    fn the_log_reader_asks_for_a_niced_bounded_read_and_reports_failures() {
        let line = serde_json::json!({
            "processImagePath": "/kernel",
            "eventMessage": "disk3: I/O error.",
            "timestamp": "2026-10-22 20:48:57.000000+0800",
            "machTimestamp": 1
        })
        .to_string();
        let t = parse_timestamp("2026-10-22T12:48:57Z").unwrap();
        let runner = Scripted::new(vec![
            Ok(format!("{line}\n{line}\ngarbage\n")),
            Err(PlatformError::Timeout("log did not finish within 120 s.".into())),
        ]);
        let calls = std::sync::Arc::new(runner);
        struct Shared(std::sync::Arc<Scripted>);
        impl ToolRunner for Shared {
            fn run_tool(&self, tool: &crate::runner::Tool, t: std::time::Duration) -> Result<crate::runner::ToolOutput, PlatformError> {
                self.0.run_tool(tool, t)
            }
        }
        let log = MacLog::new(Box::new(Shared(calls.clone())));
        let got = log.read(t.minus(3_600_000), t.plus(1_000)).unwrap();
        assert_eq!(got.len(), 1, "the same event twice is one issue");
        let (program, args, timeout) = calls.calls.lock().unwrap()[0].clone();
        assert_eq!(program, "/usr/bin/log", "run at lower priority by the runner, not through nice(1)");
        assert!(args.contains(&"show".to_string()) && args.contains(&"ndjson".to_string()));
        assert!(args.iter().any(|a| a == "2h"), "{args:?}");
        assert_eq!(timeout, std::time::Duration::from_secs(60));
        let err = log.read(t.minus(3 * 86_400_000), t).unwrap_err();
        assert!(matches!(err, PlatformError::Timeout(_)));
        let (_, args, timeout) = calls.calls.lock().unwrap()[1].clone();
        assert!(args.iter().any(|a| a == "72h"), "capped at three days: {args:?}");
        assert_eq!(timeout, std::time::Duration::from_secs(120));
        assert_eq!(log.id(), IssueSourceId::MacLog);
    }

    #[test]
    fn the_log_reader_reports_a_growing_count_of_records_while_it_streams() {
        use std::sync::atomic::{AtomicU64, Ordering};
        struct Count(std::sync::Mutex<Vec<u64>>, AtomicU64);
        impl ReadControl for Count {
            fn cancelled(&self) -> bool {
                false
            }
            fn parsed(&self, n: u64) {
                self.0.lock().unwrap().push(n);
                self.1.store(n, Ordering::SeqCst);
            }
            fn on_cancel(&self, _: Box<dyn Fn() + Send + Sync>) -> Box<dyn Send> {
                Box::new(())
            }
        }
        let t = parse_timestamp("2026-10-22T12:48:57Z").unwrap();
        let text = (0..200)
            .map(|i| format!("{{\"eventMessage\":\"noise {i}\"}}\n"))
            .collect::<String>();
        struct One(String);
        impl ToolRunner for One {
            fn run_tool(&self, _: &crate::runner::Tool, _: std::time::Duration) -> Result<crate::runner::ToolOutput, PlatformError> {
                Ok(crate::runner::ToolOutput {
                    ok: true,
                    stdout: self.0.clone(),
                    error: String::new(),
                })
            }
        }
        let control = Count(Default::default(), AtomicU64::new(0));
        let got = MacLog::new(Box::new(One(text)))
            .read_controlled(t.minus(3_600_000), t, &control)
            .unwrap();
        assert!(got.is_empty());
        let seen = control.0.lock().unwrap().clone();
        assert_eq!(seen.first(), Some(&1), "a count from the very first record");
        assert!(seen.len() >= 4, "{seen:?}");
        assert!(seen.windows(2).all(|w| w[0] <= w[1]), "never backwards: {seen:?}");
        assert_eq!(seen.last(), Some(&200));
        assert!(MacLog::new(Box::new(One(String::new()))).controllable());
    }
}
