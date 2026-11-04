//! `service_main` in a real process (Unix): SIGTERM is waited for with `sigwait` and becomes the stop event, a failing body
//! exits with its own status, and on Linux the OS log is standard error with journald's priority prefixes. The test runs
//! itself again as the service (the child mode below), because signals and exit statuses belong to a process.
#![cfg(unix)]

use paddock_service::{Hosting, ServiceExit, ServiceSpec, service_main};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

const CHILD: &str = "PADDOCK_SERVICE_MAIN_CHILD";

#[derive(Debug)]
struct Refused;

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("refused on purpose")
    }
}

impl std::error::Error for Refused {}

impl ServiceExit for Refused {
    fn exit_code(&self) -> u32 {
        7
    }
    fn next_step(&self) -> Option<String> {
        Some("run it again".into())
    }
}

fn spec() -> ServiceSpec {
    ServiceSpec::builder(
        "net.decryptology.probe.service",
        "ProbeService",
        "probe-service",
    )
    .build()
    .unwrap()
}

/// Runs as the service when the parent asked for it; returns for the parent's own run.
fn child_mode() {
    let Ok(mode) = std::env::var(CHILD) else {
        return;
    };
    let result = match mode.as_str() {
        "wait" => service_main(&spec(), Hosting::Foreground, |context| {
            println!("ready");
            // Event-driven: the signal thread sends the stop; no polling.
            let event = context.events.recv();
            println!(
                "event {event:?} flag {}",
                context.stop.load(std::sync::atomic::Ordering::SeqCst)
            );
            Ok::<(), Refused>(())
        }),
        _ => service_main(&spec(), Hosting::Foreground, |_| Err(Refused)),
    };
    let code = result.map_or(99, paddock_service::service_main::exit_status);
    std::process::exit(code);
}

fn spawn(mode: &str) -> std::process::Child {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_entry", "--nocapture", "--test-threads=1"])
        .env(CHILD, mode)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

/// The entry the child process runs (the parent's `cargo test` run does nothing here).
#[test]
fn child_entry() {
    child_mode();
}

#[test]
fn sigterm_is_the_stop_event_and_the_service_exits_cleanly() {
    let mut child = spawn("wait");
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    // Wait for the body to run before signalling: the signals are blocked before the body starts.
    loop {
        line.clear();
        assert!(
            out.read_line(&mut line).unwrap() > 0,
            "the child ended before it was ready"
        );
        // libtest prints "test child_entry ... " first, so the word ends that line.
        if line.trim_end().ends_with("ready") {
            break;
        }
    }
    // SIGTERM to the child, by its identity (pid and start time, ADR 0033).
    let id = paddock_proc::ProcessId::of(child.id()).expect("the child runs");
    assert!(paddock_proc::ask(id));
    let rest: Vec<String> = out.lines().map_while(Result::ok).collect();
    let status = child.wait().unwrap();
    assert!(status.success(), "{status:?} {rest:?}");
    assert!(
        rest.iter().any(|l| l.contains("event Ok(Stop) flag true")),
        "{rest:?}"
    );
}

#[test]
fn a_failing_body_exits_with_its_code_and_the_reason_is_in_the_os_log() {
    let output = spawn("fail").wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(7));
    #[cfg(target_os = "linux")]
    {
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(
                "ProbeService did not run (exit code 7): refused on purpose Next step: run it again"
            ),
            "{stderr}"
        );
    }
}
