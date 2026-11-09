//! Mixed shell/Rust runs on ONE lock folder: the reference shell plock (tests/fixtures/plock.sh, a copy of the script heavy
//! replaces) and the Rust binary must hold, wait, queue, break and read each other's records. macOS only: the script uses
//! BSD `stat -f`, `sysctl` and perl. Every wait is bounded.
#![cfg(target_os = "macos")]

use crate::procutil::*;
use paddock_proc::command;
use std::fs;
use std::process::Stdio;

fn script() -> String {
    format!("{}/tests/fixtures/plock.sh", env!("CARGO_MANIFEST_DIR"))
}

fn shell(w: &World, args: &[&str], env: &[(&str, &str)]) -> Guard {
    let mut c = command("bash");
    c.arg(script());
    base_env(&mut c, w);
    c.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in env {
        c.env(k, v);
    }
    Guard {
        child: c.spawn().expect("spawn bash"),
        label: format!("plock {args:?}"),
    }
}

fn strs<'a>(v: &'a [&'a str]) -> Vec<&'a str> {
    v.to_vec()
}

#[test]
fn a_shell_holder_and_a_rust_waiter_in_order() {
    let w = World::new();
    let (flag, out) = (w.flag(), w.path("out"));
    let (flag, out) = (flag.to_str().unwrap(), out.to_str().unwrap());
    let mut a = shell(&w, &strs(&hold_args("SA", flag, out)), &[]);
    wait_until("shell holds", || w.locked());
    let mut b = heavy(&w, &echo_args("RB", out));
    wait_until("rust queued", || w.count("pitcrew-heavy.ticket.") >= 1);
    let mut c = shell(&w, &strs(&echo_args("SC", out)), &[]);
    wait_until("shell queued", || w.count("pitcrew-heavy.ticket.") >= 2);
    let mut d = heavy(&w, &echo_args("RD", out));
    wait_until("rust queued 2", || w.count("pitcrew-heavy.ticket.") >= 3);
    w.release();
    for g in [&mut a, &mut b, &mut c, &mut d] {
        assert!(g.finish().success(), "{}", g.label);
    }
    assert_eq!(w.out_words(), ["SA", "RB", "SC", "RD"], "one FIFO across both implementations");
}

#[test]
fn a_rust_holder_and_shell_waiters_with_status_read_by_both() {
    let w = World::new();
    let (flag, out) = (w.flag(), w.path("out"));
    let (flag, out) = (flag.to_str().unwrap(), out.to_str().unwrap());
    let mut a = heavy(&w, &hold_args("RA", flag, out));
    wait_until("rust holds", || w.locked());
    let mut b = shell(&w, &strs(&echo_args("SB", out)), &[]);
    wait_until("shell notes the holder", || {
        w.read(w.root().join("pitcrew-lock-activity.log")).contains("held by RA")
    });
    let mut st = shell(&w, &["status"], &[]);
    assert!(st.finish().success());
    let mut o = String::new();
    use std::io::Read;
    st.child.stdout.take().unwrap().read_to_string(&mut o).unwrap();
    assert!(
        o.contains("held by RA pid") && o.contains("(alive: yes)") && o.contains("command: sh -c"),
        "shell status of a Rust lock: {o}"
    );
    let (_, o2, _) = heavy_out(&w, &["status"], &[]);
    assert!(
        o2.contains("held by RA pid") && o2.contains("queued: SB"),
        "rust status sees the shell ticket: {o2}"
    );
    w.release();
    assert!(a.finish().success() && b.finish().success());
    assert_eq!(w.out_words(), ["RA", "SB"]);
}

#[test]
fn high_priority_crosses_implementations_both_ways() {
    for (holder_shell, high_shell) in [(true, false), (false, true)] {
        let w = World::new();
        let (flag, out) = (w.flag(), w.path("out"));
        let (flag, out) = (flag.to_str().unwrap(), out.to_str().unwrap());
        let mut a = if holder_shell {
            shell(&w, &strs(&hold_args("A", flag, out)), &[])
        } else {
            heavy(&w, &hold_args("A", flag, out))
        };
        wait_until("A holds", || w.locked());
        // the normal waiter is of the other implementation than the high one
        let mut b = if high_shell {
            heavy_env(&w, &echo_args("B", out), &[("PLOCK_POLL", "0.1")])
        } else {
            shell(&w, &strs(&echo_args("B", out)), &[("PLOCK_POLL", "0.1")])
        };
        wait_until("B queued", || w.count("pitcrew-heavy.ticket.") >= 1);
        let mut hargs = echo_args("H", out);
        hargs.insert(1, "--high");
        let mut h = if high_shell {
            shell(&w, &strs(&hargs), &[("PLOCK_POLL", "1.5")])
        } else {
            heavy_env(&w, &hargs, &[("PLOCK_POLL", "1.5")])
        };
        wait_until("H marker", || w.count("pitcrew-heavy.high.") >= 1);
        w.release();
        for g in [&mut a, &mut b, &mut h] {
            assert!(g.finish().success(), "{}", g.label);
        }
        assert_eq!(
            w.out_words(),
            ["A", "H", "B"],
            "holder_shell={holder_shell} high_shell={high_shell}"
        );
    }
}

#[test]
fn each_implementation_breaks_the_other_ones_dead_lock() {
    for rust_writes in [true, false] {
        let w = World::new();
        let dead = dead_pid();
        fs::create_dir(w.lock_dir()).unwrap();
        fs::write(
            w.lock_dir().join("owner"),
            format!("{dead}\nx\nghost\n{}\n/tmp\nsleep 1\n", now_secs()),
        )
        .unwrap();
        let out = w.path("out");
        let mut g = if rust_writes {
            shell(&w, &strs(&echo_args("Z", out.to_str().unwrap())), &[])
        } else {
            heavy(&w, &echo_args("Z", out.to_str().unwrap()))
        };
        assert!(g.finish().success());
        assert!(
            w.read(w.root().join("pitcrew-lock-breaks.log"))
                .contains("Z broke a stale lock of ghost")
        );
        assert_eq!(w.out_words(), ["Z"]);
    }
}

#[test]
fn a_live_holder_of_one_implementation_is_never_broken_by_the_other_across_tz_and_locale() {
    for (holder_shell, t1, l1, t2, l2) in [
        (true, "UTC", "de_DE.UTF-8", "Asia/Tokyo", "C"),
        (false, "America/Los_Angeles", "fr_FR.UTF-8", "UTC", "en_US.UTF-8"),
    ] {
        let w = World::new();
        let (flag, out) = (w.flag(), w.path("out"));
        let (flag, out) = (flag.to_str().unwrap(), out.to_str().unwrap());
        let e1 = [("TZ", t1), ("LC_ALL", l1)];
        let e2 = [("TZ", t2), ("LC_ALL", l2)];
        let mut h = if holder_shell {
            shell(&w, &strs(&hold_args("Holder", flag, out)), &e1)
        } else {
            heavy_env(&w, &hold_args("Holder", flag, out), &e1)
        };
        wait_until("holder", || w.locked());
        let mut wt = if holder_shell {
            heavy_env(&w, &echo_args("Waiter", out), &e2)
        } else {
            shell(&w, &strs(&echo_args("Waiter", out)), &e2)
        };
        wait_until("waiter notes", || {
            w.read(w.root().join("pitcrew-lock-activity.log")).contains("Waiter waiting")
        });
        assert!(w.lock_dir().exists() && w.out().is_empty());
        assert!(!w.read(w.root().join("pitcrew-lock-breaks.log")).contains("broke"));
        w.release();
        assert!(h.finish().success() && wt.finish().success());
    }
}

#[test]
fn a_recycled_pid_written_by_rust_is_broken_by_the_shell_and_the_reverse() {
    for shell_breaks in [true, false] {
        let w = World::new();
        let live = sleeper();
        fs::create_dir(w.lock_dir()).unwrap();
        fs::write(
            w.lock_dir().join("owner"),
            format!("{}\nx\nreused\n{}\n", live.pid(), now_secs() - 100),
        )
        .unwrap();
        let out = w.path("out");
        let mut g = if shell_breaks {
            shell(&w, &strs(&echo_args("Z", out.to_str().unwrap())), &[])
        } else {
            heavy(&w, &echo_args("Z", out.to_str().unwrap()))
        };
        assert!(g.finish().success());
        assert!(
            w.read(w.root().join("pitcrew-lock-breaks.log"))
                .contains("Z broke a stale lock of reused")
        );
    }
}

#[test]
fn a_dead_ticket_and_marker_of_the_other_implementation_are_removed() {
    for shell_cleans in [true, false] {
        let w = World::new();
        let dead = dead_pid();
        fs::write(
            w.root().join(format!("pitcrew-heavy.ticket.0000000000000001-{dead}")),
            format!("{dead}\nx\nghost\n{}\n", now_secs()),
        )
        .unwrap();
        fs::write(
            w.root().join(format!("pitcrew-heavy.high.{dead}-m")),
            format!("{dead}\nx\n{}\n", now_secs()),
        )
        .unwrap();
        let out = w.path("out");
        let mut g = if shell_cleans {
            shell(&w, &strs(&echo_args("N", out.to_str().unwrap())), &[])
        } else {
            heavy(&w, &echo_args("N", out.to_str().unwrap()))
        };
        assert!(g.finish().success());
        assert_eq!(w.count("pitcrew-heavy.ticket."), 0);
        assert_eq!(w.count("pitcrew-heavy.high."), 0);
    }
}

#[test]
fn the_old_style_lock_blocks_both() {
    let w = World::new();
    fs::create_dir(w.root().join("pitcrew-heavy.lock")).unwrap();
    let out = w.path("out");
    let mut s = shell(&w, &strs(&echo_args("S", out.to_str().unwrap())), &[]);
    let mut r = heavy(&w, &echo_args("R", out.to_str().unwrap()));
    wait_until("both notice", || {
        let a = w.read(w.root().join("pitcrew-lock-activity.log"));
        a.contains("S waiting") && a.contains("R waiting")
    });
    assert!(w.out().is_empty());
    fs::remove_dir(w.root().join("pitcrew-heavy.lock")).unwrap();
    assert!(s.finish().success() && r.finish().success());
    assert_eq!(w.out_words().len(), 2);
}
