#![allow(clippy::unwrap_used, clippy::expect_used)]
//! The real binary: it writes JSON lines (progress, heartbeat, one result last), refuses bad arguments and leaves no scratch file behind.

use dyno_core::bench::wire::{parse_line, Line};
use dyno_core::bench::BenchStatus;
use std::io::Write;
use std::process::{Command, Output, Stdio};

/// Every stdout line parsed the way the agent parses it; panics (a failing test) on a line the agent would refuse.
fn lines(out: &Output) -> Vec<Line> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .enumerate()
        .map(|(i, l)| parse_line(l, i + 1).unwrap_or_else(|e| panic!("{e}: {l}")))
        .collect()
}

/// The one result, which must be the last line and the only one.
fn status(out: &Output) -> BenchStatus {
    let all = lines(out);
    let results: Vec<_> = all.iter().filter(|l| matches!(l, Line::Result { .. })).collect();
    assert_eq!(results.len(), 1, "exactly one result: {all:?}");
    match all.last() {
        Some(Line::Result { status }) => status.clone(),
        other => panic!("the result is not last: {other:?}"),
    }
}

#[allow(clippy::disallowed_methods)]
fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dyno-bench"))
}

/// One repetition, no warm-up and no gap, so a test of the real binary does not wait.
const FAST: [&str; 6] = ["--reps", "1", "--warmup-ms", "0", "--gap-ms", "0"];

#[test]
fn version_prints_and_exits_zero_for_the_bundle_check() {
    let out = bin().arg("--version").output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("dyno-bench "));
}

#[test]
fn cpu_writes_progress_and_one_done_result_with_every_repetition() {
    let out = bin()
        .args(["cpu", "--millis", "20", "--reps", "2", "--warmup-ms", "10", "--gap-ms", "0"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let all = lines(&out);
    // Warm-up (repetition 0) then two repetitions, each with its single-core and all-cores phase.
    let progress: Vec<_> = all
        .iter()
        .filter_map(|l| {
            if let Line::Progress(p) = l {
                Some((p.phase.as_str(), p.rep, p.of))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        progress,
        vec![
            ("cpu-single", 0, 2),
            ("cpu-multi", 0, 2),
            ("cpu-single", 1, 2),
            ("cpu-multi", 1, 2),
            ("cpu-single", 2, 2),
            ("cpu-multi", 2, 2)
        ]
    );
    assert!(all.iter().any(|l| matches!(l, Line::Heartbeat)), "the heartbeat starts at once");
    assert!(matches!(status(&out), BenchStatus::Done { metrics } if metrics.len() == 3 && metrics.iter().all(|m| m.samples.len() == 2)));
}

#[test]
fn gpu_is_reported_unavailable_with_a_reason() {
    let out = bin().arg("gpu").output().unwrap();
    assert!(matches!(status(&out), BenchStatus::Unavailable { reason } if !reason.is_empty()));
}

#[test]
fn disk_needs_a_folder_and_cleans_up() {
    let out = bin().args(["disk"]).output().unwrap();
    assert!(matches!(status(&out), BenchStatus::Failed { .. }));
    let dir = tempfile::tempdir().unwrap();
    let out = bin()
        .args(["disk", "--disk-mib", "2", "--millis", "20", "--dir"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(matches!(status(&out), BenchStatus::Done { .. }));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn wait_go_measures_only_after_the_agent_says_go() {
    let mut child = bin()
        .args(["cpu", "--millis", "10"])
        .args(FAST)
        .arg("--wait-go")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"go\n").unwrap();
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    assert!(matches!(status(&out), BenchStatus::Done { .. }));
}

#[test]
fn wait_go_without_go_fails_instead_of_measuring() {
    // stdin is closed at once: the agent never said go.
    let out = bin()
        .args(["cpu", "--millis", "10"])
        .args(FAST)
        .arg("--wait-go")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(matches!(status(&out), BenchStatus::Failed { reason } if reason.contains("go")));
    assert!(!lines(&out).iter().any(|l| matches!(l, Line::Progress(_))), "nothing was measured");
}

#[test]
fn bad_arguments_exit_with_usage() {
    let out = bin().arg("bogus").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage"));
}
