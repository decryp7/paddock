//! Linux: the systemd journal (`journalctl`, kernel and service messages matched by a curated pattern) and
//! `coredumpctl` (crashes with a core dump). Both are read-only and run with timeouts. A machine without systemd, or a
//! user who may not read the journal, gets an "unavailable: reason" entry that says what would help.

use super::{hours_back, timeout_for, ToolRunner};
use dyno_core::grouping::file_name;
use dyno_core::issues::{kind_for_signal, Issue, IssueKind, IssueSource, IssueSourceId, MAX_PER_READ};
use dyno_core::ports::PlatformError;
use dyno_core::time::Timestamp;

/// The journal messages worth a look (an extended regular expression for `journalctl --grep`, which uses PCRE2).
pub const JOURNAL_PATTERN: &str = "Out of memory|oom-kill|Killed process|I/O error|EXT4-fs error|BTRFS error|XFS .*(corruption|error)|Machine check|Hardware Error|temperature above threshold|thermal throttling|Failed to start|Failed with result|Main process exited, code=(dumped|killed)|uncleanly shut down|NVRM: Xid|GPU HANG|ring .* timeout";

/// What a journal message means, with the app or unit it names. Pure.
pub fn classify(message: &str, unit: &str) -> Option<(IssueKind, String, String)> {
    let m = message;
    let has = |needles: &[&str]| needles.iter().any(|n| m.contains(n));
    if has(&["Out of memory", "oom-kill", "Killed process"]) {
        let app = m
            .split("Killed process")
            .nth(1)
            .and_then(|r| r.split_once('(').and_then(|(_, x)| x.split(')').next()))
            .or_else(|| m.split("task=").nth(1).and_then(|r| r.split(',').next()))
            .unwrap_or("")
            .to_string();
        return Some((
            IssueKind::MemoryExhaustion,
            app,
            "The system ran out of memory and ended a program".into(),
        ));
    }
    if has(&["I/O error", "EXT4-fs error", "BTRFS error"]) || (m.contains("XFS") && has(&["corruption", "error"])) {
        return Some((
            IssueKind::DiskError,
            String::new(),
            "A disk or file system reported an error".into(),
        ));
    }
    if has(&["Machine check", "Hardware Error"]) {
        return Some((IssueKind::MemoryError, String::new(), "The hardware reported an error".into()));
    }
    if has(&["temperature above threshold", "thermal throttling"]) {
        return Some((
            IssueKind::Overheating,
            String::new(),
            "The processor got hot and slowed down".into(),
        ));
    }
    if m.contains("uncleanly shut down") {
        return Some((IssueKind::BadShutdown, String::new(), "The system did not shut down cleanly".into()));
    }
    if has(&["NVRM: Xid", "GPU HANG", "timeout"]) && has(&["NVRM: Xid", "GPU HANG", "ring "]) {
        return Some((
            IssueKind::DriverFailure,
            String::new(),
            "The graphics driver had to be reset".into(),
        ));
    }
    if has(&["Failed to start", "Failed with result", "Main process exited, code="]) {
        let app = if unit.is_empty() { unit_in_message(m) } else { unit.to_string() };
        let title = if app.is_empty() {
            "A service failed".to_string()
        } else {
            format!("{app} failed")
        };
        return Some((IssueKind::ServiceFailure, app, title));
    }
    None
}

fn unit_in_message(m: &str) -> String {
    if let Some(r) = m.strip_prefix("Failed to start ") {
        return r.trim_end_matches('.').to_string();
    }
    m.split_whitespace()
        .find(|w| w.ends_with(".service") || w.ends_with(".service:"))
        .map(|w| w.trim_end_matches(':').to_string())
        .unwrap_or_default()
}

/// One `journalctl -o json` line as an issue. Pure.
pub fn journal_issue(line: &str) -> Option<Issue> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    // MESSAGE is an array of bytes when the entry is not valid text; those are skipped.
    let message = v.get("MESSAGE")?.as_str()?;
    let micros: i64 = v.get("__REALTIME_TIMESTAMP")?.as_str()?.parse().ok()?;
    let unit = v
        .get("UNIT")
        .or_else(|| v.get("_SYSTEMD_UNIT"))
        .and_then(|u| u.as_str())
        .unwrap_or("");
    let (kind, app, title) = classify(message, unit)?;
    let cursor = v.get("__CURSOR").and_then(|c| c.as_str()).unwrap_or("");
    Some(Issue::new(
        IssueSourceId::Journal,
        Timestamp(micros / 1000),
        kind,
        &app,
        &title,
        message,
        cursor,
    ))
}

/// The systemd journal.
pub struct Journal {
    runner: Box<dyn ToolRunner>,
}

impl Journal {
    /// A reader running `journalctl` through `runner`.
    pub fn new(runner: Box<dyn ToolRunner>) -> Journal {
        Journal { runner }
    }
}

impl IssueSource for Journal {
    fn id(&self) -> IssueSourceId {
        IssueSourceId::Journal
    }

    fn read(&self, since: Timestamp, until: Timestamp) -> Result<Vec<Issue>, PlatformError> {
        let hours = hours_back(since, until).min(24 * 14);
        let args: Vec<String> = [
            "--no-pager",
            "-o",
            "json",
            "-n",
            "2000",
            &format!("--since=-{hours}h"),
            &format!("--grep={JOURNAL_PATTERN}"),
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
        let text = match self.runner.run("journalctl", &args, timeout_for(hours)) {
            Err(PlatformError::Failed(m)) if m.contains("not seeing messages from other users") || m.contains("No journal files") => {
                return Err(PlatformError::AccessDenied(
                    "Your account may not read the system journal. Add it to the systemd-journal group (log out and in again), then read again.".into(),
                ))
            }
            other => other?,
        };
        let mut out: Vec<Issue> = text
            .lines()
            .filter_map(journal_issue)
            .filter(|i| i.at >= since && i.at <= until)
            .collect();
        out.sort_by(|a, b| b.at.cmp(&a.at).then(a.id.cmp(&b.id)));
        out.dedup_by(|a, b| a.id == b.id);
        out.truncate(MAX_PER_READ);
        Ok(out)
    }
}

/// The signal's name for the numbers `coredumpctl` reports.
pub fn signal_name(n: i64) -> String {
    match n {
        4 => "SIGILL".into(),
        6 => "SIGABRT".into(),
        7 => "SIGBUS".into(),
        8 => "SIGFPE".into(),
        11 => "SIGSEGV".into(),
        n => format!("signal {n}"),
    }
}

/// `coredumpctl --json=short list` output as issues. Pure; a malformed entry is skipped.
pub fn coredump_issues(text: &str) -> Vec<Issue> {
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(text.trim()) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|v| {
            let micros = v
                .get("time")
                .and_then(|t| t.as_i64().or_else(|| t.as_str().and_then(|s| s.parse().ok())))?;
            let exe = v.get("exe").and_then(|e| e.as_str()).unwrap_or("");
            let comm = v.get("comm").and_then(|e| e.as_str()).unwrap_or("");
            let app = if exe.is_empty() { comm } else { file_name(exe) };
            let sig = v.get("sig").and_then(|s| s.as_i64()).map(signal_name).unwrap_or_default();
            let pid = v.get("pid").and_then(|p| p.as_i64()).unwrap_or(0);
            let kind = kind_for_signal(v.get("sig").and_then(|s| s.as_i64()).map(|n| n.to_string()).as_deref());
            let who = if app.is_empty() { "A program" } else { app };
            let title = if kind == IssueKind::StoppedFromOutside {
                format!("{who} was stopped from outside")
            } else {
                format!("{who} crashed")
            };
            let evidence = format!("{sig}{}{}", if exe.is_empty() { "" } else { " in " }, exe);
            Some(Issue::new(
                IssueSourceId::Coredumps,
                Timestamp(micros / 1000),
                kind,
                app,
                &title,
                evidence.trim(),
                &pid.to_string(),
            ))
        })
        .collect()
}

/// Crashes with a core dump, through `coredumpctl`.
pub struct Coredumps {
    runner: Box<dyn ToolRunner>,
}

impl Coredumps {
    /// A reader running `coredumpctl` through `runner`.
    pub fn new(runner: Box<dyn ToolRunner>) -> Coredumps {
        Coredumps { runner }
    }
}

impl IssueSource for Coredumps {
    fn id(&self) -> IssueSourceId {
        IssueSourceId::Coredumps
    }

    fn read(&self, since: Timestamp, until: Timestamp) -> Result<Vec<Issue>, PlatformError> {
        let hours = hours_back(since, until).min(24 * 30);
        let args: Vec<String> = ["--no-pager", "--json=short", &format!("--since=-{hours}h"), "list"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        let text = match self.runner.run("coredumpctl", &args, timeout_for(hours)) {
            // coredumpctl exits 1 and says so when there is nothing to list.
            Err(PlatformError::Failed(m)) if m.contains("No coredumps found") => return Ok(Vec::new()),
            other => other?,
        };
        let mut out: Vec<Issue> = coredump_issues(&text)
            .into_iter()
            .filter(|i| i.at >= since && i.at <= until)
            .collect();
        out.sort_by(|a, b| b.at.cmp(&a.at).then(a.id.cmp(&b.id)));
        out.truncate(MAX_PER_READ);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::Scripted;
    use super::*;

    fn entry(msg: &str, micros: i64, unit: Option<&str>) -> String {
        let mut v = serde_json::json!({"MESSAGE": msg, "__REALTIME_TIMESTAMP": micros.to_string(), "__CURSOR": format!("c{micros}")});
        if let Some(u) = unit {
            v["UNIT"] = u.into();
        }
        v.to_string()
    }

    #[test]
    fn journal_messages_are_classified() {
        let c = |m: &str| classify(m, "");
        let oom = c("Out of memory: Killed process 4242 (firefox) total-vm:1kB").unwrap();
        assert_eq!((oom.0, oom.1.as_str()), (IssueKind::MemoryExhaustion, "firefox"));
        let cg = c("oom-kill:constraint=CONSTRAINT_NONE,nodemask=(null),cpuset=/,task=java,pid=1").unwrap();
        assert_eq!(cg.1, "java");
        assert_eq!(
            c("blk_update_request: I/O error, dev sda, sector 1").unwrap().0,
            IssueKind::DiskError
        );
        assert_eq!(c("EXT4-fs error (device sda1): ext4_lookup").unwrap().0, IssueKind::DiskError);
        assert_eq!(c("XFS (sdb1): Metadata corruption detected").unwrap().0, IssueKind::DiskError);
        assert_eq!(
            c("mce: [Hardware Error]: Machine check events logged").unwrap().0,
            IssueKind::MemoryError
        );
        assert_eq!(
            c("CPU0: Core temperature above threshold, cpu clock throttled").unwrap().0,
            IssueKind::Overheating
        );
        assert_eq!(
            c("systemd-journald[1]: File /var/log/journal/x/system.journal corrupted or uncleanly shut down, renaming")
                .unwrap()
                .0,
            IssueKind::BadShutdown
        );
        assert_eq!(
            c("NVRM: Xid (PCI:0000:01:00): 79, GPU has fallen off the bus").unwrap().0,
            IssueKind::DriverFailure
        );
        assert_eq!(
            c("amdgpu 0000:03:00.0: ring gfx timeout, signaled seq=1").unwrap().0,
            IssueKind::DriverFailure
        );
        let svc = classify("foo.service: Failed with result 'exit-code'.", "foo.service").unwrap();
        assert_eq!(
            (svc.0, svc.1.as_str(), svc.2.as_str()),
            (IssueKind::ServiceFailure, "foo.service", "foo.service failed")
        );
        assert_eq!(c("Failed to start Foo Daemon.").unwrap().1, "Foo Daemon");
        assert_eq!(
            c("bar.service: Main process exited, code=dumped, status=11/SEGV").unwrap().1,
            "bar.service"
        );
        assert_eq!(c("Failed with result").unwrap().2, "A service failed");
        assert!(c("everything is fine").is_none());
        assert!(c("a timeout somewhere").is_none());
    }

    #[test]
    fn journal_lines() {
        let i = journal_issue(&entry("blk_update_request: I/O error, dev sda", 1_761_127_872_123_456, None)).unwrap();
        assert_eq!(i.at, Timestamp(1_761_127_872_123));
        assert_eq!(i.source, IssueSourceId::Journal);
        assert!(journal_issue(r#"{"MESSAGE":[1,2,3],"__REALTIME_TIMESTAMP":"1"}"#).is_none());
        assert!(journal_issue(r#"{"MESSAGE":"I/O error"}"#).is_none());
        assert!(journal_issue("nope").is_none());
        let unit = journal_issue(&entry("x.service: Failed with result 'exit-code'.", 5_000, Some("x.service"))).unwrap();
        assert_eq!(unit.app, "x.service");
    }

    #[test]
    fn coredumps() {
        let json = r#"[{"time":1761127872123456,"pid":77,"uid":1000,"gid":1000,"sig":11,"corefile":"present","exe":"/usr/bin/foo","size":1},{"time":"1761127872999000","sig":6,"comm":"bar"},{"nope":1}]"#;
        let list = coredump_issues(json);
        assert_eq!(list.len(), 2);
        assert_eq!((list[0].app.as_str(), list[0].kind), ("foo", IssueKind::AppCrash));
        assert_eq!(list[0].evidence, "SIGSEGV in /usr/bin/foo");
        assert_eq!(list[1].app, "bar");
        assert_eq!(list[1].evidence, "SIGABRT");
        assert!(coredump_issues("not json").is_empty());
        assert!(coredump_issues("{}").is_empty());
        // A core dump from a signal others send is not a crash; the numbers coredumpctl reports are the signal's.
        let ended =
            coredump_issues(r#"[{"time":5000000,"pid":9,"sig":15,"exe":"/usr/bin/foo"},{"time":6000000,"pid":10,"sig":9,"comm":"bar"}]"#);
        assert!(ended.iter().all(|i| i.kind == IssueKind::StoppedFromOutside), "{ended:?}");
        assert_eq!(ended[0].title, "foo was stopped from outside");
        assert_eq!(signal_name(7), "SIGBUS");
        assert_eq!(signal_name(99), "signal 99");
        assert_eq!(signal_name(4), "SIGILL");
        assert_eq!(signal_name(8), "SIGFPE");
    }

    fn shared(answers: Vec<Result<String, PlatformError>>) -> (Box<dyn ToolRunner>, std::sync::Arc<Scripted>) {
        let s = std::sync::Arc::new(Scripted::new(answers));
        struct Shared(std::sync::Arc<Scripted>);
        impl ToolRunner for Shared {
            fn run_tool(&self, tool: &crate::runner::Tool, t: std::time::Duration) -> Result<crate::runner::ToolOutput, PlatformError> {
                self.0.run_tool(tool, t)
            }
        }
        (Box::new(Shared(s.clone())), s)
    }

    #[test]
    fn the_journal_reader_filters_by_the_window_and_explains_missing_rights() {
        let t = Timestamp(1_761_127_872_123);
        let line = entry("blk_update_request: I/O error, dev sda", 1_761_127_872_123_456, None);
        let (r, calls) = shared(vec![
            Ok(format!("{line}\n{line}\n")),
            Ok(line.clone()),
            Err(PlatformError::Failed(
                "Hint: You are currently not seeing messages from other users and the system.".into(),
            )),
            Err(PlatformError::Unsupported("journalctl is not available on this computer.".into())),
        ]);
        let j = Journal::new(r);
        assert_eq!(j.read(t.minus(3_000_000), t.plus(1000)).unwrap().len(), 1);
        let args = calls.calls.lock().unwrap()[0].1.clone();
        assert!(
            args.iter().any(|a| a.starts_with("--grep=")) && args.contains(&"--since=-1h".to_string()),
            "{args:?}"
        );
        assert!(j.read(t.plus(10_000), t.plus(20_000)).unwrap().is_empty());
        let e = j.read(t.minus(1000), t).unwrap_err();
        assert!(matches!(&e, PlatformError::AccessDenied(m) if m.contains("systemd-journal")), "{e}");
        assert!(matches!(j.read(t.minus(1000), t).unwrap_err(), PlatformError::Unsupported(_)));
        assert_eq!(j.id(), IssueSourceId::Journal);
    }

    #[test]
    fn the_coredump_reader_treats_an_empty_list_as_empty() {
        let t = Timestamp(1_761_127_872_123);
        let json = r#"[{"time":1761127872123456,"pid":77,"sig":11,"exe":"/usr/bin/foo"}]"#;
        let (r, _) = shared(vec![
            Ok(json.into()),
            Err(PlatformError::Failed("No coredumps found.".into())),
            Err(PlatformError::Unsupported("coredumpctl is not available on this computer.".into())),
        ]);
        let c = Coredumps::new(r);
        assert_eq!(c.read(t.minus(1000), t.plus(1000)).unwrap().len(), 1);
        assert!(c.read(t.minus(1000), t).unwrap().is_empty());
        assert!(c.read(t.minus(1000), t).is_err());
        assert_eq!(c.id(), IssueSourceId::Coredumps);
    }
}
