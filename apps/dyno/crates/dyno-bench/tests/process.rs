#![allow(clippy::unwrap_used, clippy::expect_used)]
//! The real binary: it prints one JSON status, refuses bad arguments and leaves no scratch file behind.

use dyno_core::bench::BenchStatus;
use std::process::Command;

#[allow(clippy::disallowed_methods)]
fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dyno-bench"))
}

#[test]
fn version_prints_and_exits_zero_for_the_bundle_check() {
    let out = bin().arg("--version").output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("dyno-bench "));
}

#[test]
fn cpu_prints_a_done_status() {
    let out = bin().args(["cpu", "--millis", "20"]).output().unwrap();
    assert!(out.status.success());
    let s: BenchStatus = serde_json::from_slice(&out.stdout).unwrap();
    assert!(matches!(s, BenchStatus::Done { metrics } if metrics.len() == 3));
}

#[test]
fn gpu_is_reported_unavailable_with_a_reason() {
    let out = bin().arg("gpu").output().unwrap();
    let s: BenchStatus = serde_json::from_slice(&out.stdout).unwrap();
    assert!(matches!(s, BenchStatus::Unavailable { reason } if !reason.is_empty()));
}

#[test]
fn disk_needs_a_folder_and_cleans_up() {
    let out = bin().args(["disk"]).output().unwrap();
    let s: BenchStatus = serde_json::from_slice(&out.stdout).unwrap();
    assert!(matches!(s, BenchStatus::Failed { .. }));
    let dir = tempfile::tempdir().unwrap();
    let out = bin()
        .args(["disk", "--disk-mib", "2", "--millis", "20", "--dir"])
        .arg(dir.path())
        .output()
        .unwrap();
    let s: BenchStatus = serde_json::from_slice(&out.stdout).unwrap();
    assert!(matches!(s, BenchStatus::Done { .. }));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn bad_arguments_exit_with_usage() {
    let out = bin().arg("bogus").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage"));
}
