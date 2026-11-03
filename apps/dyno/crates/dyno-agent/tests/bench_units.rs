//! Linux: the benchmark child as a named systemd user unit, with a scripted runner (ADR 0030 D5, the B2 spike as a test).
//! Needs a systemd user manager, so the tests are `#[ignore]`d in a plain `cargo test` (a CI container usually has none) and FAIL,
//! never pass silently, when run without one. Run them where a manager exists: `scripts/linux-units-check.sh` (release checklist,
//! `docs/releasing.md`). Nothing here touches anything it did not start: unit names carry this test process's id.
#![cfg(target_os = "linux")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]

use dyno_agent::bench::{BenchLauncher, ProcessLauncher, SystemctlControl};
use dyno_agent::bench_child::{cleanup_leftovers, write_records, ChildEvents, ChildRequest, Stop};
use dyno_core::bench::unit::{self, UnitRecord};
use dyno_core::bench::wire::{Liveness, PhaseProgress};
use dyno_core::bench::{BenchKind, BenchStatus};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Fails the test when there is no user manager: an ignored test that is asked for must run (ER-12).
fn manager() {
    assert!(
        SystemctlControl::available(),
        "no systemd user manager here (systemctl --user is-system-running must say running or degraded); see docs/linux.md"
    );
}

/// Writes an executable script and returns its path.
fn script(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn request(run: &str) -> ChildRequest {
    ChildRequest {
        run_id: run.into(),
        kind: BenchKind::Cpu,
        scratch: std::path::PathBuf::new(),
        disk_mib: 16,
    }
}

fn systemctl(args: &[&str]) -> String {
    let out = Command::new("systemctl").arg("--user").args(args).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn active(unit: &str) -> bool {
    systemctl(&["is-active", &format!("{unit}.service")]) == "active"
}

/// Cancels after the runner has reported once; remembers when it decided to.
struct CancelAfterProgress {
    seen: AtomicBool,
    cancelled_at: Mutex<Option<Instant>>,
}

impl ChildEvents for CancelAfterProgress {
    fn progress(&self, _: &PhaseProgress) {
        self.seen.store(true, Ordering::SeqCst);
    }

    fn liveness(&self, _: Liveness) {}

    fn stop(&self) -> Option<Stop> {
        if self.seen.load(Ordering::SeqCst) {
            self.cancelled_at.lock().unwrap().get_or_insert_with(Instant::now);
            return Some(Stop::Cancelled);
        }
        None
    }
}

struct Never;

impl ChildEvents for Never {
    fn progress(&self, _: &PhaseProgress) {}
    fn liveness(&self, _: Liveness) {}
    fn stop(&self) -> Option<Stop> {
        None
    }
}

#[test]
#[ignore = "needs a systemd user manager (docs/linux.md)"]
fn a_unit_run_waits_for_go_runs_at_nice_0_without_a_memory_limit_and_leaves_nothing() {
    manager();
    let dir = tempfile::tempdir().unwrap();
    // The runner reads `go` (stdin through --pipe), then reports its own nice value and memory.high through its result line.
    let runner = script(
        dir.path(),
        "runner.sh",
        r#"read go
[ "$go" = go ] || { echo '{"t":"result","status":{"state":"failed","reason":"no go"}}'; exit 0; }
ni=$(ps -o ni= -p $$ | tr -d ' ')
mh=$(cat /sys/fs/cgroup$(cut -d: -f3 /proc/self/cgroup)/memory.high 2>/dev/null || echo none)
echo '{"t":"heartbeat"}'
echo "{\"t\":\"result\",\"status\":{\"state\":\"unavailable\",\"reason\":\"ni=$ni mh=$mh\"}}""#,
    );
    let records = dir.path().join("bench-units.json");
    let launcher = ProcessLauncher::with_units(&runner, &records);
    let status = launcher.run(&request(&format!("t1-{}", std::process::id())), &Never);
    let BenchStatus::Unavailable { reason } = status else {
        panic!("{status:?}")
    };
    assert!(
        reason.starts_with("ni=0 "),
        "the unit runs at nice 0 even when the agent is lowered: {reason}"
    );
    assert!(reason.ends_with("mh=max") || reason.ends_with("mh=none"), "no MemoryHigh: {reason}");
    assert!(!records.exists(), "the record is forgotten when the run ends");
    assert!(!active(&unit::unit_name(&format!("t1-{}-cpu", std::process::id()))));
}

#[test]
#[ignore = "needs a systemd user manager (docs/linux.md)"]
fn cancel_stops_an_all_core_burner_within_two_seconds_and_nothing_is_left() {
    manager();
    let dir = tempfile::tempdir().unwrap();
    let runner = script(
        dir.path(),
        "burner.sh",
        r#"read go
for i in 1 2 3 4 5 6 7 8; do (while :; do :; done) & done
echo '{"t":"progress","phase":"cpu","label":"burning","rep":0,"of":5,"count":0,"total":5}'
while :; do echo '{"t":"heartbeat"}'; sleep 1; done"#,
    );
    let records = dir.path().join("bench-units.json");
    let launcher = ProcessLauncher::with_units(&runner, &records);
    let run = format!("t2-{}", std::process::id());
    let events = CancelAfterProgress {
        seen: AtomicBool::new(false),
        cancelled_at: Mutex::new(None),
    };
    let status = launcher.run(&request(&run), &events);
    let took = events.cancelled_at.lock().unwrap().expect("it cancelled").elapsed();
    assert!(
        matches!(status, BenchStatus::Stopped { ref reason } if reason == "Cancelled."),
        "{status:?}"
    );
    assert!(took < Duration::from_secs(2), "Cancel returned after {took:?}");
    assert!(!active(&unit::unit_name(&format!("{run}-cpu"))), "the unit is gone");
    let left = Command::new("pgrep").arg("-f").arg(runner.to_str().unwrap()).output().unwrap();
    assert!(
        String::from_utf8_lossy(&left.stdout).trim().is_empty(),
        "burners left: {}",
        String::from_utf8_lossy(&left.stdout)
    );
    assert!(!records.exists());
}

#[test]
#[ignore = "needs a systemd user manager (docs/linux.md)"]
fn a_restart_stops_a_recorded_leftover_unit_and_leaves_a_foreign_one() {
    manager();
    let dir = tempfile::tempdir().unwrap();
    let pid = std::process::id();
    let (mine, foreign) = (format!("dyno-bench-t3a-{pid}"), format!("dyno-bench-t3b-{pid}"));
    for u in [&mine, &foreign] {
        let c = unit::UnitCommand {
            program: "systemd-run",
            args: vec![
                "--user".into(),
                format!("--unit={u}"),
                "--collect".into(),
                "--".into(),
                "sleep".into(),
                "300".into(),
            ],
        };
        assert!(Command::new(c.program).args(&c.args).output().unwrap().status.success());
    }
    let control = SystemctlControl;
    let mine_id = control.invocation_id(&mine).expect("an invocation id");
    // The record for the foreign unit names a different invocation (a reused name).
    let records = dir.path().join("bench-units.json");
    write_records(
        &records,
        &[
            UnitRecord {
                unit: mine.clone(),
                invocation_id: mine_id,
            },
            UnitRecord {
                unit: foreign.clone(),
                invocation_id: "ffffffffffffffffffffffffffffffff".into(),
            },
        ],
    )
    .unwrap();
    let decision = cleanup_leftovers(&control, &records);
    assert!(decision.stop.contains(&mine), "{decision:?}");
    assert!(!active(&mine), "the recorded unit was stopped");
    assert!(active(&foreign), "a unit with a foreign invocation id is left alone");
    assert!(!records.exists(), "the records are forgotten");
    systemctl(&["stop", &format!("{foreign}.service")]);
    assert!(!active(&foreign));
}
