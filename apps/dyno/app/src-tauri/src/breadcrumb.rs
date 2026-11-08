//! The breadcrumb trail of what the user did (ER-27): one info line per meaningful action, by a stable id, never content. The
//! shared log (`paddock-log`) keeps lines with an `Operation` property as breadcrumbs, attaches the latest ones to every error and
//! the problem report lists them, so a bug report reads as "what they did, then what failed".
//!
//! What is logged: the request's kind (`saveSettings`, `startJob`...), for a job its kind and, when it is a plain word, its action
//! (`helper` / `turnOn`), for a settings save the *names* of the top-level keys that changed (never a value), how long it took and
//! the outcome with the error's code. Never paths, names, titles, hosts or values.

use dyno_core::protocol::Request;
use serde_json::Value;

/// The requests that are actions of the user (everything else is a read the screens make on their own and is not logged, so the
/// trail stays readable). Names are the wire's (camelCase).
const ACTIONS: &[&str] = &[
    "saveSettings",
    "acknowledgeAlarms",
    "shelveAlarm",
    "unshelveAlarm",
    "muteRule",
    "capture",
    "takeSnapshot",
    "startBenchmark",
    "cancelBenchmark",
    "deleteBenchmark",
    "startJob",
    "cancelJob",
    "startDiskScan",
    "cancelDiskScan",
    "retryDiskScan",
    "upsertAlertRule",
    "deleteAlertRule",
    "forgetBaseline",
    "forgetSavedDiskMap",
    "forgetSavedDiskMaps",
    "removeTool",
    "toolOffer",
    "powerKeepAwake",
    "powerStopKeepAwake",
    "powerIdleCancel",
    "powerIdleSnooze",
    "wolWake",
    "wolSave",
    "wolRemove",
    "powerSetWake",
    "powerCancelWake",
    "intelEdit",
];

/// What to log about one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    /// The request's kind.
    pub operation: String,
    /// For a job: its kind and plain-word action, else empty.
    pub kind: String,
}

fn is_word(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 32
        && s.chars().all(|c| c.is_ascii_alphanumeric())
        && s.chars().next().is_some_and(|c| c.is_ascii_lowercase())
}

/// The action `request` is, or `None` for a read.
pub fn describe(request: &Request) -> Option<Action> {
    let value = serde_json::to_value(request).ok()?;
    let (operation, payload) = match &value {
        Value::String(s) => (s.clone(), Value::Null),
        Value::Object(m) if m.len() == 1 => {
            let (k, v) = m.iter().next()?;
            (k.clone(), v.clone())
        }
        _ => return None,
    };
    if !ACTIONS.contains(&operation.as_str()) {
        return None;
    }
    let mut kind = String::new();
    if operation == "startJob" {
        // `{ "spec": { "helper": { "action": "turnOn" } } }`: the spec's own kind, and its action when that is an enum word.
        if let Some(Value::Object(spec)) = payload.get("spec") {
            if let Some((k, v)) = spec.iter().next() {
                kind = k.clone();
                let word = v.as_str().or_else(|| v.get("action").and_then(Value::as_str));
                if let Some(w) = word.filter(|w| is_word(w)) {
                    kind = format!("{k}/{w}");
                }
            }
        }
    }
    Some(Action { operation, kind })
}

/// The names of the top-level keys whose value differs between two settings documents (sorted; values are never returned).
pub fn changed_keys(old: &Value, new: &Value) -> Vec<String> {
    let (Some(a), Some(b)) = (old.as_object(), new.as_object()) else {
        return Vec::new();
    };
    let mut keys: Vec<String> = a.keys().chain(b.keys()).filter(|k| a.get(*k) != b.get(*k)).cloned().collect();
    keys.sort();
    keys.dedup();
    keys.truncate(16);
    keys
}

/// Logs the action's end: info on success, a warning with the error's code on failure. `id` ties the line to the request.
pub fn log_end(action: &Action, id: u64, ms: u128, outcome: Result<(), &str>, changed: &[String]) {
    let id = format!("r{id}");
    match outcome {
        Ok(()) => log::info!(
            Operation = action.operation.as_str(), OperationId = id.as_str(), Kind = action.kind.as_str(),
            Changed = changed.join(",").as_str(), Ms = ms as u64, Outcome = "ok";
            "Action"
        ),
        Err(code) => log::warn!(
            Operation = action.operation.as_str(), OperationId = id.as_str(), Kind = action.kind.as_str(),
            Ms = ms as u64, Outcome = "failed", Code = code;
            "Action failed"
        ),
    }
}

/// The last settings document saved in this run (to name what changed next time); the first save in a run has nothing to compare.
static LAST_SETTINGS: std::sync::Mutex<Option<Value>> = std::sync::Mutex::new(None);

/// Records `new` and returns the names of the keys that differ from the previous document of this run.
pub fn settings_saved(new: Value) -> Vec<String> {
    let mut last = LAST_SETTINGS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let keys = last.as_ref().map(|old| changed_keys(old, &new)).unwrap_or_default();
    *last = Some(new);
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_the_users_actions_are_logged_and_a_job_names_its_kind_not_its_content() {
        assert!(describe(&Request::Settings).is_none(), "a read is not an action");
        assert!(describe(&Request::HelperStatus).is_none());
        let a = describe(&Request::Capture).expect("capture is an action");
        assert_eq!((a.operation.as_str(), a.kind.as_str()), ("capture", ""));
        let job: Request = serde_json::from_value(json!({"startJob": {"spec": {"helper": {"action": "turnOn"}}}})).unwrap();
        let a = describe(&job).expect("a job is an action");
        assert_eq!((a.operation.as_str(), a.kind.as_str()), ("startJob", "helper/turnOn"));
    }

    #[test]
    fn a_settings_change_names_the_keys_and_never_the_values() {
        let old = json!({"theme": "dark", "ipLookup": {"ipTemplate": "https://private.example/{ip}"}, "same": 1});
        let new = json!({"theme": "light", "ipLookup": {"ipTemplate": "https://other.example/{ip}"}, "same": 1, "added": true});
        let keys = changed_keys(&old, &new);
        assert_eq!(keys, ["added", "ipLookup", "theme"]);
        assert!(keys.iter().all(|k| !k.contains("example") && !k.contains("dark")));
        assert!(changed_keys(&json!(1), &new).is_empty());
        assert!(settings_saved(old).is_empty(), "the first save of a run has nothing to compare");
        assert_eq!(settings_saved(new), ["added", "ipLookup", "theme"]);
    }
}
