//! The shell plock.test.sh checks, ported onto the fake clock and fake process table. Names follow the shell checks.

use crate::helpers::*;
use heavy::lock::{Acquirer, Poll};
use heavy::mask::mask_cmd;
use std::fs;
use std::sync::atomic::Ordering;

// 1. two contenders are served in order, whatever their poll rates
#[test]
fn contenders_are_served_in_order() {
    let rig = Rig::new();
    let (a, b, c) = (rig.agent(11), rig.agent(12), rig.agent(13));
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    assert_eq!(ha.poll(), Poll::Held);
    let mut hb = Acquirer::new(&b.h, req("B")).unwrap();
    rig.clock.advance(std::time::Duration::from_millis(5));
    let mut hc = Acquirer::new(&c.h, req("C")).unwrap();
    ha.release();
    // C polls first and more often: only the queue keeps B first
    for _ in 0..5 {
        assert_eq!(hc.poll(), Poll::Waiting, "C must wait for B");
    }
    assert_eq!(hb.poll(), Poll::Held);
    assert_eq!(hc.poll(), Poll::Waiting);
    hb.release();
    assert_eq!(hc.poll(), Poll::Held);
}

// 1b. --high goes first, deterministically, three times
#[test]
fn high_goes_before_an_earlier_normal_waiter() {
    for _ in 0..3 {
        let rig = Rig::new();
        let (a, b, hh) = (rig.agent(11), rig.agent(12), rig.agent(13));
        let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
        assert_eq!(ha.poll(), Poll::Held);
        let mut hb = Acquirer::new(&b.h, req("B")).unwrap();
        let mut hhi = Acquirer::new(&hh.h, req_high("H")).unwrap();
        assert_eq!(rig.count("pitcrew-heavy.high."), 1);
        ha.release();
        assert_eq!(hb.poll(), Poll::Waiting, "B yields to the high-priority run");
        assert_eq!(hhi.poll(), Poll::Held);
        assert_eq!(rig.count("pitcrew-heavy.high."), 0, "the marker is gone once H holds");
        hhi.release();
        assert_eq!(hb.poll(), Poll::Held);
    }
}

// 2. an empty owner is never published
#[test]
fn owner_write_always_failing_gives_up_and_publishes_nothing() {
    let rig = Rig::new();
    let a = rig.agent(11);
    a.h.faults.owner_fail.store(999, Ordering::SeqCst);
    let mut acq = Acquirer::new(&a.h, req("X")).unwrap();
    let last = acquire(&mut acq, &a.h, 10);
    let Poll::GaveUp(msg) = last else {
        panic!("expected GaveUp, got {last:?}")
    };
    assert!(msg.contains("giving up"), "{msg}");
    assert!(!rig.lock_dir().exists(), "no lock dir was published");
    assert_eq!(rig.count(".plock."), 0, "temp dirs removed");
    assert!(a.said().contains("attempt 3 of 3"));
}

#[test]
fn one_failed_owner_write_is_retried_and_then_succeeds() {
    let rig = Rig::new();
    let a = rig.agent(11);
    a.h.faults.owner_fail.store(1, Ordering::SeqCst);
    let mut acq = Acquirer::new(&a.h, req("X")).unwrap();
    assert_eq!(acquire(&mut acq, &a.h, 10), Poll::Held);
}

#[test]
fn a_ticket_that_cannot_be_written_gives_up_clearly() {
    let rig = Rig::new();
    let a = rig.agent(11);
    a.h.faults.ticket_fail.store(true, Ordering::SeqCst);
    let Err(msg) = Acquirer::new(&a.h, req("X")) else {
        panic!("expected Err")
    };
    assert!(msg.contains("queue ticket"), "{msg}");
    assert!(!rig.lock_dir().exists());
}

#[test]
fn a_write_that_succeeds_but_leaves_an_empty_owner_is_never_published() {
    let rig = Rig::new();
    let a = rig.agent(11);
    a.h.faults.owner_empty.store(999, Ordering::SeqCst);
    let mut acq = Acquirer::new(&a.h, req("X")).unwrap();
    assert!(matches!(acquire(&mut acq, &a.h, 10), Poll::GaveUp(_)));
    assert!(!rig.lock_dir().exists());
}

// 2b. ownerless lock and unparsable owner
#[test]
fn ownerless_lock_is_left_alone_while_young_and_broken_after_the_limit() {
    let rig = Rig::new();
    let y = rig.agent(11);
    fs::create_dir(rig.lock_dir()).unwrap();
    let mut acq = Acquirer::new(&y.h, req("Y")).unwrap();
    assert_eq!(acq.poll(), Poll::Waiting);
    assert!(rig.lock_dir().exists(), "younger than the limit: left alone");
    rig.clock.advance(std::time::Duration::from_secs(4));
    assert_eq!(acq.poll(), Poll::Held);
    assert!(
        rig.breaks_log().contains("broke a stale lock of an unknown owner"),
        "{}",
        rig.breaks_log()
    );
}

#[test]
fn an_old_non_empty_unparsable_owner_is_never_broken_and_status_says_so() {
    let rig = Rig::new();
    let g = rig.agent(11);
    rig.plant_lock("this is not a record\n");
    rig.clock.advance(std::time::Duration::from_secs(1000));
    let mut acq = Acquirer::new(&g.h, req("G")).unwrap();
    for _ in 0..5 {
        assert_eq!(acq.poll(), Poll::Waiting);
    }
    assert!(rig.lock_dir().exists());
    assert!(g.h.status().contains("UNPARSABLE"));
    fs::remove_dir_all(rig.lock_dir()).unwrap();
    assert_eq!(acq.poll(), Poll::Held, "runs after a person removes the lock");
}

// 3. dead owner (3-line and 6-line records)
#[test]
fn dead_owners_lock_is_broken_and_logged_old_record() {
    let rig = Rig::new();
    let z = rig.agent(11);
    rig.plant_lock("99001\nMon Jan  1 00:00:00 2001\nghost\n");
    let mut acq = Acquirer::new(&z.h, req("Z")).unwrap();
    assert_eq!(acquire(&mut acq, &z.h, 5), Poll::Held);
    assert!(rig.breaks_log().contains("Z broke a stale lock of ghost"));
    assert!(
        !rig.breaks_log().contains("waiting") && !rig.breaks_log().contains("took the lock"),
        "waits and takes are not in the breaks log"
    );
    assert!(z.h.status().contains("broke a stale lock of ghost"));
}

#[test]
fn dead_owners_lock_is_broken_new_record() {
    let rig = Rig::new();
    let z = rig.agent(11);
    rig.plant_lock(&format!("99002\nx\nghost2\n{}\n/tmp\nsleep 1\n", rig.now()));
    let mut acq = Acquirer::new(&z.h, req("Z")).unwrap();
    assert_eq!(acquire(&mut acq, &z.h, 5), Poll::Held);
    assert!(rig.breaks_log().contains("Z broke a stale lock of ghost2"));
}

// 3b. a recycled pid is broken
#[test]
fn a_recycled_pid_is_broken_and_logged() {
    let rig = Rig::new();
    let z = rig.agent(11);
    rig.procs.run(99003, rig.now()); // alive, but started after the record was written
    rig.plant_lock(&format!("99003\nMon Jan  1 00:00:00 2001\nreused\n{}\n", rig.now() - 100));
    let mut acq = Acquirer::new(&z.h, req("Z")).unwrap();
    assert_eq!(acquire(&mut acq, &z.h, 5), Poll::Held);
    assert!(rig.breaks_log().contains("Z broke a stale lock of reused"));
    assert!(z.h.log.has("os start"), "the evidence names the observed start time");
}

#[test]
fn a_start_within_the_two_second_slack_is_the_same_process() {
    let rig = Rig::new();
    let z = rig.agent(11);
    rig.procs.run(99004, rig.now() + 2);
    rig.plant_lock(&format!("99004\nx\nslack\n{}\n", rig.now()));
    let mut acq = Acquirer::new(&z.h, req("Z")).unwrap();
    assert_eq!(acq.poll(), Poll::Waiting, "2 s after the record is still the writer");
    rig.procs.run(99004, rig.now() + 3);
    assert_eq!(acquire(&mut acq, &z.h, 5), Poll::Held, "3 s after is a recycled pid");
}

// 4/5/6. a live owner is not broken; wait message names the holder
#[test]
fn a_live_owner_is_not_broken_and_the_waiter_says_who_holds_it() {
    let rig = Rig::new();
    let (h, w) = (rig.agent(11), rig.agent(12));
    let mut holder = Acquirer::new(&h.h, req("Holder")).unwrap();
    assert_eq!(holder.poll(), Poll::Held);
    let mut waiter = Acquirer::new(&w.h, req("Waiter")).unwrap();
    for _ in 0..8 {
        assert_eq!(waiter.poll(), Poll::Waiting);
        rig.clock.advance(std::time::Duration::from_millis(300));
    }
    assert!(rig.lock_dir().exists());
    assert_eq!(rig.owner_line(2), "Holder");
    assert!(rig.breaks_log().is_empty(), "no break was logged");
    let said = w.said();
    assert!(said.contains("Waiter waiting") && said.contains("held by Holder"), "{said}");
    assert!(rig.activity_log().contains("Waiter waiting") && rig.activity_log().contains("held by Holder"));
    holder.release();
    assert_eq!(waiter.poll(), Poll::Held);
}

// 7. liveness does not depend on the start text (TZ / locale formatted) at all
#[test]
fn liveness_ignores_the_start_text() {
    let rig = Rig::new();
    let w = rig.agent(12);
    for text in [
        "Sat 31 Oct 23:29:50 2026",
        "sam. 31 oct. 23:29:50 2026",
        "2026年10月31日",
        "",
        "UTC+9 whatever",
    ] {
        let _ = fs::remove_dir_all(rig.lock_dir());
        rig.procs.run(21, rig.now() - 50);
        rig.plant_lock(&format!("21\n{text}\nHolder\n{}\n", rig.now() - 10));
        let mut acq = Acquirer::new(&w.h, req("Waiter")).unwrap();
        for _ in 0..3 {
            assert_eq!(acq.poll(), Poll::Waiting, "start text {text:?}");
        }
        assert!(rig.breaks_log().is_empty(), "start text {text:?}");
    }
}

// 7b. a process whose start time cannot be read counts as alive
#[test]
fn an_unreadable_start_time_is_not_a_dead_holder() {
    let rig = Rig::new();
    let w = rig.agent(12);
    rig.procs.run_opaque(31);
    rig.plant_lock(&format!("31\n\nHolder\n{}\n", rig.now() - 100));
    let mut acq = Acquirer::new(&w.h, req("Waiter")).unwrap();
    for _ in 0..3 {
        assert_eq!(acq.poll(), Poll::Waiting);
    }
    assert!(rig.breaks_log().is_empty());
}

#[test]
fn a_holder_that_cannot_read_its_own_start_writes_an_empty_start_text() {
    let rig = Rig::new();
    let h = rig.agent(40);
    rig.procs.run_opaque(40);
    let mut a = Acquirer::new(&h.h, req("H")).unwrap();
    assert_eq!(a.poll(), Poll::Held);
    assert_eq!(rig.owner_line(1), "");
    let w = rig.agent(41);
    let mut b = Acquirer::new(&w.h, req("W")).unwrap();
    assert_eq!(b.poll(), Poll::Waiting);
    assert!(rig.breaks_log().is_empty());
}

// 8. things vanishing never crash a waiter
#[test]
fn a_lock_that_vanishes_mid_check_does_not_crash() {
    let rig = Rig::new();
    let w = rig.agent(12);
    // the whole lock dir vanishes between the decision and the rename
    rig.plant_lock("99010\nx\nghost\n1\n");
    let dir = rig.lock_dir();
    *w.h.faults.before_aside.lock().unwrap() = Some(Box::new(move || {
        let _ = fs::remove_dir_all(dir);
    }));
    assert!(!w.h.break_if_stale(&rig.lock_dir(), "W"), "nothing to break, and no panic");
    // break_if_stale on a path that never existed
    assert!(!w.h.break_if_stale(&rig.root().join("nope"), "W"));
    // read_owner of a missing dir is Empty, not an error
    assert_eq!(w.h.read_owner(&rig.root().join("nope")), heavy::record::OwnerRead::Empty);
    let mut acq = Acquirer::new(&w.h, req("W")).unwrap();
    assert_eq!(acq.poll(), Poll::Held);
}

#[test]
fn old_three_line_record_without_epoch_uses_the_file_time_and_an_unknown_time_reads_fine() {
    let rig = Rig::new();
    let w = rig.agent(12);
    rig.procs.run(51, rig.now() - 1000);
    rig.plant_lock("51\nstart\noldagent\n");
    let mut acq = Acquirer::new(&w.h, req("W")).unwrap();
    assert_eq!(acq.poll(), Poll::Waiting);
    assert!(w.h.status().contains("held by oldagent"));
    assert_eq!(heavy::timefmt::mins(10, None), "an unknown time");
}

#[test]
fn an_empty_owner_lock_status_reports_the_age() {
    let rig = Rig::new();
    let w = rig.agent(12);
    fs::create_dir(rig.lock_dir()).unwrap();
    assert!(w.h.status().contains("empty owner record"));
}

// 8c. EPERM counts as alive
#[test]
fn a_pid_we_may_not_signal_is_alive() {
    let rig = Rig::new();
    let w = rig.agent(12);
    rig.procs.run_opaque(1);
    rig.plant_lock(&format!("1\nx\ninit\n{}\n", rig.now() - 100));
    let mut acq = Acquirer::new(&w.h, req("W")).unwrap();
    for _ in 0..3 {
        assert_eq!(acq.poll(), Poll::Waiting);
    }
    assert!(rig.lock_dir().exists() && rig.breaks_log().is_empty());
}

// 9. old-style lock and old-format live record
#[test]
fn the_old_style_lock_is_honoured() {
    let rig = Rig::new();
    let w = rig.agent(12);
    fs::create_dir(rig.root().join("pitcrew-heavy.lock")).unwrap();
    let mut acq = Acquirer::new(&w.h, req("W")).unwrap();
    for _ in 0..3 {
        assert_eq!(acq.poll(), Poll::Waiting);
        rig.clock.advance(std::time::Duration::from_secs(1));
    }
    assert!(w.said().contains("old-style lock"));
    assert!(w.h.status().contains("old-style lock"));
    fs::remove_dir(rig.root().join("pitcrew-heavy.lock")).unwrap();
    assert_eq!(acq.poll(), Poll::Held);
}

#[test]
fn a_live_old_format_lock_is_kept_and_broken_once_its_owner_is_gone() {
    let rig = Rig::new();
    let w = rig.agent(12);
    rig.procs.run(61, rig.now() - 500);
    rig.plant_lock("61\nMon Jan  1 00:00:00 2001\noldagent\n");
    let mut acq = Acquirer::new(&w.h, req("W")).unwrap();
    assert_eq!(acq.poll(), Poll::Waiting);
    assert!(rig.breaks_log().is_empty());
    rig.procs.end(61);
    assert_eq!(acquire(&mut acq, &w.h, 5), Poll::Held);
    assert!(rig.breaks_log().contains("broke a stale lock of oldagent"));
}

// 10. markers and tickets
#[test]
fn a_high_marker_of_a_recycled_pid_is_removed_and_does_not_block() {
    let rig = Rig::new();
    let n = rig.agent(12);
    rig.procs.run(71, rig.now());
    let m = rig.root().join("pitcrew-heavy.high.71-old");
    fs::write(&m, format!("71\nx\n{}\n", rig.now() - 100)).unwrap();
    let mut acq = Acquirer::new(&n.h, req("N")).unwrap();
    assert_eq!(acq.poll(), Poll::Held);
    assert!(!m.exists());
}

#[test]
fn a_live_high_marker_makes_a_normal_run_wait_until_that_process_is_gone() {
    let rig = Rig::new();
    let n = rig.agent(12);
    rig.procs.run(72, rig.now() - 50);
    let m = rig.root().join("pitcrew-heavy.high.72-real");
    fs::write(&m, format!("72\nx\n{}\n", rig.now())).unwrap();
    let mut acq = Acquirer::new(&n.h, req("N")).unwrap();
    for _ in 0..3 {
        assert_eq!(acq.poll(), Poll::Waiting);
        rig.clock.advance(std::time::Duration::from_secs(1));
    }
    assert!(n.said().contains("high-priority"));
    rig.procs.end(72);
    assert_eq!(acq.poll(), Poll::Held);
}

#[test]
fn a_ticket_of_a_recycled_pid_does_not_block_and_is_removed() {
    let rig = Rig::new();
    let n = rig.agent(12);
    rig.procs.run(81, rig.now());
    fs::write(
        rig.root().join("pitcrew-heavy.ticket.0000000000000001-81"),
        format!("81\nx\nghost\n{}\n", rig.now() - 100),
    )
    .unwrap();
    let mut acq = Acquirer::new(&n.h, req("N")).unwrap();
    assert_eq!(acq.poll(), Poll::Held);
    assert_eq!(rig.count("pitcrew-heavy.ticket."), 0);
}

#[test]
fn a_marker_with_spaces_in_its_name_is_removed_whole_and_unrelated_files_survive() {
    let rig = Rig::new();
    let n = rig.agent(12);
    fs::write(rig.root().join("pitcrew-heavy.high.99999999-x y z"), "99999999\n").unwrap();
    for f in ["x", "y", "z"] {
        fs::write(rig.root().join(f), "").unwrap();
    }
    let mut acq = Acquirer::new(&n.h, req("N")).unwrap();
    assert_eq!(acq.poll(), Poll::Held);
    assert!(!rig.root().join("pitcrew-heavy.high.99999999-x y z").exists());
    for f in ["x", "y", "z"] {
        assert!(rig.root().join(f).exists(), "{f} must survive");
    }
}

#[test]
fn unreadable_and_vanishing_tickets_do_not_crash_or_block_forever() {
    let rig = Rig::new();
    let n = rig.agent(12);
    // a directory with a ticket's name cannot be read: skipped, no panic
    fs::create_dir(rig.root().join("pitcrew-heavy.ticket.0000000000000000-1")).unwrap();
    // an empty ticket: only age may remove it
    fs::write(rig.root().join("pitcrew-heavy.ticket.0000000000000002-2"), "").unwrap();
    let mut acq = Acquirer::new(&n.h, req("N")).unwrap();
    assert_eq!(acq.poll(), Poll::Waiting, "a young unreadable ticket still counts as ahead");
    rig.clock.advance(std::time::Duration::from_secs(4));
    assert_eq!(acq.poll(), Poll::Held);
    assert!(rig.breaks_log().contains("removed the unreadable ticket"));
}

// 11. the owner record and the masked command
#[test]
fn the_owner_record_has_six_lines() {
    let rig = Rig::new();
    let a = rig.agent(11);
    let mut acq = Acquirer::new(&a.h, req("Rec")).unwrap();
    assert_eq!(acq.poll(), Poll::Held);
    let text = fs::read_to_string(rig.lock_dir().join("owner")).unwrap();
    assert_eq!(text.lines().count(), 6);
    assert_eq!(rig.owner_line(0), "11");
    assert_eq!(rig.owner_line(2), "Rec");
    assert_eq!(rig.owner_line(3), rig.now().to_string());
    assert_eq!(rig.owner_line(4), "/work");
    assert_eq!(rig.owner_line(5), "cargo test");
    assert!(rig.owner_line(1).ends_with("UTC"), "informational start text, locale free");
}

fn args(v: &[&str]) -> String {
    mask_cmd(&v.iter().map(|s| (*s).to_string()).collect::<Vec<_>>())
}

#[test]
fn masking_generic() {
    let got = args(&[
        "api_key=abc",
        "--token",
        "hunter2",
        "--password=pw",
        "https://u:p@host/x",
        "-pSECRET",
        "-u",
        "joe",
        "-p",
        "crate",
        "keyboard",
        "Authorization: Bearer xyz",
        "--features",
        "key-store",
        "--x=Authorization: Bearer q",
        "GITHUB_PAT=ghp_x",
        "plain",
    ]);
    assert_eq!(
        got,
        "api_key=*** --token *** --password=*** https://***@host/x -pSECRET -u joe -p crate keyboard *** --features key-store --x=*** GITHUB_PAT=*** plain"
    );
}

#[test]
fn masking_curl() {
    assert_eq!(
        args(&["curl", "-u", "joe:pw", "-uA:B", "--user", "c:d", "-H", "X-Api-Key: k", "-o", "out"]),
        "curl -u *** -u*** --user *** -H *** -o out"
    );
}

#[test]
fn masking_mysql() {
    assert_eq!(
        args(&["mysql", "-h", "db", "-u", "root", "-pSECRET", "-p", "pw", "dbname"]),
        "mysql -h db -u root -p*** -p *** dbname"
    );
}

#[test]
fn masking_docker_login_but_not_run_ports() {
    assert_eq!(
        args(&["docker", "login", "-u", "me", "-p", "pw", "reg"]),
        "docker login -u me -p *** reg"
    );
    assert_eq!(args(&["docker", "run", "-p", "8080:80", "img"]), "docker run -p 8080:80 img");
}

#[test]
fn masking_url_with_an_at_in_the_password_and_length_and_newlines() {
    assert_eq!(
        args(&["git", "clone", "https://user:p@ss@host/repo"]),
        "git clone https://***@host/repo"
    );
    assert_eq!(args(&["a\nb", "c\td"]), "a b c d");
    assert_eq!(args(&[&"x".repeat(900)]).chars().count(), 500);
}

// 11b. break-aside race defences
#[test]
fn lock_replaced_by_a_new_dir_during_a_break_is_put_back_not_broken_not_logged() {
    let rig = Rig::new();
    let w = rig.agent(12);
    rig.procs.run(91, rig.now() - 50);
    rig.plant_lock(&format!("99020\nx\nghost\n{}\n", rig.now() - 5));
    let (dir, now) = (rig.lock_dir(), rig.now());
    *w.h.faults.before_aside.lock().unwrap() = Some(Box::new(move || {
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("owner"), format!("91\nx\nnewer\n{now}\n")).unwrap();
    }));
    let mut acq = Acquirer::new(&w.h, req("W")).unwrap();
    assert_eq!(acq.poll(), Poll::Waiting);
    assert_eq!(rig.owner_line(2), "newer");
    assert!(rig.breaks_log().is_empty());
}

#[test]
fn owner_rewritten_in_place_during_a_break_is_put_back_and_leaves_no_break_dir() {
    let rig = Rig::new();
    let w = rig.agent(12);
    rig.procs.run(92, rig.now() - 50);
    rig.plant_lock(&format!("99021\nx\nghost\n{}\n", rig.now() - 5));
    let (dir, now) = (rig.lock_dir(), rig.now());
    *w.h.faults.before_aside.lock().unwrap() = Some(Box::new(move || {
        fs::write(dir.join("owner"), format!("92\nx\nrewritten\n{now}\n")).unwrap();
    }));
    let mut acq = Acquirer::new(&w.h, req("W")).unwrap();
    assert_eq!(acq.poll(), Poll::Waiting);
    assert_eq!(rig.owner_line(2), "rewritten");
    assert!(rig.breaks_log().is_empty());
    assert_eq!(rig.count(".plock.break."), 0);
}

// 12. status
#[test]
fn status_shows_holder_and_command_then_free_and_nothing_left_behind() {
    let rig = Rig::new();
    let a = rig.agent(11);
    assert!(a.h.status().starts_with("free"));
    let mut acq = Acquirer::new(&a.h, req("S")).unwrap();
    assert_eq!(acq.poll(), Poll::Held);
    let s = a.h.status();
    assert!(
        s.contains("held by S pid 11") && s.contains("command: cargo test") && s.contains("cwd: /work"),
        "{s}"
    );
    acq.release();
    assert_eq!(a.h.status().lines().next(), Some("free"));
    assert_eq!(rig.count("pitcrew-heavy.ticket."), 0);
    assert!(!rig.lock_dir().exists());
}

#[test]
fn release_leaves_a_lock_that_is_no_longer_ours() {
    let rig = Rig::new();
    let a = rig.agent(11);
    let mut acq = Acquirer::new(&a.h, req("S")).unwrap();
    assert_eq!(acq.poll(), Poll::Held);
    fs::write(rig.lock_dir().join("owner"), format!("77\nx\nsomeone\n{}\n", rig.now())).unwrap();
    acq.release();
    assert!(rig.lock_dir().exists(), "never delete another owner's lock");
}

// 8. a file that vanishes between the listing and the read must neither crash nor count as a waiter ahead
#[cfg(unix)]
#[test]
fn a_ticket_that_vanishes_between_listing_and_reading_is_skipped() {
    let rig = Rig::new();
    let n = rig.agent(12);
    // a dangling link is listed by read_dir and fails to open: exactly what a vanished file looks like to the reader
    std::os::unix::fs::symlink(rig.root().join("gone"), rig.root().join("pitcrew-heavy.ticket.0000000000000001-1")).unwrap();
    let mut acq = Acquirer::new(&n.h, req("N")).unwrap();
    assert_eq!(acq.poll(), Poll::Held, "a vanished ticket is nobody ahead of us");
}

#[cfg(unix)]
#[test]
fn a_high_marker_that_vanishes_between_listing_and_reading_is_skipped() {
    let rig = Rig::new();
    let n = rig.agent(12);
    std::os::unix::fs::symlink(rig.root().join("gone"), rig.root().join("pitcrew-heavy.high.1-x")).unwrap();
    let mut acq = Acquirer::new(&n.h, req("N")).unwrap();
    assert_eq!(acq.poll(), Poll::Held);
}
