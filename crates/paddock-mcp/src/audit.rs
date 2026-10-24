//! The audit log (ADR 0007 rule 3): time, client, tool, arguments (redacted), result size, duration, outcome. It holds no file
//! contents beyond what the arguments and a size say. The app shows it and lets the user clear it.

use paddock_log::Redact;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard};

/// How a call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    /// A result.
    Ok,
    /// The tool reported an error (including wrong arguments).
    ToolError,
    /// Refused before running: unknown or switched-off tool, too many calls in flight.
    Refused,
    /// The time limit passed.
    TimedOut,
    /// The tool panicked (contained).
    Panicked,
    /// The client cancelled the call (`notifications/cancelled`); it got no answer.
    Cancelled,
}

/// One call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    /// When (epoch milliseconds, the server clock's calendar time).
    pub at_ms: i64,
    /// Who asked.
    pub client: String,
    /// The tool.
    pub tool: String,
    /// The arguments, redacted.
    pub arguments: Value,
    /// Bytes of the result text.
    pub result_bytes: usize,
    /// How long it took.
    pub duration_ms: u64,
    /// How it ended.
    pub outcome: Outcome,
}

/// Where entries go.
pub trait AuditSink: Send + Sync {
    /// Records one call.
    fn record(&self, entry: AuditEntry);
}

/// The last `capacity` calls, in memory, clearable.
#[derive(Debug)]
pub struct AuditLog {
    capacity: usize,
    entries: Mutex<VecDeque<AuditEntry>>,
}

impl AuditLog {
    /// A log keeping at most `capacity` entries (at least 1).
    pub fn new(capacity: usize) -> AuditLog {
        AuditLog {
            capacity: capacity.max(1),
            entries: Mutex::new(VecDeque::new()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, VecDeque<AuditEntry>> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The entries, oldest first.
    pub fn entries(&self) -> Vec<AuditEntry> {
        self.lock().iter().cloned().collect()
    }

    /// Removes everything (the user's Clear).
    pub fn clear(&self) {
        self.lock().clear();
    }
}

impl AuditSink for AuditLog {
    fn record(&self, entry: AuditEntry) {
        let mut e = self.lock();
        if e.len() == self.capacity {
            e.pop_front();
        }
        e.push_back(entry);
    }
}

/// Redacts call arguments with the family's rules (secrets masked, content-like fields replaced by a size, values bounded).
pub fn redact_arguments(redactor: &dyn Redact, value: &Value, depth: usize) -> Value {
    match value {
        Value::Object(m) if depth < 6 => Value::Object(
            m.iter()
                .filter_map(|(k, v)| {
                    let v = match v {
                        Value::Object(_) | Value::Array(_) if depth < 5 => {
                            redact_arguments(redactor, v, depth + 1)
                        }
                        other => other.clone(),
                    };
                    redactor.field(k, v).map(|v| (k.clone(), v))
                })
                .collect(),
        ),
        Value::Array(a) if depth < 6 => Value::Array(
            a.iter()
                .map(|v| redact_arguments(redactor, v, depth + 1))
                .collect(),
        ),
        Value::String(s) => redactor
            .field("value", Value::String(s.clone()))
            .unwrap_or(Value::Null),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_log::Redactor;
    use serde_json::json;

    fn entry(n: i64) -> AuditEntry {
        AuditEntry {
            at_ms: n,
            client: "c".into(),
            tool: "t".into(),
            arguments: json!({}),
            result_bytes: 0,
            duration_ms: 0,
            outcome: Outcome::Ok,
        }
    }

    #[test]
    fn the_log_is_a_bounded_ring_that_can_be_cleared() {
        let log = AuditLog::new(2);
        for n in 1..=3 {
            log.record(entry(n));
        }
        assert_eq!(
            log.entries().iter().map(|e| e.at_ms).collect::<Vec<_>>(),
            [2, 3]
        );
        log.clear();
        assert!(log.entries().is_empty());
        AuditLog::new(0).record(entry(1));
        assert_eq!(
            serde_json::to_value(Outcome::TimedOut).unwrap(),
            "timed-out"
        );
    }

    #[test]
    fn arguments_are_redacted_deeply_and_bounded() {
        let r = Redactor::new();
        let v = json!({"query": "err", "limit": 5, "apiKey": "sk-secret", "nested": {"password": "hunter2", "ok": 1}, "list": [{"token": "t"}, "plain"],
            "content": "file body that must not be logged", "big": "x".repeat(5000)});
        let out = redact_arguments(&r, &v, 0);
        let text = out.to_string();
        for leaked in ["sk-secret", "hunter2", "file body", "\"t\""] {
            assert!(!text.contains(leaked), "{leaked} leaked: {text}");
        }
        assert!(
            out["query"].as_str().unwrap().starts_with("<omitted"),
            "a query is content: only its size is kept"
        );
        assert_eq!(out["limit"], 5);
        assert_eq!(out["nested"]["ok"], 1);
        assert!(out["big"].as_str().unwrap().chars().count() < 5000);
        let deep = (0..20).fold(json!("leaf"), |acc, _| json!({"a": acc}));
        assert!(redact_arguments(&r, &deep, 0).is_object());
        let dropping = Redactor::new().with_hook(|k, _| (k == "drop").then_some(None));
        assert_eq!(
            redact_arguments(&dropping, &json!({"drop": 1, "keep": 2}), 0),
            json!({"keep": 2})
        );
        assert_eq!(
            redact_arguments(&r, &json!([[[[[[[1]]]]]]]), 0),
            json!([[[[[[[1]]]]]]])
        );
    }
}
