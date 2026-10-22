//! How the host runs plugins ([`HostConfig`]: folders, limits, timeouts, failure policy) and what the user decided
//! per plugin ([`PluginSettings`]: disabled, network granted, settings). Both are plain values the server owns and
//! persists; the host never reads configuration files itself.

use crate::policy::Policy;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Duration;

/// Per-call deadlines. A plugin that misses one is stopped (a hang) and counts a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeouts {
    /// Describe and configure after a start.
    pub start: Duration,
    /// One request or response filter.
    pub filter: Duration,
    /// One endpoint request.
    pub endpoint: Duration,
    /// One tool invocation.
    pub tool: Duration,
    /// Loading a model.
    pub engine_load: Duration,
    /// Unloading a model.
    pub engine_unload: Duration,
    /// The longest silence within a generation.
    pub generate_idle: Duration,
}

impl Default for Timeouts {
    fn default() -> Timeouts {
        Timeouts {
            start: Duration::from_secs(10),
            filter: Duration::from_secs(10),
            endpoint: Duration::from_secs(30),
            tool: Duration::from_secs(60),
            engine_load: Duration::from_secs(600),
            engine_unload: Duration::from_secs(30),
            generate_idle: Duration::from_secs(120),
        }
    }
}

/// What the host does when a filter fails (crashes, hangs, is quarantined) instead of answering.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FilterFailure {
    /// Refuse the request with 503 (the default: a privacy filter that fails must not let content past).
    #[default]
    Reject,
    /// Skip the filter and carry on.
    Skip,
}

/// How the host runs plugins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostConfig {
    /// The folder of installed plugins: `<id>/pitwall-plugin.toml` and the executable.
    pub plugins_dir: PathBuf,
    /// The folder of the plugins' private data folders (`<data_dir>/<id>`, the only place a plugin may write).
    pub data_dir: PathBuf,
    /// The memory cap of a plugin that asks for none (MiB).
    pub default_memory_mb: u64,
    /// The largest memory cap a plugin may ask for (MiB); engines usually need more than other kinds.
    pub max_memory_mb: u64,
    /// The largest memory cap an engine plugin may ask for (MiB).
    pub max_engine_memory_mb: u64,
    /// Largest frame on a plugin connection (bytes); also bounds request and response bodies.
    pub max_frame: usize,
    /// Deadlines.
    pub timeouts: Timeouts,
    /// Restarts, backoff and quarantine.
    pub policy: Policy,
    /// What a failing filter means for the request.
    pub filter_failure: FilterFailure,
    /// The prefix of each plugin's sandbox name (`<prefix>.<id>`, the AppContainer profile on Windows).
    pub sandbox_prefix: String,
    /// Most plugins discovered.
    pub max_plugins: usize,
}

impl HostConfig {
    /// The defaults for these folders.
    pub fn new(plugins_dir: impl Into<PathBuf>, data_dir: impl Into<PathBuf>) -> HostConfig {
        HostConfig {
            plugins_dir: plugins_dir.into(),
            data_dir: data_dir.into(),
            default_memory_mb: 512,
            max_memory_mb: 4096,
            max_engine_memory_mb: 64 * 1024,
            max_frame: pitwall_ipc::frame::DEFAULT_MAX_FRAME,
            timeouts: Timeouts::default(),
            policy: Policy::default(),
            filter_failure: FilterFailure::Reject,
            sandbox_prefix: "pitwall.plugin".into(),
            max_plugins: 128,
        }
    }
}

/// What the user decided about plugins (persisted by the server; versioned there).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PluginSettings {
    /// Plugins that must not run.
    pub disabled: BTreeSet<String>,
    /// Plugins the user allowed to use the network (effective only when their manifest asks for it too).
    pub network: BTreeSet<String>,
    /// Each plugin's own settings (a JSON object), sent to it after every start.
    pub config: BTreeMap<String, serde_json::Value>,
}

impl PluginSettings {
    /// Whether `id` may run.
    pub fn enabled(&self, id: &str) -> bool {
        !self.disabled.contains(id)
    }

    /// Whether the user allowed `id` the network.
    pub fn network_granted(&self, id: &str) -> bool {
        self.network.contains(id)
    }

    /// The settings of `id` (an empty object when the user set none).
    pub fn config_of(&self, id: &str) -> serde_json::Value {
        self.config.get(id).cloned().unwrap_or_else(|| serde_json::json!({}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_private_and_bounded() {
        let c = HostConfig::new("/p", "/d");
        assert_eq!(c.filter_failure, FilterFailure::Reject);
        assert!(c.default_memory_mb <= c.max_memory_mb && c.max_memory_mb <= c.max_engine_memory_mb);
        assert!(c.timeouts.filter < c.timeouts.engine_load);
        let s = PluginSettings::default();
        assert!(s.enabled("x") && !s.network_granted("x"));
        assert_eq!(s.config_of("x"), serde_json::json!({}));
        let s: PluginSettings = serde_json::from_str(r#"{"disabled":["a"],"network":["b"],"config":{"c":{"k":1}}}"#).unwrap();
        assert!(!s.enabled("a") && s.network_granted("b") && s.config_of("c")["k"] == 1);
        assert_eq!(serde_json::from_str::<PluginSettings>("{}").unwrap(), PluginSettings::default());
        assert_eq!(serde_json::to_string(&FilterFailure::Skip).unwrap(), "\"skip\"");
    }
}
