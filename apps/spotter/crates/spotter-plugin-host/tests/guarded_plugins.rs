//! Every way a plugin can fail, triggered for real with the fake plugin binary: the host reports it, attributes it to
//! the plugin, quarantines repeat offenders, and never panics or hangs.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use spotter_core::source::SourceError;
use spotter_core::time::Timestamp;
use spotter_plugin_api::Config;
use spotter_plugin_host::PluginHost;
use std::path::Path;
use std::time::Instant;

/// Installs the fake plugin under the name `id` (its behaviour) in `root/id/`.
fn install(root: &Path, id: &str, extra: &str) {
    let dir = root.join(id);
    std::fs::create_dir_all(&dir).unwrap();
    let exe_name = if cfg!(windows) { format!("{id}.exe") } else { id.to_string() };
    std::fs::copy(env!("CARGO_BIN_EXE_spotter-fake-plugin"), dir.join(&exe_name)).unwrap();
    std::fs::write(dir.join("plugin.toml"), format!("id = '{id}'\nexecutable = '{id}'\n{extra}")).unwrap();
}

fn host_with(plugins: &[(&str, &str)]) -> (tempfile::TempDir, PluginHost) {
    let root = tempfile::tempdir().unwrap();
    for (id, extra) in plugins {
        install(root.path(), id, extra);
    }
    let host = PluginHost::new();
    host.scan(root.path());
    (root, host)
}

fn problem(host: &PluginHost, id: &str) -> String {
    host.statuses()
        .into_iter()
        .find(|s| s.installed.id == id)
        .unwrap()
        .installed
        .problem
        .unwrap_or_default()
}

#[test]
fn a_good_plugin_describes_itself_and_lists() {
    let (_root, host) = host_with(&[("good", "")]);
    let kinds = host.kinds();
    assert_eq!(kinds.len(), 1);
    assert_eq!(kinds[0].0, "good/files");
    assert!(host.kind("good/files").is_some());
    let out = host.list("good", "files", Config::new().with("dir", "d"), 3, Timestamp(0));
    let releases = out.result.unwrap();
    assert_eq!(releases.len(), 3);
    assert_eq!(releases[0].name, "d-1.0.0");
    assert!(out.issue.is_none());
    host.set_disabled(vec!["good".into()]);
    assert!(host.kinds().is_empty());
    assert!(
        matches!(host.list("good", "files", Config::new(), 3, Timestamp(0)).result, Err(SourceError::Plugin(m)) if m.contains("switched off"))
    );
}

#[test]
fn plugins_that_fail_at_start_are_not_loaded() {
    let started = Instant::now();
    let (_root, host) = host_with(&[("exit", ""), ("hang", ""), ("garbage", ""), ("old-protocol", ""), ("wrong-id", "")]);
    assert!(host.kinds().is_empty());
    assert!(problem(&host, "exit").contains("crashed"), "{}", problem(&host, "exit"));
    assert!(problem(&host, "hang").contains("not a Spotter plugin") || problem(&host, "hang").contains("did not answer"));
    assert!(problem(&host, "garbage").contains("invalid answer") || problem(&host, "garbage").contains("not a Spotter plugin"));
    assert!(
        problem(&host, "old-protocol").contains("too old"),
        "{}",
        problem(&host, "old-protocol")
    );
    assert!(problem(&host, "wrong-id").contains("must agree"));
    // The hanging plugin is stopped at the handshake deadline (10 s), not waited for forever.
    assert!(started.elapsed().as_secs() < 60);
    assert!(host.health().contains_key("exit"), "a crash at start is a strike");
}

#[test]
fn crashes_hangs_and_memory_blowups_are_strikes_and_quarantine() {
    let (_root, host) = host_with(&[
        ("list-abort", ""),
        ("list-hang", "timeout_secs = 1"),
        ("list-memory", "memory_limit_mb = 64"),
        ("list-panic", ""),
        ("list-offline", ""),
    ]);
    assert_eq!(host.kinds().len(), 5);
    let list = |id: &str, t: i64| host.list(id, "files", Config::new(), 10, Timestamp(t));

    let crash = list("list-abort", 0);
    assert!(
        matches!(&crash.result, Err(SourceError::Plugin(m)) if m.contains("crashed")),
        "{:?}",
        crash.result
    );
    assert!(crash.issue.is_some() && !crash.quarantined);
    assert!(!list("list-abort", 1).quarantined);
    let third = list("list-abort", 2);
    assert!(third.quarantined, "three strikes");
    let blocked = list("list-abort", 3);
    assert!(matches!(&blocked.result, Err(SourceError::Plugin(m)) if m.contains("quarantined")));
    assert!(blocked.issue.is_none(), "a quarantined plugin is not started at all");
    assert!(
        host.kind("list-abort/files").is_some(),
        "its watches stay valid and show the quarantine"
    );
    host.reset("list-abort");
    assert!(list("list-abort", 4).issue.is_some(), "after a reset it runs again");

    let started = Instant::now();
    let hang = list("list-hang", 0);
    assert!(
        matches!(&hang.result, Err(SourceError::Plugin(m)) if m.contains("did not answer")),
        "{:?}",
        hang.result
    );
    assert!(started.elapsed().as_secs() < 10);

    let mem = list("list-memory", 0);
    assert!(
        matches!(&mem.result, Err(SourceError::Plugin(m)) if m.contains("memory")),
        "{:?}",
        mem.result
    );

    let panic = list("list-panic", 0);
    assert!(
        matches!(&panic.result, Err(SourceError::Failed(m)) if m.contains("crashed: boom")),
        "{:?}",
        panic.result
    );
    assert!(panic.issue.is_none(), "a contained panic is the plugin's own reported failure");

    let offline = list("list-offline", 0);
    assert!(matches!(offline.result, Err(SourceError::Unreachable(_))));
    assert!(offline.issue.is_none(), "ordinary errors are not strikes");
    assert!(!host.health().contains_key("list-offline"));
}
