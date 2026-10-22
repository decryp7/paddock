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
    fn never_panics_on_odd_input() {
        for odd in ["", "msg=", "msg=\"", "msg=\"\\", "\u{0}\u{ff}", "msg=\"é\\\u{1F600}"] {
            let _ = classify(odd);
        }
        let huge = format!("{}é{}", "a".repeat(MAX_LINE_LEN - 1), "fetching a task timed out");
        assert_eq!(classify(&huge), RunnerLogEvent::Other);
    }
}
