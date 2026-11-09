//! ADR 0073 / rule 73: the shipped `pitwall-server` carries no test switch. The fake engine is its own test-only
//! program (`pitwall-fake-engine`); the server's command line does not know its flags.
#![allow(clippy::unwrap_used, clippy::expect_used)]

/// Hang guard, never a verdict: a server that accepted a test flag would serve forever, so the guard turns that into a
/// failure (the held child is killed when its future is dropped) instead of a stuck run.
const HANG_GUARD: std::time::Duration = std::time::Duration::from_secs(60);

/// Runs the built server with a throw-away data folder and port 0 (so a server that did start would touch nothing of
/// the user's and take no fixed port), then `extra`.
#[allow(clippy::disallowed_methods)] // a test starting the built binary and waiting for it to end
async fn run_server(data: &std::path::Path, extra: &[&str]) -> std::process::Output {
    let child = tokio::process::Command::new(env!("CARGO_BIN_EXE_pitwall-server"))
        .arg("--data-dir")
        .arg(data)
        .args(["--port", "0"])
        .args(extra)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("pitwall-server starts");
    tokio::time::timeout(HANG_GUARD, child.wait_with_output())
        .await
        .expect("pitwall-server ended: it accepted a test flag and kept serving")
        .expect("pitwall-server ends")
}

#[tokio::test]
async fn the_shipped_server_refuses_every_test_flag_with_the_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    let gate = dir.path().join("ready").to_string_lossy().into_owned();
    for flag in [
        vec!["--fake-engine"],
        vec!["--die-after-ms", "5"],
        vec!["--ready-when", gate.as_str()],
    ] {
        let out = run_server(dir.path(), &flag).await;
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{flag:?} must be a usage error, stderr: {err}");
        assert!(err.contains(&format!("unknown option {}", flag[0])), "{flag:?}: {err}");
    }
}

#[tokio::test]
async fn the_probe_sees_a_known_flag() {
    // Control (ER-33): the check above can tell an accepted flag from a refused one.
    let dir = tempfile::tempdir().unwrap();
    let out = run_server(dir.path(), &["--version"]).await;
    assert!(out.status.success() && String::from_utf8_lossy(&out.stdout).starts_with("pitwall-server "));
}
