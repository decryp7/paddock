//! Chrome's own network log for the real-browser tests: a failed render's assertion message ends with the log's tail, so a
//! cause that only shows on a CI runner (a proxy lookup, a cancelled request, a reset connection) can be read off the
//! failure instead of guessed at.

use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Entries [`NetLog::tail`] keeps.
const TAIL: usize = 40;
/// The longest parameter text one entry shows.
const PARAMS: usize = 240;
/// One browser's net log, written into its own temp folder (`--log-net-log`), never into the profile folder the tests check.
pub struct NetLog {
    dir: tempfile::TempDir,
}

impl NetLog {
    pub fn new() -> NetLog {
        NetLog {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn file(&self) -> PathBuf {
        self.dir.path().join("net-log.json")
    }

    /// The browser flags that write the log.
    pub fn flags(&self) -> Vec<String> {
        vec![
            format!("--log-net-log={}", self.file().display()),
            "--net-log-capture-mode=Default".into(),
        ]
    }

    /// The last telling events of the log (event type, phase, source, parameters), or why there are none. Read once the
    /// browser has ended: Chrome closes the JSON when it quits, and an unfinished log is closed here before parsing.
    pub fn tail(&self) -> String {
        let Ok(text) = std::fs::read_to_string(self.file()) else {
            return "net log: none written".into();
        };
        let log: Value = serde_json::from_str(&text)
            .or_else(|_| serde_json::from_str(&format!("{}]}}", text.trim_end().trim_end_matches(','))))
            .unwrap_or(Value::Null);
        summarise(&log).unwrap_or_else(|| {
            let lines: Vec<&str> = text.lines().collect();
            format!(
                "net log (unreadable), last lines:\n{}",
                lines[lines.len().saturating_sub(10)..].join("\n")
            )
        })
    }
}

impl NetLog {
    /// The addresses the browser opened a TCP connection to (`TCP_CONNECT` attempts), read once it has ended.
    pub fn connected_addresses(&self) -> Vec<String> {
        let text = std::fs::read_to_string(self.file()).unwrap_or_default();
        let log: Value = serde_json::from_str(&text)
            .or_else(|_| serde_json::from_str(&format!("{}]}}", text.trim_end().trim_end_matches(','))))
            .unwrap_or(Value::Null);
        let types = names(&log, "logEventTypes");
        let connect = types.iter().find(|(_, n)| n.as_str() == "TCP_CONNECT_ATTEMPT").map(|(k, _)| *k);
        log.get("events")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|e| e.get("type").and_then(Value::as_u64) == connect)
            .filter_map(|e| e.pointer("/params/address").and_then(Value::as_str).map(str::to_string))
            .collect()
    }
}

fn names(log: &Value, table: &str) -> HashMap<u64, String> {
    log.pointer(&format!("/constants/{table}"))
        .and_then(Value::as_object)
        .map(|m| m.iter().filter_map(|(k, v)| Some((v.as_u64()?, k.clone()))).collect())
        .unwrap_or_default()
}

/// Every event of a main-frame request (a page load), and any other event that is a cancellation or carries a network
/// error, oldest first; the last [`TAIL`] of them.
fn summarise(log: &Value) -> Option<String> {
    let events = log.get("events")?.as_array()?;
    let types = names(log, "logEventTypes");
    let sources = names(log, "logSourceType");
    let num = |e: &Value, p: &str| e.pointer(p).and_then(Value::as_u64).unwrap_or(u64::MAX);
    let ty = |e: &Value| types.get(&num(e, "/type")).cloned().unwrap_or_else(|| num(e, "/type").to_string());
    let loads: HashSet<u64> = events
        .iter()
        .filter(|e| e.pointer("/params/request_type").and_then(Value::as_str) == Some("main frame"))
        .map(|e| num(e, "/source/id"))
        .collect();
    let lines: Vec<String> = events
        .iter()
        .filter(|e| {
            let failed = e.pointer("/params/net_error").and_then(Value::as_i64).is_some_and(|n| n != 0);
            loads.contains(&num(e, "/source/id")) || failed || ty(e) == "CANCELLED"
        })
        .map(|e| {
            let phase = match num(e, "/phase") {
                1 => "begin",
                2 => "end",
                _ => "",
            };
            let source = sources.get(&num(e, "/source/type")).cloned().unwrap_or_default();
            let params: String = e
                .get("params")
                .map(Value::to_string)
                .unwrap_or_default()
                .chars()
                .take(PARAMS)
                .collect();
            format!("  {} {phase} [{source} {}] {params}", ty(e), num(e, "/source/id"))
        })
        .collect();
    let skip = lines.len().saturating_sub(TAIL);
    Some(format!(
        "net log ({} events; page loads, cancellations and errors, last {}):\n{}",
        events.len(),
        lines.len() - skip,
        lines[skip..].join("\n")
    ))
}
