//! A test leaves no holder behind (ER-14): the holders the tests start are released when their world ends, a panicking test included,
//! and a holder whose folder is gone or that has waited for ever ends by itself. Unix only: the holders are `sh` loops.
#![cfg(unix)]

use crate::procutil::*;
use paddock_testkit::TestCommand;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

// The leak: a test that panicked before its `release()` left `sh -c 'while [ ! -e "$0" ]...'` polling with parent pid 1 for ever.
#[test]
fn a_test_that_panics_before_releasing_its_holder_leaves_no_holder() {
    let mut dir = PathBuf::new();
    let r = catch_unwind(AssertUnwindSafe(|| {
        let w = World::new();
        dir = w.dir.path().to_path_buf();
        let (flag, out) = (w.flag(), w.path("out"));
        let a = throttle(&w, &hold_args("A", flag.to_str().unwrap(), out.to_str().unwrap()));
        wait_until("A holds", || w.locked());
        let _keep = a;
        panic!("the test fails before it releases A");
    }));
    assert!(r.is_err());
    assert!(!dir.as_os_str().is_empty());
    // The world's own drop did the waiting; a second look names what is left (the folder is unique to this run).
    let left = leaked_in(&dir);
    assert!(left.is_empty(), "holders left alive: {left:?}");
}

// Positive control (ER-33): the check sees a process that mentions the folder and ignores the flag, and sees it gone afterwards.
#[test]
fn the_leak_check_sees_a_process_that_ignores_the_flag() {
    let w = World::new();
    let marker = w.path("marker");
    let mut g = Guard {
        child: TestCommand::new("sh")
            .args(["-c", "read line; :", marker.to_str().unwrap()])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .expect("spawn sleep"),
        label: "sleeper that ignores the flag".into(),
    };
    wait_until("the check sees it", || !leaked_in(w.dir.path()).is_empty());
    g.child.kill().expect("kill");
    g.wait(DEADLINE).expect("ended");
    wait_until("the check sees it gone", || leaked_in(w.dir.path()).is_empty());
}

// The loop itself is bounded: its folder deleted, a holder ends without the flag and writes nothing.
#[test]
fn a_holder_ends_by_itself_when_its_folder_is_gone() {
    let w = World::new();
    let sub = w.path("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let (flag, out) = (sub.join("flag"), w.path("out"));
    let mut holder = hold(&flag, &out, &[]);
    std::fs::remove_dir_all(&sub).unwrap();
    let st = holder.wait(DEADLINE).expect("ended after its folder went");
    assert!(!st.success(), "gave up, not released");
    assert!(!out.exists(), "a holder that gave up writes nothing");
}

// The lifetime limit: a holder with a present folder and no flag gives up after HOLD_MAX polls and writes nothing.
#[test]
fn a_holder_gives_up_after_its_maximum_lifetime() {
    let w = World::new();
    let (flag, out) = (w.flag(), w.path("out"));
    let mut h = hold(&flag, &out, &[("HOLD_MAX", "3")]);
    let st = h.wait(DEADLINE).expect("gave up");
    assert!(!st.success(), "gave up, not released");
    assert!(!out.exists(), "a holder that gave up writes nothing");
}

// A released holder still writes its name (the normal path of every other test).
#[test]
fn a_released_holder_writes_its_name() {
    let w = World::new();
    let (flag, out) = (w.flag(), w.path("out"));
    let mut h = hold(&flag, &out, &[]);
    w.release();
    assert!(h.wait(DEADLINE).expect("released").success());
    assert_eq!(w.out_words(), ["H"]);
}

fn hold(flag: &std::path::Path, out: &std::path::Path, env: &[(&str, &str)]) -> Guard {
    let mut c = TestCommand::new("sh");
    c.args(["-c", HOLD, flag.to_str().unwrap(), out.to_str().unwrap(), "H"]);
    for (k, v) in env {
        c.env(k, v);
    }
    Guard {
        child: c.spawn().expect("spawn sh"),
        label: "holder".into(),
    }
}
