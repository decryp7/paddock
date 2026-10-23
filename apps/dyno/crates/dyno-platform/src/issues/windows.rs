//! Windows: the Event Log through `wevtutil` (the OS's own front end to the Event Log API), with one curated query per
//! channel, rendered XML parsed here. Observe-only; the Application, System and Windows Update channels are readable
//! by a standard user, the Security log is never touched. The table of events is the single source of truth for both
//! the query and the classification.

use super::{timeout_for, ToolRunner};
use dyno_core::issues::{Issue, IssueKind, IssueSource, IssueSourceId, MAX_PER_READ};
use dyno_core::ports::PlatformError;
use dyno_core::time::Timestamp;

/// Most events asked for per channel.
const MAX_EVENTS: u32 = 300;

/// One curated event: where it is logged, who logs it, which ids, and what it means.
pub struct Rule {
    /// The channel (log) name.
    pub channel: &'static str,
    /// The event provider's name.
    pub provider: &'static str,
    /// The event ids.
    pub ids: &'static [u32],
    /// What it means.
    pub kind: IssueKind,
    /// The title; `{}` is replaced by the app or device name.
    pub title: &'static str,
}

/// The curated events (Windows documents these in the Event Log reference; the ids are stable across versions).
pub const RULES: &[Rule] = &[
    Rule {
        channel: "Application",
        provider: "Application Error",
        ids: &[1000],
        kind: IssueKind::AppCrash,
        title: "{} stopped working",
    },
    Rule {
        channel: "Application",
        provider: "Application Hang",
        ids: &[1002],
        kind: IssueKind::AppHang,
        title: "{} stopped responding",
    },
    Rule {
        channel: "System",
        provider: "Microsoft-Windows-Kernel-Power",
        ids: &[41],
        kind: IssueKind::BadShutdown,
        title: "The PC restarted without shutting down cleanly",
    },
    Rule {
        channel: "System",
        provider: "EventLog",
        ids: &[6008],
        kind: IssueKind::BadShutdown,
        title: "The previous shutdown was unexpected",
    },
    Rule {
        channel: "System",
        provider: "Service Control Manager",
        ids: &[7031, 7034],
        kind: IssueKind::ServiceFailure,
        title: "The {} service stopped unexpectedly",
    },
    Rule {
        channel: "System",
        provider: "Service Control Manager",
        ids: &[7000, 7001, 7009, 7011],
        kind: IssueKind::ServiceFailure,
        title: "The {} service did not start",
    },
    Rule {
        channel: "System",
        provider: "disk",
        ids: &[7, 11, 51, 153],
        kind: IssueKind::DiskError,
        title: "A disk reported an error",
    },
    Rule {
        channel: "System",
        provider: "Ntfs",
        ids: &[55, 98],
        kind: IssueKind::DiskError,
        title: "The file system reported a problem",
    },
    Rule {
        channel: "System",
        provider: "Microsoft-Windows-WHEA-Logger",
        ids: &[1, 17, 18, 19, 47],
        kind: IssueKind::MemoryError,
        title: "The hardware reported an error",
    },
    Rule {
        channel: "System",
        provider: "Microsoft-Windows-Resource-Exhaustion-Detector",
        ids: &[2004],
        kind: IssueKind::MemoryExhaustion,
        title: "Windows ran low on memory",
    },
    Rule {
        channel: "System",
        provider: "Display",
        ids: &[4101],
        kind: IssueKind::DriverFailure,
        title: "The display driver stopped responding and recovered",
    },
    Rule {
        channel: "System",
        provider: "Microsoft-Windows-Kernel-PnP",
        ids: &[219],
        kind: IssueKind::DriverFailure,
        title: "A driver could not be loaded",
    },
    Rule {
        channel: "Microsoft-Windows-WindowsUpdateClient/Operational",
        provider: "Microsoft-Windows-WindowsUpdateClient",
        ids: &[20],
        kind: IssueKind::UpdateFailure,
        title: "A Windows update failed to install",
    },
];

/// The channels the rules use, once each.
pub fn channels() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for r in RULES {
        if !out.contains(&r.channel) {
            out.push(r.channel);
        }
    }
    out
}

/// The XPath query for one channel's events in the last `ms` milliseconds.
pub fn query(channel: &str, ms: i64) -> String {
    let clauses: Vec<String> = RULES
        .iter()
        .filter(|r| r.channel == channel)
        .map(|r| {
            let ids: Vec<String> = r.ids.iter().map(|i| format!("EventID={i}")).collect();
            format!("(Provider[@Name='{}'] and ({}))", r.provider, ids.join(" or "))
        })
        .collect();
    format!(
        "*[System[({}) and TimeCreated[timediff(@SystemTime) <= {}]]]",
        clauses.join(" or "),
        ms.max(1)
    )
}

/// The Windows Event Log.
pub struct WindowsEvents {
    runner: Box<dyn ToolRunner>,
}

impl WindowsEvents {
    /// A reader running `wevtutil` through `runner`.
    pub fn new(runner: Box<dyn ToolRunner>) -> WindowsEvents {
        WindowsEvents { runner }
    }
}

impl IssueSource for WindowsEvents {
    fn id(&self) -> IssueSourceId {
        IssueSourceId::WindowsEvents
    }

    fn read(&self, since: Timestamp, until: Timestamp) -> Result<Vec<Issue>, PlatformError> {
        let ms = until.since(since).max(1) + 60_000;
        let hours = super::hours_back(since, until);
        let mut out = Vec::new();
        let mut failure: Option<PlatformError> = None;
        let mut readable = 0;
        for channel in channels() {
            let args = vec![
                "qe".to_string(),
                channel.to_string(),
                format!("/q:{}", query(channel, ms)),
                format!("/c:{MAX_EVENTS}"),
                "/rd:true".into(),
                "/f:RenderedXml".into(),
            ];
            match self.runner.run("wevtutil.exe", &args, timeout_for(hours)) {
                Ok(text) => {
                    readable += 1;
                    out.extend(parse_events(&text).into_iter().filter(|i| i.at >= since && i.at <= until));
                }
                // The update channel is absent on some editions; that is not a failure of the whole source.
                Err(PlatformError::Failed(m)) if m.contains("not found") || m.contains("0x3ff") => readable += 1,
                Err(PlatformError::Failed(m)) if m.contains("Access is denied") => {
                    failure.get_or_insert(PlatformError::AccessDenied(format!(
                        "Windows would not let Dyno read the {channel} log. Add this account to the Event Log Readers group."
                    )));
                }
                Err(e) => {
                    failure.get_or_insert(e);
                }
            }
        }
        if readable == 0 {
            return Err(failure.unwrap_or_else(|| PlatformError::Failed("No Windows event log could be read.".into())));
        }
        out.sort_by(|a, b| b.at.cmp(&a.at).then(a.id.cmp(&b.id)));
        out.dedup_by(|a, b| a.id == b.id);
        out.truncate(MAX_PER_READ);
        Ok(out)
    }
}

/// Every curated event in `wevtutil ... /f:RenderedXml` output. Pure; a malformed event is skipped.
pub fn parse_events(text: &str) -> Vec<Issue> {
    text.split("<Event ")
        .skip(1)
        .filter_map(|chunk| event_issue(chunk.split("</Event>").next().unwrap_or(chunk)))
        .collect()
}

fn event_issue(xml: &str) -> Option<Issue> {
    let provider = attr(xml, "<Provider", "Name")?;
    let id: u32 = element(xml, "EventID")?.trim().parse().ok()?;
    let at = super::parse_timestamp(&attr(xml, "<TimeCreated", "SystemTime")?)?;
    let record = element(xml, "EventRecordID").unwrap_or_default();
    let rule = RULES
        .iter()
        .find(|r| r.provider.eq_ignore_ascii_case(&provider) && r.ids.contains(&id))?;
    let data = event_data(xml);
    let message = element(xml, "Message").map(|m| unescape(&m)).unwrap_or_default();
    let app = match rule.kind {
        IssueKind::AppCrash | IssueKind::AppHang | IssueKind::ServiceFailure => data.first().cloned().unwrap_or_default(),
        _ => String::new(),
    };
    let app = app.trim_end_matches(".exe").to_string();
    let title = if app.is_empty() && rule.title.contains("{}") {
        rule.title.replace("{}", "A program").replace("The A program service", "A service")
    } else {
        rule.title.replace("{}", &app)
    };
    let evidence = if message.trim().is_empty() {
        let joined: Vec<&str> = data.iter().map(String::as_str).filter(|s| !s.is_empty()).take(6).collect();
        format!("Event {id} from {provider}: {}", joined.join(", "))
    } else {
        message
    };
    Some(Issue::new(
        IssueSourceId::WindowsEvents,
        at,
        rule.kind,
        &app,
        &title,
        &evidence,
        &format!("{provider}|{id}|{record}"),
    ))
}

/// The text of the first `<name ...>text</name>` element.
fn element(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}");
    let mut from = 0;
    while let Some(i) = xml[from..].find(&open) {
        let start = from + i + open.len();
        let after = xml[start..].chars().next()?;
        if after == '>' || after.is_whitespace() {
            let gt = xml[start..].find('>')? + start;
            if xml[..gt].ends_with('/') {
                return Some(String::new());
            }
            let end = xml[gt + 1..].find(&format!("</{name}>"))? + gt + 1;
            return Some(unescape(&xml[gt + 1..end]));
        }
        from = start;
    }
    None
}

/// The value of attribute `attr` on the first `tag` (single or double quotes).
fn attr(xml: &str, tag: &str, attr: &str) -> Option<String> {
    let i = xml.find(tag)?;
    let end = xml[i..].find('>')? + i;
    let head = &xml[i..end];
    for q in ['\'', '"'] {
        let key = format!("{attr}={q}");
        if let Some(k) = head.find(&key) {
            let v = &head[k + key.len()..];
            return Some(unescape(&v[..v.find(q)?]));
        }
    }
    None
}

/// The values of the `<Data>` elements inside `<EventData>`, in order.
fn event_data(xml: &str) -> Vec<String> {
    let Some(start) = xml.find("<EventData") else { return Vec::new() };
    let section = &xml[start..xml[start..].find("</EventData>").map_or(xml.len(), |e| start + e)];
    let mut out = Vec::new();
    let mut rest = section;
    while let Some(i) = rest.find("<Data") {
        let tail = &rest[i + 5..];
        let Some(gt) = tail.find('>') else { break };
        if tail[..gt].ends_with('/') {
            out.push(String::new());
            rest = &tail[gt + 1..];
            continue;
        }
        let Some(end) = tail[gt + 1..].find("</Data>") else { break };
        out.push(unescape(&tail[gt + 1..gt + 1 + end]));
        rest = &tail[gt + 1 + end + 7..];
    }
    out
}

/// The five XML entities and numeric character references.
pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let Some(semi) = tail.find(';').filter(|n| *n <= 10) else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let ent = &tail[1..semi];
        let ch = match ent {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => ent
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &tail[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::super::testing::Scripted;
    use super::*;
    use dyno_core::time::{DAY, HOUR};

    const CRASH: &str = r#"<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'><System><Provider Name='Application Error'/><EventID Qualifiers='0'>1000</EventID><Level>2</Level><TimeCreated SystemTime='2026-10-22T10:11:12.1234567Z'/><EventRecordID>4242</EventRecordID><Channel>Application</Channel></System><EventData><Data>chrome.exe</Data><Data>126.0.1</Data><Data>c0000005</Data></EventData><RenderingInfo Culture='en-US'><Message>Faulting application name: chrome.exe, version: 126.0.1&#xD;
Faulting module name: ntdll.dll &lt;x&gt;</Message></RenderingInfo></Event>"#;
    const SERVICE: &str = r#"<Event xmlns='x'><System><Provider Name='Service Control Manager' Guid='{555}'/><EventID Qualifiers='49152'>7034</EventID><TimeCreated SystemTime='2026-10-22T11:00:00.000Z'/><EventRecordID>9</EventRecordID></System><EventData><Data Name='param1'>Print Spooler</Data><Data Name='param2'>1</Data></EventData></Event>"#;
    const OTHER: &str = r#"<Event xmlns='x'><System><Provider Name='Some Provider'/><EventID>1000</EventID><TimeCreated SystemTime='2026-10-22T11:00:00.000Z'/></System></Event>"#;

    #[test]
    fn crashes_services_and_unknowns() {
        let issues = parse_events(&format!("{CRASH}\n{SERVICE}\n{OTHER}\n<Event garbage"));
        assert_eq!(issues.len(), 2);
        let c = &issues[0];
        assert_eq!((c.kind, c.app.as_str()), (IssueKind::AppCrash, "chrome"));
        assert_eq!(c.title, "chrome stopped working");
        assert!(c.evidence.contains("Faulting module name: ntdll.dll <x>"), "{}", c.evidence);
        assert_eq!(c.at, super::super::parse_timestamp("2026-10-22T10:11:12.123Z").unwrap());
        let s = &issues[1];
        assert_eq!((s.kind, s.app.as_str()), (IssueKind::ServiceFailure, "Print Spooler"));
        assert_eq!(s.title, "The Print Spooler service stopped unexpectedly");
        assert!(
            s.evidence.starts_with("Event 7034 from Service Control Manager: Print Spooler, 1"),
            "{}",
            s.evidence
        );
        assert!(parse_events("").is_empty());
    }

    #[test]
    fn events_without_a_name_get_a_general_title() {
        let xml = r#"<Event x><System><Provider Name='Service Control Manager'/><EventID>7000</EventID><TimeCreated SystemTime='2026-10-22T11:00:00Z'/></System></Event>"#;
        let i = &parse_events(xml)[0];
        assert_eq!(i.title, "A service did not start");
        let xml = r#"<Event x><System><Provider Name='Application Hang'/><EventID>1002</EventID><TimeCreated SystemTime='2026-10-22T11:00:00Z'/></System></Event>"#;
        assert_eq!(parse_events(xml)[0].title, "A program stopped responding");
    }

    #[test]
    fn the_same_record_has_the_same_id() {
        assert_eq!(parse_events(CRASH)[0].id, parse_events(CRASH)[0].id);
        assert_ne!(parse_events(CRASH)[0].id, parse_events(&CRASH.replace("4242", "4243"))[0].id);
    }

    #[test]
    fn queries_cover_every_rule_once_per_channel() {
        assert_eq!(
            channels(),
            vec!["Application", "System", "Microsoft-Windows-WindowsUpdateClient/Operational"]
        );
        let q = query("Application", 86_400_000);
        assert!(q.contains("Provider[@Name='Application Error'] and (EventID=1000)"));
        assert!(q.contains("timediff(@SystemTime) <= 86400000"));
        assert!(!q.contains("Kernel-Power"));
        let sys = query("System", 1);
        for r in RULES.iter().filter(|r| r.channel == "System") {
            assert!(sys.contains(r.provider), "{}", r.provider);
        }
        // Every kind the page lists except overheating is reachable on Windows.
        let kinds: std::collections::HashSet<_> = RULES.iter().map(|r| r.kind).collect();
        assert!(kinds.len() >= 8);
    }

    #[test]
    fn entities_unescape() {
        assert_eq!(
            unescape("a &lt;b&gt; &amp; &quot;c&quot; &apos;d&apos; &#65;&#x42;"),
            "a <b> & \"c\" 'd' AB"
        );
        assert_eq!(unescape("fish & chips; &unknown; &"), "fish & chips; &unknown; &");
        assert_eq!(unescape("plain"), "plain");
        assert_eq!(unescape("&#1114112;"), "&#1114112;");
    }

    fn reader(answers: Vec<Result<String, PlatformError>>) -> (WindowsEvents, std::sync::Arc<Scripted>) {
        let s = std::sync::Arc::new(Scripted::new(answers));
        struct Shared(std::sync::Arc<Scripted>);
        impl ToolRunner for Shared {
            fn run(&self, p: &str, a: &[String], t: std::time::Duration) -> Result<String, PlatformError> {
                self.0.run(p, a, t)
            }
        }
        (WindowsEvents::new(Box::new(Shared(s.clone()))), s)
    }

    #[test]
    fn a_read_queries_each_channel_and_keeps_events_in_the_window() {
        let t = super::super::parse_timestamp("2026-10-22T10:11:12Z").unwrap();
        let (r, calls) = reader(vec![Ok(CRASH.into()), Ok(SERVICE.into()), Ok(String::new())]);
        let got = r.read(t.minus(DAY), t.plus(HOUR)).unwrap();
        assert_eq!(got.len(), 2);
        assert!(got[0].at >= got[1].at, "newest first");
        let calls = calls.calls.lock().unwrap();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].0, "wevtutil.exe");
        assert!(calls[0].1.contains(&"/f:RenderedXml".to_string()) && calls[0].1.contains(&"/rd:true".to_string()));
        // A window that excludes them returns nothing.
        let (r, _) = reader(vec![Ok(CRASH.into()), Ok(SERVICE.into()), Ok(String::new())]);
        assert!(r.read(t.plus(DAY), t.plus(2 * DAY)).unwrap().is_empty());
    }

    #[test]
    fn a_missing_update_channel_is_not_a_failure_but_no_readable_channel_is() {
        let (r, _) = reader(vec![
            Ok(CRASH.into()),
            Ok(String::new()),
            Err(PlatformError::Failed(
                "wevtutil.exe failed: The specified channel could not be found. not found".into(),
            )),
        ]);
        let t = super::super::parse_timestamp("2026-10-22T10:11:12Z").unwrap();
        assert_eq!(r.read(t.minus(DAY), t.plus(HOUR)).unwrap().len(), 1);
        let (r, _) = reader(vec![
            Err(PlatformError::Failed("Access is denied.".into())),
            Err(PlatformError::Failed("Access is denied.".into())),
            Err(PlatformError::Failed("Access is denied.".into())),
        ]);
        let e = r.read(t.minus(DAY), t).unwrap_err();
        assert!(
            matches!(&e, PlatformError::AccessDenied(m) if m.contains("Event Log Readers")),
            "{e}"
        );
        let (r, _) = reader(vec![
            Err(PlatformError::Unsupported("wevtutil.exe is not available on this computer.".into())),
            Err(PlatformError::Timeout("x".into())),
            Err(PlatformError::Timeout("x".into())),
        ]);
        assert!(matches!(r.read(t.minus(DAY), t).unwrap_err(), PlatformError::Unsupported(_)));
        assert_eq!(r.id(), IssueSourceId::WindowsEvents);
    }
}
