//! ADR 0073 / rule 73: the shipped `pitwall-server` carries no test switch. The fake engine is its own test-only
//! program (`pitwall-fake-engine`); the server's command line does not know its flags.
#![allow(clippy::unwrap_used, clippy::expect_used)]

/// Hang guard, never a verdict: a server that accepted a test flag would serve forever, so the guard turns that into a
/// failure (the child is killed) instead of a stuck run.
const HANG_GUARD: std::time::Duration = std::time::Duration::from_secs(60);

#[allow(clippy::disallowed_methods)] // a test starting the built binary and waiting for it to end
fn run_server(args: &[&str]) -> std::process::Output {
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_pitwall-server"))
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("pitwall-server starts");
    let (done, guard) = std::sync::mpsc::channel::<()>();
    let pid = child.id();
    let watchdog = std::thread::spawn(move || {
        if guard.recv_timeout(HANG_GUARD) == Err(std::sync::mpsc::RecvTimeoutError::Timeout) {
            let _ = std::process::Command::new(if cfg!(windows) { "taskkill" } else { "kill" })
                .args(if cfg!(windows) {
                    vec!["/F".to_string(), "/PID".into(), pid.to_string()]
                } else {
                    vec!["-9".to_string(), pid.to_string()]
                })
                .status();
        }
    });
    let out = child.wait_with_output().expect("pitwall-server ends");
    let _ = done.send(());
    let _ = watchdog.join();
    out
}

#[test]
fn the_shipped_server_refuses_every_test_flag_with_the_usage_error() {
    for flag in [vec!["--fake-engine"], vec!["--die-after-ms", "5"], vec!["--ready-when", "/tmp/x"]] {
        let out = run_server(&flag);
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{flag:?} must be a usage error, stderr: {err}");
        assert!(err.contains(&format!("unknown option {}", flag[0])), "{flag:?}: {err}");
    }
}

#[test]
fn the_probe_sees_a_known_flag() {
    // Control (ER-33): the check above can tell an accepted flag from a refused one.
    let out = run_server(&["--version"]);
    assert!(out.status.success() && String::from_utf8_lossy(&out.stdout).starts_with("pitwall-server "));
}
