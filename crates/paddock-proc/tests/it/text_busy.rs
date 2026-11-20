//! Starting a program that something still holds open for writing (a child forked in the middle of its write, CI run 8174's
//! class): every start of this crate retries `ETXTBSY` for a bounded time instead of reporting a crash. Linux only: macOS lets
//! such a file run, which the precondition below would show.
#![cfg(target_os = "linux")]

use paddock_env::text_busy::{is_text_busy, retries_for};
use paddock_proc::{Limits, Supervised, run_bounded};
use std::fs::OpenOptions;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

/// A copy of `echo` of its own (a program the kernel maps, not a script), and a write handle on it that is let go when the first retry has happened (or after the hang guard).
fn held_program() -> (tempfile::TempDir, PathBuf, std::thread::JoinHandle<()>) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("prog");
    std::fs::copy("/bin/echo", &path).unwrap();
    let f = OpenOptions::new().write(true).open(&path).unwrap();
    // The precondition (ER-33): while the handle is held, a plain start really fails with ETXTBSY.
    let raw = paddock_proc::command(&path).stdin(Stdio::null()).spawn();
    assert!(
        raw.as_ref().is_err_and(is_text_busy),
        "the held write handle must make a plain start fail with ETXTBSY, got {raw:?}"
    );
    // The label `spawn_retrying` logs for this program: only a retry of this very start lets the handle go.
    let label = format!("start {}", path.display());
    let releaser = std::thread::spawn(move || {
        let guard = Instant::now() + Duration::from_secs(20);
        while retries_for(&label) == 0 && Instant::now() < guard {
            std::thread::yield_now();
        }
        drop(f);
    });
    (dir, path, releaser)
}

#[test]
fn a_bounded_run_of_a_program_held_open_for_writing_starts_after_the_retry() {
    let (_dir, path, releaser) = held_program();
    let mut cmd = paddock_proc::command(&path);
    cmd.arg("started");
    let out = run_bounded(cmd, Limits::new(Duration::from_secs(30))).unwrap();
    releaser.join().unwrap();
    assert!(out.success(), "{out:?}");
    assert_eq!(out.stdout_text().trim(), "started");
}

#[test]
fn a_supervised_child_held_open_for_writing_starts_after_the_retry() {
    let (_dir, path, releaser) = held_program();
    let mut child = Supervised::spawn(paddock_proc::command(&path)).unwrap();
    releaser.join().unwrap();
    child.wait();
}
