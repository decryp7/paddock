//! The real GPU runner: mechanics only, never a speed (ADR 0030 section 5). On a machine with a graphics adapter the smoke run
//! ends Done with finite, positive numbers; without one it ends Unavailable with a reason. Either way it writes JSON lines with
//! one `result` last, and `--list` prints the adapters as one JSON line.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use dyno_core::bench::wire::{parse_line, Line};
use dyno_core::bench::BenchStatus;
use std::process::Command;

// The runner under test, started on purpose (the family's process helpers are for the agent, not for this test).
#[allow(clippy::disallowed_methods)]
fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dyno-bench-gpu"))
}

#[test]
fn a_smoke_run_ends_with_one_result_that_is_done_or_unavailable_with_a_reason() {
    let out = bin().arg("--smoke").output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<Line> = text.lines().enumerate().map(|(i, l)| parse_line(l, i + 1).unwrap()).collect();
    let Some(Line::Result { status }) = lines.last() else {
        panic!("no result last: {text}")
    };
    assert_eq!(lines.iter().filter(|l| matches!(l, Line::Result { .. })).count(), 1);
    match status {
        BenchStatus::Done { metrics } => assert!(metrics.iter().all(|m| m.value.is_finite() && m.value > 0.0), "{metrics:?}"),
        BenchStatus::Unavailable { reason } => assert!(!reason.is_empty()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_adapter_list_is_one_json_array() {
    let out = bin().arg("--list").output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v.is_array());
}

#[test]
fn bad_arguments_are_refused() {
    assert_eq!(bin().arg("--bogus").output().unwrap().status.code(), Some(2));
    assert_eq!(bin().args(["--adapter", "x"]).output().unwrap().status.code(), Some(2));
}
