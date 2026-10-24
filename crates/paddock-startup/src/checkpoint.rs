//! The UI state checkpoint (ADR 0058): each window's latest UI state, kept in the app's memory only, which a page the
//! shell reloaded gets back with its `window_ready` reply. The app's autosaved session on disk is for restarts; this is
//! for reloads within the run. What the state holds is the app's (a `UiCheckpoint` contribution in the UI).

use crate::watchdog::{Recovery, Watchdog};
use serde::Serialize;
use std::collections::HashMap;
use std::time::Instant;

/// The latest UI state of each window.
#[derive(Debug, Default)]
pub struct Checkpoints(HashMap<String, serde_json::Value>);

impl Checkpoints {
    /// Keeps `ui` as window `label`'s latest state.
    pub fn put(&mut self, label: &str, ui: serde_json::Value) {
        self.0.insert(label.to_string(), ui);
    }

    /// The window's last checkpoint (kept: a second reload restores it too).
    pub fn get(&self, label: &str) -> Option<serde_json::Value> {
        self.0.get(label).cloned()
    }

    /// Forgets a closed window.
    pub fn closed(&mut self, label: &str) {
        self.0.remove(label);
    }
}

/// The reply to `window_ready`.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadyReply {
    /// Set when the shell reloaded this window before this page loaded.
    pub recovered: Option<Recovery>,
    /// With `recovered`: the window's last UI state checkpoint for the reloaded page to apply. The engine kept the data;
    /// this is the rest (query, filters, selection).
    #[cfg_attr(test, ts(type = "unknown"))]
    pub ui: Option<serde_json::Value>,
}

/// What `window_ready` answers: the recovery reason and, with it, the UI state to restore. A normal start (or a reload
/// the user asked for) restores nothing: the page's own state is the truth.
pub fn ready_reply(
    dog: &mut Watchdog,
    checkpoints: &Checkpoints,
    label: &str,
    now: Instant,
) -> ReadyReply {
    let recovered = dog.ready(label, now);
    let ui = recovered.and_then(|_| checkpoints.get(label));
    ReadyReply { recovered, ui }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Duration;

    #[test]
    fn a_recovered_page_gets_the_last_checkpoint_and_a_normal_start_does_not() {
        let t0 = Instant::now();
        let mut w = Watchdog::default();
        let mut c = Checkpoints::default();
        w.page_started("main", t0);
        c.put("main", json!({"query": {"text": "error"}}));
        assert_eq!(
            ready_reply(&mut w, &c, "main", t0),
            ReadyReply {
                recovered: None,
                ui: None
            }
        );
        c.put(
            "main",
            json!({"query": {"text": "timeout"}, "selection": {"recordId": 42}}),
        );
        assert!(w.terminated("main", t0 + Duration::from_secs(1)));
        let reply = ready_reply(&mut w, &c, "main", t0 + Duration::from_secs(2));
        assert_eq!(reply.recovered, Some(Recovery::Terminated));
        assert_eq!(
            reply.ui,
            Some(json!({"query": {"text": "timeout"}, "selection": {"recordId": 42}}))
        );
        // Kept for a second reload; forgotten with the window; other windows have their own.
        assert_eq!(c.get("main"), reply.ui);
        assert_eq!(c.get("w-2"), None);
        c.closed("main");
        assert_eq!(c.get("main"), None);
    }

    #[test]
    fn a_recovered_page_without_a_checkpoint_restores_nothing() {
        let mut w = Watchdog::default();
        let now = Instant::now();
        w.terminated("w-2", now);
        assert_eq!(
            ready_reply(&mut w, &Checkpoints::default(), "w-2", now),
            ReadyReply {
                recovered: Some(Recovery::Terminated),
                ui: None
            }
        );
    }

    #[test]
    fn the_reply_is_camel_case_json() {
        let reply = ReadyReply {
            recovered: Some(Recovery::Hung),
            ui: Some(json!({"a": 1})),
        };
        assert_eq!(
            serde_json::to_value(&reply).unwrap(),
            json!({"recovered": "hung", "ui": {"a": 1}})
        );
    }
}
