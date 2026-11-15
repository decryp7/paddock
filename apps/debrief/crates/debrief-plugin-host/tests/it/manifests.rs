//! `load_manifests`: it returns when the plugins binary exits, not on the next tick of a poll, and a binary that never
//! answers is stopped at the bound.

use debrief_plugin_host::builtins::{load_manifests, load_manifests_within, MANIFESTS_FLAG};
use std::path::Path;
use std::time::{Duration, Instant};

const FAKE: &str = env!("CARGO_BIN_EXE_debrief-plugin-host-fake");

/// How long the fake's own `--manifests` run takes, waited for by blocking on its exit (no poll): the baseline.
fn own_run(exe: &Path) -> Duration {
    let started = Instant::now();
    let out = paddock_proc::command(exe).arg(MANIFESTS_FLAG).output().unwrap();
    let took = started.elapsed();
    assert!(out.status.success(), "{out:?}");
    took
}

#[test]
fn load_manifests_returns_when_the_plugins_exit_not_after_a_poll() {
    // Each call against a direct run of the same binary in the same moment (interleaved, so load hits both alike); the
    // fastest of each. A wait that sleeps 20 ms before it looks again never returns sooner than 20 ms after the start,
    // while the fake's own run takes a few milliseconds: then the fastest call trails the fastest run by about the poll.
    // The margin is half that poll.
    let exe = Path::new(FAKE);
    let (mut baseline, mut call) = (Duration::MAX, Duration::MAX);
    for _ in 0..10 {
        baseline = baseline.min(own_run(exe));
        let started = Instant::now();
        let manifests = load_manifests(exe).unwrap();
        call = call.min(started.elapsed());
        assert_eq!(manifests.len(), 1);
    }
    assert!(
        call < baseline + Duration::from_millis(10),
        "load_manifests returned {call:?} after the start against {baseline:?} for the plugins' own run: it notices the \
         exit late (a poll), not when it happens"
    );
}

#[test]
fn plugins_that_never_describe_themselves_are_stopped_at_the_bound() {
    let dir = tempfile::tempdir().unwrap();
    let name = format!("debrief-plugin-host-fake-hang{}", std::env::consts::EXE_SUFFIX);
    let hang = dir.path().join(name);
    std::fs::copy(FAKE, &hang).unwrap();
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
