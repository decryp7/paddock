//! The host against real plugin processes (the `dyno-test-plugin` binary, copied into temporary plugin folders).
//! Every process is one of ours and is stopped when the host drops; the temporary folders are removed.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use dyno_plugin_host::{ErrorKind, HostConfig, PluginHost, PluginState, Policy, Timeouts};
use std::path::Path;
use std::time::{Duration, Instant};

fn add_plugin(root: &Path, id: &str) {
    let dir = root.join(id);
    std::fs::create_dir_all(&dir).unwrap();
    let exe = if cfg!(windows) { "plugin.exe" } else { "plugin" };
    std::fs::copy(env!("CARGO_BIN_EXE_dyno-test-plugin"), dir.join(exe)).unwrap();
    let toml = format!(
        "id = \"{id}\"\nname = \"Test {id}\"\nversion = \"0.0.1\"\napi_version = 1\nexecutable = \"plugin\"\ncontributes = [\"sensors\"]\n"
    );
    std::fs::write(dir.join("plugin.toml"), toml).unwrap();
}

fn host_with(ids: &[&str], tweak: impl FnOnce(&mut HostConfig)) -> (PluginHost, tempfile::TempDir) {
    // The `dyno-it-` prefix is what CI's host clean-up anchors on to end a plugin a failed run left behind.
    let root = tempfile::Builder::new().prefix("dyno-it-plugins-").tempdir().unwrap();
    for id in ids {
        add_plugin(root.path(), id);
    }
    let mut config = HostConfig::new(root.path());
    config.timeouts = Timeouts {
        start: Duration::from_secs(20),
        describe: Duration::from_secs(10),
        sensors: Duration::from_millis(1500),
        ..Timeouts::default()
    };
    config.policy = Policy {
        backoff_initial: Duration::ZERO,
        ..Policy::default()
    };
    tweak(&mut config);
    (PluginHost::new(config), root)
}

fn state(host: &PluginHost, id: &str) -> PluginState {
    host.list().into_iter().find(|p| p.id == id).unwrap().state
}

fn sensors_of(host: &PluginHost, id: &str) -> Result<Vec<dyno_plugin_api::SensorReading>, dyno_plugin_host::HostError> {
    host.sensors()
        .into_iter()
        .find(|r| r.plugin == id)
        .expect("the plugin was asked")
        .result
}

#[test]
fn a_good_plugin_answers_everything_and_stops_cleanly() {
    let (host, _root) = host_with(&["ok"], |_| {});
    assert_eq!(state(&host, "ok"), PluginState::Stopped);
    assert!(host.start_all().is_empty());
    assert_eq!(state(&host, "ok"), PluginState::Running);
    let pid = sensors_of(&host, "ok").unwrap()[0].value;
    assert!(
        pid > 0.0 && pid != f64::from(std::process::id()),
        "the plugin runs in its own process"
    );
    assert_eq!(state(&host, "ok"), PluginState::Running);
    host.stop_all();
    assert_eq!(state(&host, "ok"), PluginState::Stopped);
    assert!(host.sensors().is_empty());
}

#[test]
fn a_panicking_plugin_is_its_own_error_and_is_quarantined_after_three() {
    let (host, _root) = host_with(&["ok", "panic"], |_| {});
    for round in 1..=3 {
        let all = host.sensors();
        assert!(
            all.iter().find(|r| r.plugin == "ok").unwrap().result.is_ok(),
            "the good plugin keeps working"
        );
        let e = all.into_iter().find(|r| r.plugin == "panic").unwrap().result.unwrap_err();
        assert_eq!(e.plugin, "panic");
        assert!(e.message.contains("panicked"), "{}", e.message);
        let expected = if round < 3 { ErrorKind::Crashed } else { ErrorKind::Quarantined };
        assert_eq!(e.kind, expected, "round {round}");
    }
    assert_eq!(state(&host, "panic"), PluginState::Quarantined);
    assert_eq!(host.sensors().len(), 1, "a quarantined plugin is not asked");
    host.unquarantine("panic").unwrap();
    assert_eq!(state(&host, "panic"), PluginState::Running);
}

#[test]
fn a_hanging_plugin_is_killed_after_the_timeout() {
    let (host, _root) = host_with(&["hang", "ok"], |_| {});
    assert!(host.start_all().is_empty());
    let t = Instant::now();
    let all = host.sensors();
    assert!(
        t.elapsed() < Duration::from_secs(8),
        "the plugins are asked in parallel and the hang is cut off"
    );
    let e = all.iter().find(|r| r.plugin == "hang").unwrap().result.clone().unwrap_err();
    assert_eq!(e.kind, ErrorKind::Timeout);
    assert!(e.message.contains("stopped"));
    assert_eq!(state(&host, "hang"), PluginState::Failed);
    assert!(sensors_of(&host, "ok").is_ok());
}

#[test]
fn a_plugin_that_exits_at_once_fails_to_start_and_is_quarantined() {
    let (host, _root) = host_with(&["exit"], |_| {});
    let t = Instant::now();
    let e = host.start_all().remove(0);
    assert!(
        t.elapsed() < Duration::from_secs(10),
        "the exit is noticed before the start timeout"
    );
    assert_eq!((e.plugin.as_str(), e.kind), ("exit", ErrorKind::Crashed));
    // The child may exit before the bootstrap is written (a closed pipe) or after it: one outcome either way, with the OS
    // error in the message when there is one. The outcome is the kind above and the quarantine below, not the exact wording.
    assert!(e.message.contains("exited before it"), "{}", e.message);
    assert_eq!(host.start_all()[0].kind, ErrorKind::Crashed);
    assert_eq!(host.start_all()[0].kind, ErrorKind::Quarantined);
    assert_eq!(state(&host, "exit"), PluginState::Quarantined);
}

#[test]
fn a_plugin_sending_garbage_is_a_protocol_error() {
    let (host, _root) = host_with(&["garbage"], |_| {});
    assert!(host.start_all().is_empty());
    let e = sensors_of(&host, "garbage").unwrap_err();
    assert_eq!(e.kind, ErrorKind::Protocol);
    assert_eq!(state(&host, "garbage"), PluginState::Failed);
}

#[test]
fn a_plugin_claiming_another_id_is_refused() {
    let (host, _root) = host_with(&["liar"], |_| {});
    let e = host.start_all().remove(0);
    assert_eq!(e.kind, ErrorKind::Protocol);
    assert!(e.message.contains("someone-else"));
}

#[cfg(windows)]
#[test]
fn a_plugin_past_its_memory_cap_crashes_alone() {
    let (host, _root) = host_with(&["memory", "ok"], |c| {
        c.memory_limit = Some(128 * 1024 * 1024);
        c.timeouts.sensors = Duration::from_secs(10);
    });
    assert!(host.start_all().is_empty());
    let all = host.sensors();
    let e = all.iter().find(|r| r.plugin == "memory").unwrap().result.clone().unwrap_err();
    assert_eq!(e.kind, ErrorKind::Crashed, "{e}");
    assert!(all.iter().find(|r| r.plugin == "ok").unwrap().result.is_ok());
}

#[test]
fn invalid_folders_are_listed_with_what_to_fix() {
    let (host, root) = host_with(&["ok"], |_| {});
    drop(host);
    std::fs::create_dir_all(root.path().join("broken")).unwrap();
    std::fs::write(root.path().join("broken").join("plugin.toml"), "id = 1").unwrap();
    let host = PluginHost::new(HostConfig::new(root.path()));
    let list = host.list();
    let broken = list.iter().find(|p| p.id == "broken").unwrap();
    assert_eq!(broken.state, PluginState::Failed);
    assert!(broken.error.as_deref().unwrap().contains("not valid"));
    assert!(host.start_all().is_empty(), "an invalid folder is not started");
}
