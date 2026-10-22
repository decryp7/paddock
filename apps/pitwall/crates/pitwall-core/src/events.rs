//! The events feed (ADR 0010): model loads and releases with their reason, engine starts and restarts, downloads,
//! pairings, errors. A bounded ring in memory; the server also writes each event to its log (never with request
//! content).

use crate::memory::ReleaseReason;
use crate::time::Millis;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Events kept.
pub const MAX_EVENTS: usize = 2000;

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EventKind {
    /// A model finished loading.
    ModelLoaded,
    /// A model was unloaded.
    ModelUnloaded,
    /// A model's context was released.
    ContextReleased,
    /// An engine process started.
    EngineStarted,
    /// An engine process was restarted after a crash.
    EngineRestarted,
    /// An engine failed for good.
    EngineFailed,
    /// A download changed state.
    Download,
    /// An app asked to pair, or a pairing was decided.
    Pairing,
    /// Serving was paused or resumed.
    ServingChanged,
    /// The memory preset changed.
    PresetChanged,
    /// A request failed.
    RequestFailed,
}

/// How bad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    /// For the record.
    Info,
    /// Worth a look.
    Warn,
    /// Something failed.
    Error,
}

/// One event (`PitwallEvent` in `api.types.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    /// Increasing id.
    pub id: u64,
    /// When.
    pub at: Millis,
    /// What.
    pub kind: EventKind,
    /// How bad.
    pub severity: Severity,
    /// One sentence (never request content).
    pub message: String,
    /// The model concerned.
    pub model_id: Option<String>,
    /// The app concerned.
    pub app_id: Option<String>,
    /// Why memory was released.
    pub reason: Option<ReleaseReason>,
}

/// The ring.
#[derive(Debug, Clone, Default)]
pub struct EventLog {
    next: u64,
    events: VecDeque<Event>,
}

impl EventLog {
    /// Adds an event and returns it for its optional fields.
    pub fn push(&mut self, at: Millis, kind: EventKind, severity: Severity, message: impl Into<String>) -> &mut Event {
        self.next += 1;
        if self.events.len() >= MAX_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(Event { id: self.next, at, kind, severity, message: message.into(), model_id: None, app_id: None, reason: None });
        let last = self.events.len() - 1;
        &mut self.events[last]
    }

    /// The newest `limit` events, newest first.
    pub fn recent(&self, limit: usize) -> Vec<Event> {
        self.events.iter().rev().take(limit).cloned().collect()
    }

    /// Events in `[from, to]`, oldest first (for the history chart's markers).
    pub fn between(&self, from: Millis, to: Millis) -> Vec<Event> {
        self.events.iter().filter(|e| e.at >= from && e.at <= to).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_ring() {
        let mut l = EventLog::default();
        let e = l.push(5, EventKind::ModelUnloaded, Severity::Info, "released");
        e.model_id = Some("m".into());
        e.reason = Some(ReleaseReason::Idle);
        for i in 0..MAX_EVENTS {
            l.push(10 + i as u64, EventKind::Download, Severity::Info, "d");
        }
        assert_eq!(l.recent(usize::MAX).len(), MAX_EVENTS);
        assert_eq!(l.recent(1)[0].id, MAX_EVENTS as u64 + 1);
        assert!(l.between(0, 9).is_empty(), "the oldest fell out");
        assert_eq!(l.between(10, 11).len(), 2);
        let json = serde_json::to_value(&l.recent(1)[0]).unwrap();
        assert_eq!(json["kind"], "download");
        assert_eq!(json["severity"], "info");
        assert!(json["modelId"].is_null());
    }
}
