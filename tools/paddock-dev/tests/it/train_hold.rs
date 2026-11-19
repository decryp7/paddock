//! The train-CI hold with the real scripts and the real binary, on a lock folder of the test's own (ER-14): a held lock passes the
//! proof of `train release` (`train hold-check`), the same command without the hold is refused, and a hold whose script was killed
//! (no exit trap) is taken over and released by the explicit release that `train release` makes after ending a held child.
#![allow(clippy::unwrap_used)]
#[cfg(unix)]
use paddock_testkit::TestCommand;
#[cfg(unix)]
use std::path::{Path, PathBuf};

#[cfg(unix)]
fn repo() -> PathBuf {
    paddock_env::winpath::real(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap()
}

/// `bash scripts/train-ci-hold.sh <label> -- <argv...>` on `root`, with this binary as the lock tool.
#[cfg(unix)]
fn held(root: &Path, label: &str, argv: &[&str]) -> std::process::Output {
    let mut cmd = TestCommand::new("bash");
    cmd.arg(repo().join("scripts/train-ci-hold.sh"))
        .arg(label)
        .arg("--")
        .args(argv)
        .env("PADDOCK_TRAIN_CI_LOCK_ROOT", root)
        .env("PADDOCK_DEV", crate::dev_bin::PATH)
        .env("PADDOCK_SCCACHE", "off");
    cmd.output().unwrap()
}

#[cfg(unix)]
#[test]
fn a_real_held_lock_passes_the_proof_and_the_same_command_without_the_hold_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("train-ci");
    // Positive control: under the hold the proof passes.
    let ok = held(&root, "hold-check", &[crate::dev_bin::PATH, "train", "hold-check"]);
    assert!(
        ok.status.success(),
        "{}{}",
        String::from_utf8_lossy(&ok.stdout),
        String::from_utf8_lossy(&ok.stderr)
    );
    // Without it (and with a variable that names no live holder) it is refused.
    let mut cmd = crate::dev_bin::command();
    cmd.args(["train", "hold-check"])
        .env("PADDOCK_TRAIN_CI_LOCK_ROOT", &root)
        .env("PADDOCK_VM_LOCK_ROOT", &root)
        .env("PADDOCK_VM_LOCK_HELD", "999999\tMon Jan 1 00:00:00 2001\t/nonexistent/owner");
    let out = cmd.output().unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("::error::train hold-check") && err.contains("not proven"), "{err}");
}

#[cfg(unix)]
#[test]
fn a_hold_whose_script_was_killed_is_taken_over_by_the_release_and_leaves_the_lock_free() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("train-ci");
    // The command under the hold kills the script that holds the lock (its parent) without letting its exit trap run.
    let killed = held(&root, "killed", &["bash", "-c", "kill -KILL $PPID; sleep 1"]);
    assert!(!killed.status.success(), "the held script was killed");
    // The release `train release` makes: acquiring the same lock takes the dead holder over, runs `true` and releases it.
    let released = held(&root, "train-release-cleanup", &["true"]);
    assert!(released.status.success(), "{}", String::from_utf8_lossy(&released.stderr));
    // And a following member's wait sees the reservation free: a fresh hold is taken at once.
    let again = held(&root, "next", &["true"]);
    assert!(again.status.success(), "{}", String::from_utf8_lossy(&again.stderr));
}
