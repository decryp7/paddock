//! `service_main` in a real process (Unix): SIGTERM is handled process-wide and becomes the stop event (a second one ends a body that ignores the first), a failing body
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
        "stubborn" => service_main(
            &spec(),
            Hosting::Foreground,
            |context| -> Result<(), Refused> {
                println!("ready");
                let event = context.events.recv();
                // Ignores the stop: the body never ends by itself.
                println!("stopping {event:?}");
                loop {
                    std::thread::park();
                }
            },
        ),
        _ => service_main(&spec(), Hosting::Foreground, |_| Err(Refused)),
    };
    let code = result.map_or(99, paddock_service::service_main::exit_status);
    std::process::exit(code);
}

/// How long a child of these tests may live: the deadline of every wait (a hung child is ended, and the test fails).
const DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);

/// Ends `child` when `DEADLINE` passes before the returned guard is dropped (the wait is the channel, not a poll).
#[allow(dead_code, reason = "dropping it is what stops the watchdog")]
struct Watchdog(std::sync::mpsc::Sender<()>);

fn watch(child: &std::process::Child) -> Watchdog {
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let id = paddock_proc::ProcessId::of(child.id()).expect("the child runs");
    std::thread::spawn(move || {
        if rx.recv_timeout(DEADLINE) == Err(std::sync::mpsc::RecvTimeoutError::Timeout) {
            paddock_proc::end(id);
        }
    });
    Watchdog(tx)
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
    let _watchdog = watch(&child);
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
    let child = spawn("fail");
    let _watchdog = watch(&child);
    let output = child.wait_with_output().unwrap();
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

#[test]
fn a_second_sigterm_ends_a_body_that_ignored_the_first() {
    use std::os::unix::process::ExitStatusExt;
    let mut child = spawn("stubborn");
    let _watchdog = watch(&child);
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let id = paddock_proc::ProcessId::of(child.id()).expect("the child runs");
    let mut line = String::new();
    let mut wait_for = |word: &str| loop {
        line.clear();
        assert!(
            out.read_line(&mut line).unwrap() > 0,
            "the child ended before {word}"
        );
        if line.contains(word) {
            return;
        }
    };
    wait_for("ready");
    assert!(paddock_proc::ask(id));
    wait_for("stopping Ok(Stop)");
    // The body ignores the stop; the default action is back, so the second signal ends the process.
    assert!(paddock_proc::ask(id));
    let status = child.wait().unwrap();
    assert_eq!(status.signal(), Some(15), "{status:?}");
}
