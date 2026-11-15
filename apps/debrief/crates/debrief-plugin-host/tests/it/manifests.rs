//! `load_manifests` against plugins binaries that misbehave: one that never answers is stopped at the bound, one that
//! writes past the cap is stopped there, one that crashes or prints garbage is reported. How fast it returns on an exit is
//! the start-up benchmark's job (apps/debrief/docs/performance.md, 2026-11-15), not a test's.

use debrief_plugin_host::builtins::{load_manifests, load_manifests_within};
use std::path::{Path, PathBuf};
use std::time::Duration;

const FAKE: &str = env!("CARGO_BIN_EXE_debrief-plugin-host-fake");

/// The fake under a name that selects `mode`, in a folder of its own under cargo's target scratch: a hard link to the
/// binary already built and run (a fresh copy is a new program to Windows Defender's first-run scan), a copy where links
/// are not possible.
fn fake_as(mode: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let exe = dir
        .path()
        .join(format!("debrief-plugin-host-fake-{mode}{}", std::env::consts::EXE_SUFFIX));
    if std::fs::hard_link(FAKE, &exe).is_err() {
        std::fs::copy(FAKE, &exe).unwrap();
    }
    (dir, exe)
}

#[test]
fn the_fake_describes_itself() {
    assert_eq!(load_manifests(Path::new(FAKE)).unwrap().len(), 1);
}

#[test]
fn plugins_that_never_describe_themselves_are_stopped_at_the_bound() {
    let (_dir, hang) = fake_as("hang");
    let err = load_manifests_within(&hang, Duration::from_secs(1)).unwrap_err();
    assert_eq!(err, "the plugins did not describe themselves in time");
    // Precondition (ER-33): the fake ran far enough to say who it was, so "gone" below means it was stopped.
    let pid: u32 = std::fs::read_to_string(hang.with_extension("pid"))
        .expect("the hanging fake did not start within the bound")
        .parse()
        .unwrap();
    // Stopped and reaped before load_manifests returned: nothing of that process is left.
    assert!(
        paddock_proc::ProcessId::of(pid).is_none(),
        "the hanging plugins binary (pid {pid}) is still there"
    );
}

#[test]
fn plugins_that_write_past_the_cap_are_stopped_there() {
    // The fake writes 5 MiB and then never exits: only the cap can end the run before the bound.
    let (_dir, huge) = fake_as("huge");
    let err = load_manifests_within(&huge, Duration::from_secs(30)).unwrap_err();
    assert_eq!(err, "the plugins wrote more than 4 MiB");
}

#[test]
fn plugins_that_crash_or_print_garbage_say_so() {
    let (_dir, crash) = fake_as("crash");
    let err = load_manifests(&crash).unwrap_err();
    assert!(err.starts_with("the plugins could not describe themselves ("), "{err}");
    let (_dir, garbage) = fake_as("garbage");
    let err = load_manifests(&garbage).unwrap_err();
    assert!(err.starts_with("the plugins' manifests are not valid: "), "{err}");
}
