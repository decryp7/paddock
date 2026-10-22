//! Errors of a plugin call, always attributed to the plugin. A typed error the plugin answered with
//! ([`ErrorKind::Plugin`]) is not a failure of the plugin; every other kind but the host-side refusals is, and
//! counts towards its quarantine.

use serde::Serialize;
use std::time::Duration;

/// What went wrong.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ErrorKind {
    /// No plugin with this id.
    NotFound,
    /// The plugin does not make this kind of contribution.
    Unsupported,
    /// The user disabled the plugin.
    Disabled,
    /// Stopped after repeated failures; the user must reset it.
    Quarantined,
    /// It failed recently and restarts after a backoff.
    Restarting {
        /// Milliseconds until the next start.
        retry_after_ms: u64,
    },
    /// The request is too large to send to a plugin.
    TooLarge,
    /// It could not be started (or its sandbox could not be applied).
    Spawn,
    /// It crashed (exited, panicked or closed the connection).
    Crashed,
    /// It did not answer in time.
    Timeout,
    /// It used more than its memory cap and was stopped.
    MemoryCap,
    /// It broke the protocol (garbage, oversize or flooding frames, a wrong answer).
    Protocol,
    /// It answered with a typed error (the plugin is fine).
    Plugin {
        /// The plugin's error code (`pitwall_plugin_api::protocol::code`).
        code: i32,
    },
    /// An engine model handle from before the engine restarted.
    StaleHandle,
}

/// A failed plugin call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginError {
    /// The plugin's id.
    pub plugin: String,
    /// What went wrong.
    pub kind: ErrorKind,
    /// For the user.
    pub message: String,
}

impl PluginError {
    pub(crate) fn new(plugin: &str, kind: ErrorKind, message: impl Into<String>) -> PluginError {
        PluginError {
            plugin: plugin.to_string(),
            kind,
            message: message.into(),
        }
    }

    pub(crate) fn restarting(plugin: &str, wait: Duration) -> PluginError {
        let ms = u64::try_from(wait.as_millis()).unwrap_or(u64::MAX);
        PluginError::new(
            plugin,
            ErrorKind::Restarting { retry_after_ms: ms },
            format!("plugin \"{plugin}\" failed recently and restarts in {} s", ms.div_ceil(1000)),
        )
    }

    /// Whether the plugin itself failed (counts towards its quarantine).
    pub fn is_failure(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::Spawn | ErrorKind::Crashed | ErrorKind::Timeout | ErrorKind::MemoryCap | ErrorKind::Protocol
        )
    }

    /// The HTTP status a server answers a client with for this error.
    pub fn http_status(&self) -> u16 {
        match self.kind {
            ErrorKind::NotFound => 404,
            ErrorKind::Unsupported => 404,
            ErrorKind::Disabled | ErrorKind::Quarantined | ErrorKind::Restarting { .. } | ErrorKind::Spawn => 503,
            ErrorKind::TooLarge => 413,
            ErrorKind::Timeout => 504,
            ErrorKind::Crashed | ErrorKind::MemoryCap | ErrorKind::Protocol => 502,
            ErrorKind::StaleHandle => 409,
            ErrorKind::Plugin { code } => match code {
                pitwall_plugin_api::protocol::code::INVALID => 400,
                pitwall_plugin_api::protocol::code::NOT_FOUND => 404,
                pitwall_plugin_api::protocol::code::UNSUPPORTED => 501,
                _ => 502,
            },
        }
    }
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PluginError {}

/// A request a filter refused (or that could not be filtered, when filters fail closed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rejection {
    /// The filter plugin's id.
    pub plugin: String,
    /// The HTTP status for the client.
    pub status: u16,
    /// Why, for the client.
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pitwall_plugin_api::protocol::code;

    #[test]
    fn failures_and_statuses() {
        let e = |k: ErrorKind| PluginError::new("x", k, "m");
        for k in [ErrorKind::Spawn, ErrorKind::Crashed, ErrorKind::Timeout, ErrorKind::MemoryCap, ErrorKind::Protocol] {
            assert!(e(k.clone()).is_failure(), "{k:?}");
        }
        for k in [
            ErrorKind::NotFound,
            ErrorKind::Unsupported,
            ErrorKind::Disabled,
            ErrorKind::Quarantined,
            ErrorKind::TooLarge,
            ErrorKind::StaleHandle,
            ErrorKind::Plugin { code: 1 },
            ErrorKind::Restarting { retry_after_ms: 1 },
        ] {
            assert!(!e(k.clone()).is_failure(), "{k:?}");
        }
        assert_eq!(e(ErrorKind::NotFound).http_status(), 404);
        assert_eq!(e(ErrorKind::Unsupported).http_status(), 404);
        assert_eq!(e(ErrorKind::Quarantined).http_status(), 503);
        assert_eq!(e(ErrorKind::TooLarge).http_status(), 413);
        assert_eq!(e(ErrorKind::Timeout).http_status(), 504);
        assert_eq!(e(ErrorKind::Crashed).http_status(), 502);
        assert_eq!(e(ErrorKind::StaleHandle).http_status(), 409);
        assert_eq!(e(ErrorKind::Plugin { code: code::INVALID }).http_status(), 400);
        assert_eq!(e(ErrorKind::Plugin { code: code::NOT_FOUND }).http_status(), 404);
        assert_eq!(e(ErrorKind::Plugin { code: code::UNSUPPORTED }).http_status(), 501);
        assert_eq!(e(ErrorKind::Plugin { code: 7 }).http_status(), 502);
        let r = PluginError::restarting("x", Duration::from_millis(1500));
        assert_eq!(r.kind, ErrorKind::Restarting { retry_after_ms: 1500 });
        assert!(r.to_string().contains("restarts in 2 s"));
        let json = serde_json::to_value(e(ErrorKind::Plugin { code: 3 })).unwrap();
        assert_eq!(json["kind"]["kind"], "plugin");
        assert_eq!(json["kind"]["code"], 3);
    }
}
