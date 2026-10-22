//! The real redact filter, installed in a plugins folder and run by the real host in its sandbox.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use pitwall_plugin_host::{HostConfig, PluginHost, PluginSettings, State};
use serde_json::json;
use std::path::Path;

#[tokio::test(flavor = "multi_thread")]
async fn the_host_runs_the_redact_filter_with_its_audit_log() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("plugins").join("redact-filter");
    std::fs::create_dir_all(&folder).unwrap();
    let exe = env!("CARGO_BIN_EXE_pitwall-redact-filter");
    std::fs::copy(exe, folder.join(Path::new(exe).file_name().unwrap())).unwrap();
    std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/pitwall-plugin.toml"), folder.join("pitwall-plugin.toml")).unwrap();
    let mut config = HostConfig::new(root.path().join("plugins"), root.path().join("data"));
    config.sandbox_prefix = "pitwall.test".into();
    let mut settings = PluginSettings::default();
    settings.config.insert("redact-filter".into(), json!({"audit": true}));
    let host = PluginHost::new(config, settings);
    assert!(host.rescan().is_empty(), "{:?}", host.problems());
    let body = json!({"model": "m", "messages": [{"role": "user", "content": "dave@home.example at 192.168.1.20"}]});
    let out = host.filter_request(body).await.unwrap();
    assert_eq!(out["messages"][0]["content"], "[email] at [ipv4]");
    let status = host.plugins().pop().unwrap();
    assert_eq!(status.state, State::Running);
    assert_eq!(status.memory_mb, 128);
    let log = std::fs::read_to_string(root.path().join("data").join("redact-filter").join("audit.log")).unwrap();
    assert!(log.contains("\"emails\":1,\"ipv4\":1") && !log.contains("dave"), "{log}");
    host.stop_all();
    #[cfg(windows)]
    let _ = pitwall_ipc::contain::Container::delete("pitwall.test.redact-filter");
}
