//! What happens to plugins, for the dashboard's events feed and timeline: starts, failures, quarantines, stops.
//! Delivered on a bounded broadcast channel ([`crate::PluginHost::subscribe`]); a slow listener misses old events
//! rather than holding the host back. Events never carry request content.

use serde::Serialize;

/// One event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostEvent {
    /// The plugin's id.
    pub plugin: String,
    /// When, in milliseconds since the Unix epoch.
    pub at_ms: u64,
    /// What happened.
    pub kind: EventKind,
}

impl HostEvent {
    pub(crate) fn now(plugin: &str, kind: EventKind) -> HostEvent {
        let at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0);
        HostEvent {
            plugin: plugin.to_string(),
            at_ms,
            kind,
        }
    }
}

/// What happened to a plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum EventKind {
    /// Its process started and passed its checks.
    Started {
        /// The process id.
        pid: u32,
    },
    /// It failed (crash, hang, protocol, memory cap, could not start).
    Failed {
        /// What happened.
        message: String,
    },
    /// It was quarantined after repeated failures.
    Quarantined {
        /// Why.
        reason: String,
    },
    /// Its process was stopped by Pitwall (settings changed, removed, shutdown).
    Stopped,
}

/// How many events a slow listener may fall behind before it misses some.
pub const EVENT_BUFFER: usize = 256;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_timestamped_and_serialise_for_the_ui() {
        let e = HostEvent::now("x", EventKind::Started { pid: 7 });
        assert!(e.at_ms > 1_600_000_000_000);
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["kind"]["type"], "started");
        assert_eq!(json["atMs"], e.at_ms);
        assert_eq!(serde_json::to_value(EventKind::Stopped).unwrap()["type"], "stopped");
    }
}
