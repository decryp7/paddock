//! The latest activity per window, kept in memory so an error, a crash or a problem report shows what the user did just
//! before. An event is a breadcrumb when it carries an `Operation` property. Entries are what was logged (after
//! redaction), so keeping them adds nothing sensitive.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap, VecDeque};

/// Entries kept per window.
pub const BREADCRUMBS_PER_WINDOW: usize = 200;
/// Entries attached to one error.
pub const BREADCRUMBS_PER_ERROR: usize = 30;
/// Most windows tracked; the least recently active one is forgotten beyond it.
const MAX_WINDOWS: usize = 32;
/// Most parameters one entry carries.
pub const MAX_PARAMS: usize = 16;
/// The breadcrumbs attached to one error are cut to this many bytes of JSON (oldest first).
pub const MAX_BREADCRUMB_JSON: usize = 16 * 1024;
/// Repeated identical errors inside this window are logged once, then summarised.
pub const DEDUP_WINDOW: Duration = Duration::seconds(60);
/// Most distinct errors tracked for de-duplication.
const MAX_DEDUP_KEYS: usize = 256;
/// Properties that are part of the entry itself, not parameters.
const RESERVED: &[&str] = &[
    "SourceContext",
    "Operation",
    "OperationId",
    "Window",
    "MessageTemplate",
    "Severity",
];

/// One activity entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Breadcrumb {
    /// When it happened (RFC 3339, UTC).
    pub time: String,
    /// What the user or the app did.
    pub operation: String,
    /// The window it happened in (`""` when none).
    pub window: String,
    /// The operation id tying it to the command that ran it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    /// The other properties of the event, as text (already redacted).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
}

impl Breadcrumb {
    /// An entry from an event's properties, or `None` without an `Operation`.
    pub fn from_props(time: DateTime<Utc>, props: &Map<String, Value>) -> Option<Breadcrumb> {
        let text = |v: &Value| {
            v.as_str()
                .map(str::to_string)
                .unwrap_or_else(|| v.to_string())
        };
        let operation = text(props.get("Operation")?);
        let get = |k: &str| props.get(k).map(text).filter(|v| !v.is_empty());
        let params = props
            .iter()
            .filter(|(k, _)| !RESERVED.contains(&k.as_str()))
            .take(MAX_PARAMS)
            .map(|(k, v)| (k.clone(), text(v)))
            .collect();
        Some(Breadcrumb {
            time: time.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
            operation,
            window: get("Window").unwrap_or_default(),
            operation_id: get("OperationId"),
            params,
        })
    }
}

/// Ring buffers of the latest entries, per window.
#[derive(Debug, Default)]
pub struct Breadcrumbs {
    windows: HashMap<String, VecDeque<Breadcrumb>>,
    /// Window labels, least recently active first.
    order: VecDeque<String>,
}

impl Breadcrumbs {
    /// Adds an entry to its window's ring.
    pub fn push(&mut self, b: Breadcrumb) {
        let window = b.window.clone();
        self.order.retain(|w| *w != window);
        self.order.push_back(window.clone());
        while self.order.len() > MAX_WINDOWS {
            if let Some(oldest) = self.order.pop_front() {
                self.windows.remove(&oldest);
            }
        }
        let ring = self.windows.entry(window).or_default();
        ring.push_back(b);
        while ring.len() > BREADCRUMBS_PER_WINDOW {
            ring.pop_front();
        }
    }

    /// The latest `n` entries of `window` (every window's, oldest first, when `window` is `None` or has none).
    pub fn recent_for(&self, window: Option<&str>, n: usize) -> Vec<Breadcrumb> {
        let mut out: Vec<Breadcrumb> = match window
            .and_then(|w| self.windows.get(w))
            .filter(|r| !r.is_empty())
        {
            Some(ring) => ring.iter().cloned().collect(),
            None => self.windows.values().flatten().cloned().collect(),
        };
        out.sort_by(|a, b| a.time.cmp(&b.time));
        let skip = out.len().saturating_sub(n);
        out.split_off(skip)
    }
}

/// `crumbs` as a JSON array, the oldest entries dropped until it fits [`MAX_BREADCRUMB_JSON`].
pub fn bounded_json(mut crumbs: Vec<Breadcrumb>) -> String {
    let mut json = serde_json::to_string(&crumbs).unwrap_or_default();
    while json.len() > MAX_BREADCRUMB_JSON && !crumbs.is_empty() {
        crumbs.remove(0);
        json = serde_json::to_string(&crumbs).unwrap_or_default();
    }
    json
}

/// What [`ErrorDedup`] decides for one error.
#[derive(Debug, PartialEq, Eq)]
pub enum Dedup {
    /// Log it.
    Log,
    /// Log it, after a summary: it was suppressed `n` times in the previous window.
    LogAfterRepeats(u64),
    /// Do not log it; it is counted.
    Suppress,
}

/// Logs the first of identical errors in each [`DEDUP_WINDOW`] and counts the rest.
#[derive(Debug, Default)]
pub struct ErrorDedup {
    seen: HashMap<String, (DateTime<Utc>, u64)>,
}

impl ErrorDedup {
    /// Decides for the error `key` seen at `now`.
    pub fn check(&mut self, key: &str, now: DateTime<Utc>) -> Dedup {
        if self.seen.len() >= MAX_DEDUP_KEYS && !self.seen.contains_key(key) {
            self.seen.retain(|_, (t, _)| now - *t < DEDUP_WINDOW);
            if self.seen.len() >= MAX_DEDUP_KEYS {
                self.seen.clear();
            }
        }
        match self.seen.get_mut(key) {
            Some((start, n)) if now - *start < DEDUP_WINDOW && now >= *start => {
                *n += 1;
                Dedup::Suppress
            }
            Some((start, n)) => {
                let repeats = *n;
                *start = now;
                *n = 0;
                if repeats > 0 {
                    Dedup::LogAfterRepeats(repeats)
                } else {
                    Dedup::Log
                }
            }
            None => {
                self.seen.insert(key.to_string(), (now, 0));
                Dedup::Log
            }
        }
    }

    /// Errors with pending repeats (key, count); clears the counts.
    pub fn drain_repeats(&mut self) -> Vec<(String, u64)> {
        let mut out: Vec<(String, u64)> = self
            .seen
            .iter()
            .filter(|(_, (_, n))| *n > 0)
            .map(|(k, (_, n))| (k.clone(), *n))
            .collect();
        out.sort();
        for (_, n) in self.seen.values_mut() {
            *n = 0;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    fn at(s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 20, 9, 0, 0).unwrap() + Duration::seconds(i64::from(s))
    }

    fn crumb(window: &str, op: &str, s: u32) -> Breadcrumb {
        let mut props = Map::new();
        props.insert("Operation".into(), json!(op));
        props.insert("Window".into(), json!(window));
        Breadcrumb::from_props(at(s), &props).unwrap()
    }

    #[test]
    fn entries_come_from_events_with_an_operation() {
        let mut props = Map::new();
        props.insert("Rows".into(), json!(3));
        assert_eq!(Breadcrumb::from_props(at(0), &props), None);
        props.insert("Operation".into(), json!("Open"));
        props.insert("OperationId".into(), json!("op-1"));
        props.insert("Window".into(), json!("main"));
        let b = Breadcrumb::from_props(at(0), &props).unwrap();
        assert_eq!(
            (
                b.operation.as_str(),
                b.window.as_str(),
                b.operation_id.as_deref()
            ),
            ("Open", "main", Some("op-1"))
        );
        assert_eq!(b.params.get("Rows").map(String::as_str), Some("3"));
        assert!(!b.params.contains_key("Operation"));
    }

    #[test]
    fn rings_are_per_window_bounded_and_forget_the_least_recent_window() {
        let mut ring = Breadcrumbs::default();
        for i in 0..250 {
            ring.push(crumb("main", &format!("op{i}"), i));
        }
        let recent = ring.recent_for(Some("main"), 500);
        assert_eq!(recent.len(), BREADCRUMBS_PER_WINDOW);
        assert_eq!(recent.last().unwrap().operation, "op249");
        ring.push(crumb("other", "x", 300));
        assert_eq!(ring.recent_for(Some("other"), 5).len(), 1);
        assert_eq!(
            ring.recent_for(Some("missing"), 3).len(),
            3,
            "an unknown window gets everyone's latest"
        );
        for i in 0..40 {
            ring.push(crumb(&format!("w{i}"), "y", 400 + i));
        }
        assert!(ring.windows.len() <= 32);
    }

    #[test]
    fn json_is_bounded_dropping_the_oldest() {
        let many: Vec<Breadcrumb> = (0..30)
            .map(|i| {
                let mut b = crumb("main", &format!("op{i}"), i);
                b.params.insert("Note".into(), "n".repeat(1000));
                b
            })
            .collect();
        let json = bounded_json(many);
        assert!(json.len() <= MAX_BREADCRUMB_JSON);
        assert!(json.contains("op29") && !json.contains("\"op0\""));
    }

    #[test]
    fn identical_errors_are_logged_once_per_window_and_summarised() {
        let mut d = ErrorDedup::default();
        assert_eq!(d.check("e", at(0)), Dedup::Log);
        assert_eq!(d.check("e", at(10)), Dedup::Suppress);
        assert_eq!(d.check("e", at(20)), Dedup::Suppress);
        assert_eq!(d.check("other", at(20)), Dedup::Log);
        assert_eq!(d.check("e", at(61)), Dedup::LogAfterRepeats(2));
        assert_eq!(d.check("e", at(200)), Dedup::Log);
        assert_eq!(d.check("e", at(210)), Dedup::Suppress);
        assert_eq!(d.drain_repeats(), vec![("e".to_string(), 1)]);
        assert!(d.drain_repeats().is_empty());
    }

    #[test]
    fn the_dedup_table_is_bounded() {
        let mut d = ErrorDedup::default();
        for i in 0..1000 {
            d.check(&format!("e{i}"), at(0));
        }
        assert!(d.seen.len() <= MAX_DEDUP_KEYS);
    }
}
