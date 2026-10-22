//! Classifying the runner's log lines into the events the watchdog and the health view care about.
//!
//! The Gitea and Forgejo runners log with logrus' text format:
//! `time="2026-10-22T06:57:19+08:00" level=info msg="Starting runner daemon"`. Pitcrew only reads the message and
//! the level; everything else on the line stays in the log file and the searchable log view.

use serde::{Deserialize, Serialize};

/// Longest line Pitcrew classifies; longer lines are cut there (they are still kept in the log file).
pub const MAX_LINE_LEN: usize = 64 * 1024;

/// What a runner log line means for supervision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RunnerLogEvent {
    /// The daemon is starting.
    DaemonStarting,
    /// The runner declared itself to the server (it is registered and the server accepted it).
    Declared,
    /// A poll for a task timed out: the stall signal of the PowerShell watchdog Pitcrew replaces.
    FetchTimedOut,
    /// A poll for a task failed for another reason (server unreachable, refused, …).
    FetchFailed,
    /// The runner started a job.
    TaskStarted,
    /// The runner is shutting down (it waits for running jobs first).
    ShuttingDown,
    /// The server does not know this runner (it must be registered again).
    NotRegistered,
    /// Anything else.
    Other,
}

/// The `msg` of a logrus text line (unescaping `\"`), or the whole line when it has none.
pub fn message(line: &str) -> String {
    let line = cut(line);
    let Some(start) = line.find("msg=") else {
        return line.trim().to_string();
    };
    let rest = &line[start + 4..];
    if let Some(quoted) = rest.strip_prefix('"') {
        let mut out = String::new();
        let mut chars = quoted.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    if let Some(n) = chars.next() {
                        out.push(n);
                    }
                }
                '"' => break,
                _ => out.push(c),
            }
        }
        out
    } else {
        rest.split_whitespace().next().unwrap_or("").to_string()
    }
}

/// Classifies one line.
pub fn classify(line: &str) -> RunnerLogEvent {
    let line = cut(line);
    let msg = message(line).to_ascii_lowercase();
    let whole = line.to_ascii_lowercase();
    if msg.contains("fetching a task timed out")
        || (msg.contains("failed to fetch task")
            && (whole.contains("deadline exceeded") || whole.contains("timed out") || whole.contains("timeout")))
    {
        RunnerLogEvent::FetchTimedOut
    } else if msg.contains("failed to fetch task") {
        RunnerLogEvent::FetchFailed
    } else if msg.contains("declare successfully") {
        RunnerLogEvent::Declared
    } else if msg.starts_with("starting runner daemon") {
        RunnerLogEvent::DaemonStarting
    } else if msg == "task started" || msg.starts_with("task started ") {
        RunnerLogEvent::TaskStarted
    } else if msg.contains("shutdown initiated") {
        RunnerLogEvent::ShuttingDown
    } else if msg.contains("unregistered runner") || msg.contains("runner not found") || msg.contains("registration file not found") {
        RunnerLogEvent::NotRegistered
    } else {
        RunnerLogEvent::Other
    }
}

/// The labels in a "declare successfully" line (`..., with labels: [macos-arm64 linux], declare successfully`), as
/// the runner printed them (names only: the runner declares `name:host` labels by `name`). `None` for other lines.
pub fn declared_labels(line: &str) -> Option<Vec<String>> {
    let msg = message(line);
    if !msg.contains("declare successfully") {
        return None;
    }
    let rest = msg.split_once("with labels:")?.1;
    let inner = rest.trim_start().strip_prefix('[')?.split_once(']')?.0;
    Some(
        inner
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .take(crate::config::MAX_LABELS * 2)
            .map(|s| s.chars().take(crate::config::MAX_LABEL_LEN).collect())
            .collect(),
    )
}

/// Most `key=value` fields [`fields`] returns from one line.
pub const MAX_FIELDS: usize = 64;

/// The `key=value` fields of a logrus text line, in order (quoted values unescaped), e.g. the `job`,
/// `repository` and `run_id` of a "task started" line. Bounded by [`MAX_FIELDS`] and [`MAX_LINE_LEN`].
pub fn fields(line: &str) -> Vec<(String, String)> {
    let mut out = vec![];
    let mut rest = cut(line).trim_start();
    while !rest.is_empty() && out.len() < MAX_FIELDS {
        let key_end = rest.find(|c: char| c == '=' || c.is_whitespace()).unwrap_or(rest.len());
        let key = &rest[..key_end];
        rest = &rest[key_end..];
        let Some(after_eq) = rest.strip_prefix('=') else {
            rest = rest.trim_start();
            continue;
        };
        let (value, remaining) = if let Some(quoted) = after_eq.strip_prefix('"') {
            let mut value = String::new();
            let mut end = quoted.len();
            let mut chars = quoted.char_indices();
            while let Some((i, c)) = chars.next() {
                match c {
                    '\\' => {
                        if let Some((_, n)) = chars.next() {
                            value.push(n);
                        }
                    }
                    '"' => {
                        end = i + 1;
                        break;
                    }
                    _ => value.push(c),
                }
            }
            (value, &quoted[end.min(quoted.len())..])
        } else {
            let end = after_eq.find(char::is_whitespace).unwrap_or(after_eq.len());
            (after_eq[..end].to_string(), &after_eq[end..])
        };
        if !key.is_empty() {
            out.push((key.to_string(), value));
        }
        rest = remaining.trim_start();
    }
    out
}

fn cut(line: &str) -> &str {
    if line.len() <= MAX_LINE_LEN {
        return line;
    }
    let mut end = MAX_LINE_LEN;
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    &line[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(msg: &str) -> String {
        format!(r#"time="2026-10-22T06:57:19+08:00" level=info msg="{msg}""#)
    }

    #[test]
    fn extracts_messages() {
        assert_eq!(message(&line("Starting runner daemon")), "Starting runner daemon");
        assert_eq!(message(r#"level=error msg="say \"hi\"" error=x"#), r#"say "hi""#);
        assert_eq!(message("level=info msg=bare other=1"), "bare");
        assert_eq!(message("  plain text  "), "plain text");
    }

    #[test]
    fn the_declared_labels_are_read_from_the_declare_line() {
        let l = line("runner: build-pc, with version: v5.2.0, with labels: [windows-latest ubuntu:host], declare successfully");
        assert_eq!(declared_labels(&l), Some(vec!["windows-latest".into(), "ubuntu:host".into()]));
        assert_eq!(
            declared_labels(&line("runner: x, with labels: [], declare successfully")),
            Some(vec![])
        );
        assert_eq!(
            declared_labels(&line("runner: x, with labels: [a, b], declare successfully")),
            Some(vec!["a".into(), "b".into()])
        );
        assert_eq!(declared_labels(&line("runner: x, with labels: [a], something else")), None);
        assert_eq!(declared_labels(&line("declare successfully")), None);
        assert_eq!(declared_labels(&line("runner: x, with labels: a, declare successfully")), None);
        assert_eq!(declared_labels(&line("Starting runner daemon")), None);
    }

    #[test]
    fn classifies_the_lines_supervision_needs() {
        assert_eq!(classify(&line("Starting runner daemon")), RunnerLogEvent::DaemonStarting);
        assert_eq!(
            classify(&line(
                "runner: build-pc, with version: v5.2.0, with labels: [windows-latest], declare successfully"
            )),
            RunnerLogEvent::Declared
        );
        assert_eq!(classify(&line("fetching a task timed out")), RunnerLogEvent::FetchTimedOut);
        assert_eq!(
            classify(r#"level=error msg="failed to fetch task" error="deadline_exceeded: context deadline exceeded""#),
            RunnerLogEvent::FetchTimedOut
        );
        assert_eq!(
            classify(r#"level=error msg="failed to fetch task" error="unavailable: dial tcp 192.0.2.1:443: connection refused""#),
            RunnerLogEvent::FetchFailed
        );
        assert_eq!(classify(&line("task started")), RunnerLogEvent::TaskStarted);
        assert_eq!(
            classify(&line("runner: build-pc shutdown initiated, waiting 0s for running jobs")),
            RunnerLogEvent::ShuttingDown
        );
        assert_eq!(classify(&line("unregistered runner")), RunnerLogEvent::NotRegistered);
        assert_eq!(classify(&line("Running job with maxParallel=1")), RunnerLogEvent::Other);
    }

    #[test]
    fn reads_the_fields_of_a_task_started_line() {
        let l = r#"time="2026-10-22T13:36:25+08:00" level=info msg="task started" actions_url="https://github.com" address="https://git.example.com" job=tauri-smoke job_id=8288 repository=owner/repo run_id=5983 run_number=215 task_id=8271"#;
        let f = fields(l);
        let get = |k: &str| f.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
        assert_eq!(get("msg"), Some("task started"));
        assert_eq!(get("address"), Some("https://git.example.com"));
        assert_eq!(get("job"), Some("tauri-smoke"));
        assert_eq!(get("run_number"), Some("215"));
        assert_eq!(get("task_id"), Some("8271"));
        assert_eq!(
            fields(r#"a="x \"y\"" lone b= c"#),
            vec![("a".into(), r#"x "y""#.into()), ("b".into(), String::new())]
        );
        assert_eq!(fields(r#"=v k="unterminated"#), vec![("k".into(), "unterminated".into())]);
        assert!(fields(&"k=v ".repeat(500)).len() <= MAX_FIELDS);
        assert!(fields("").is_empty());
    }

    #[test]
    fn never_panics_on_odd_input() {
        for odd in ["", "msg=", "msg=\"", "msg=\"\\", "\u{0}\u{ff}", "msg=\"é\\\u{1F600}", "a=\"é\\"] {
            let _ = classify(odd);
            let _ = fields(odd);
        }
        let huge = format!("{}é{}", "a".repeat(MAX_LINE_LEN - 1), "fetching a task timed out");
        assert_eq!(classify(&huge), RunnerLogEvent::Other);
    }
}
