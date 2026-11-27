//! The shell plock.test.sh checks, ported onto the fake clock and fake process table. Names follow the shell checks.

use crate::helpers::*;
use std::fs;
use std::sync::atomic::Ordering;
use throttle::lock::{Acquirer, Poll};
use throttle::mask::mask_cmd;

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

#[test]
fn old_three_line_record_without_epoch_uses_the_file_time_and_an_unknown_time_reads_fine() {
    let rig = Rig::new();
    let w = rig.agent(12);
    let _holder = rig.plant_holder("51\nstart\noldagent\n");
    let mut acq = Acquirer::new(&w.h, req("W")).unwrap();
    assert_eq!(acq.poll(), Poll::Waiting);
    assert!(w.h.status().contains("held by oldagent"));
    assert_eq!(throttle::timefmt::mins(10, None), "an unknown time");
}

// The slot lock is the kernel's: that a slot is held, and that it is free again, is never decided from a record.
#[test]
fn a_held_slot_whose_record_is_empty_or_unparsable_is_held_and_is_never_taken_over() {
    for record in ["", "this is not a record\n"] {
        let rig = Rig::new();
        let w = rig.agent(12);
        rig.clock.advance(std::time::Duration::from_secs(5000));
        let holder = rig.plant_holder(record);
        let mut acq = Acquirer::new(&w.h, req("W")).unwrap();
        for _ in 0..5 {
            assert_eq!(acq.poll(), Poll::Waiting, "record {record:?}");
        }
        assert!(
            w.h.status().contains("held (its holder has not written who it is yet)"),
            "{}",
            w.h.status()
        );
        assert!(rig.breaks_log().is_empty(), "nothing is broken");
        drop(holder);
        assert_eq!(acq.poll(), Poll::Held, "free the moment the holder lets go");
    }
}

#[test]
fn a_record_of_a_free_slot_is_ignored_and_replaced() {
    let rig = Rig::new();
    let w = rig.agent(12);
    // what a crashed holder leaves: its record, with nobody holding the lock
    std::fs::write(rig.lock_dir(), format!("99999\nx\ncrashed\n{}\n/w\ncargo test\n", rig.now() - 100)).unwrap();
    assert!(w.h.status().starts_with("free"), "{}", w.h.status());
    let mut acq = Acquirer::new(&w.h, req("W")).unwrap();
    assert_eq!(acq.poll(), Poll::Held, "no waiting, no break, no liveness question");
    assert_eq!(rig.owner_line(2), "W");
    assert!(rig.breaks_log().is_empty());
}

#[test]
fn a_taker_never_judges_a_holder_by_its_pid() {
    // The holder's pid is not in the process table at all: it holds the lock, so it holds the slot.
    let rig = Rig::new();
    let w = rig.agent(12);
    let _holder = rig.plant_holder(&format!("424242\nx\nnot-in-any-table\n{}\n", rig.now() - 100));
    let mut acq = Acquirer::new(&w.h, req("W")).unwrap();
    for _ in 0..3 {
        assert_eq!(acq.poll(), Poll::Waiting);
    }
    assert!(w.h.status().contains("held by not-in-any-table pid 424242"));
}

#[test]
fn a_released_slot_is_free_for_the_next_taker_and_its_record_is_gone() {
    let rig = Rig::new();
    let (a, b) = (rig.agent(11), rig.agent(12));
    let mut ha = Acquirer::new(&a.h, req("A")).unwrap();
    assert_eq!(ha.poll(), Poll::Held);
    let mut hb = Acquirer::new(&b.h, req("B")).unwrap();
    assert_eq!(hb.poll(), Poll::Waiting);
    drop(ha);
    assert!(!rig.lock_dir().exists());
    assert_eq!(hb.poll(), Poll::Held);
    assert_eq!(rig.owner_line(2), "B", "the new holder's own record, not the old one");
}

// Requirement (1): the lock folder is never on a network file system.
#[test]
fn a_lock_folder_on_a_network_file_system_is_refused_with_what_to_do() {
    let rig = Rig::new();
    let mut a = rig.agent(11);
    a.h.fs_kind = std::sync::Arc::new(|_| throttle::fsinfo::FsKind::Network("smbfs".into()));
    let Err(msg) = Acquirer::new(&a.h, req("A")) else {
        panic!("expected a refusal")
    };
    assert!(
        msg.contains("network file system") && msg.contains("smbfs") && msg.contains("THROTTLE_DIR"),
        "{msg}"
    );
    assert!(a.h.status().starts_with("REFUSED"), "{}", a.h.status());
    assert_eq!(rig.held("throttle-"), 0, "nothing was taken");
    assert_eq!(rig.count("pitcrew-heavy.ticket."), 0, "no ticket either");
}

#[test]
fn a_file_system_the_os_did_not_name_is_let_through_and_logged() {
    let rig = Rig::new();
    let mut a = rig.agent(11);
    a.h.fs_kind = std::sync::Arc::new(|_| throttle::fsinfo::FsKind::Unknown("statfs failed".into()));
    let mut acq = Acquirer::new(&a.h, req("A")).unwrap();
    assert_eq!(acq.poll(), Poll::Held);
    assert!(a.h.log.has("the file system of the lock folder is not known"));
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
    assert!(n.said().contains("behind train work: real"), "{}", n.said());
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
    let text = fs::read_to_string(rig.lock_dir()).unwrap();
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

// ---- review fixes -------------------------------------------------------------------------------------------------------

#[test]
fn release_removes_our_record_and_frees_the_kernel_lock() {
    let rig = Rig::new();
    let a = rig.agent(11);
    let mut acq = Acquirer::new(&a.h, req("S")).unwrap();
    assert_eq!(acq.poll(), Poll::Held);
    acq.release();
    assert!(!rig.lock_dir().exists());
    assert_eq!(rig.count(".plock."), 0);
    assert!(!throttle::slotfile::is_held(&rig.root().join("throttle-slot1.lock")).unwrap());
    assert!(a.h.log.has("released the lock"));
}

#[test]
fn a_ticket_and_a_marker_of_a_pid_we_may_not_inspect_are_alive_too() {
    let rig = Rig::new();
    let n = rig.agent(12);
    rig.procs.run_opaque(1);
    let t = rig.root().join("pitcrew-heavy.ticket.0000000000000001-1");
    fs::write(&t, format!("1\nx\ninit\n{}\n", rig.now() - 100)).unwrap();
    let m = rig.root().join("pitcrew-heavy.high.1-init");
    fs::write(&m, format!("1\nx\n{}\n", rig.now() - 100)).unwrap();
    let mut acq = Acquirer::new(&n.h, req("N")).unwrap();
    assert_eq!(acq.poll(), Poll::Waiting, "a live high marker goes first");
    assert!(t.exists() && m.exists(), "EPERM is alive: neither is removed");
    assert!(rig.breaks_log().is_empty());
}

#[test]
fn masking_more_shapes() {
    // a key ending in a colon, or holding a colon, hides the next word
    assert_eq!(
        args(&["npm", "config", "set", "//reg/:_authToken", "abc123"]),
        "npm config set *** ***"
    );
    assert_eq!(args(&["tool", "X-Api-Key:", "k", "next"]), "tool *** *** next");
    assert_eq!(args(&["sshpass", "-p", "hunter2", "ssh", "host"]), "sshpass -p *** ssh host");
    assert_eq!(args(&["sshpass", "-pSECRET", "ssh"]), "sshpass -p*** ssh");
    assert_eq!(
        args(&["tool", "--pw", "hunter2", "--passphrase", "p2"]),
        "tool --pw *** --passphrase ***"
    );
    for tok in [
        "ghp_abcdefghijklmnopqrstuvwxyz0123456789",
        "gho_abcdefghijklmnopqrstuvwxyz0123456789",
        "github_pat_11ABCDEFG0abcdefghijklmnopqrstuvwxyz",
        "glpat-abcdefghijklmnopqrst",
        "xoxb-123456789012-abcdefghij",
        "xoxp-123456789012-abcdefghij",
        "AKIAIOSFODNN7EXAMPLE",
        "ASIAIOSFODNN7EXAMPLE",
        "sk-abcdefghijklmnopqrstuvwxyz012345",
        "sk_live_abcdefghijklmnopqrstuvwx",
    ] {
        assert_eq!(args(&["tool", tok]), "tool ***", "{tok}");
        assert_eq!(args(&["tool", &format!("--opt={tok}")]), "tool --opt=***", "{tok}");
        assert_eq!(args(&["tool", &format!("https://h/p/{tok}/x")]), "tool https://h/p/***/x", "{tok}");
    }
    // not tokens: short, or a different word
    assert_eq!(
        args(&["cargo", "build", "-p", "skeleton", "AKIAshort", "ghp_short", "sk-x"]),
        "cargo build -p skeleton AKIAshort ghp_short sk-x"
    );
}

#[test]
fn masking_free_text() {
    let t = "error: token=SUPERSECRET42 failed\nAuthorization: Bearer abcdef\nplain line\n//reg/:_authToken abc123\nghp_abcdefghijklmnopqrstuvwxyz0123456789";
    let m = throttle::mask::mask_text(t);
    for leak in ["SUPERSECRET42", "abcdef", "abc123", "ghp_abcdef"] {
        assert!(!m.contains(leak), "{leak} leaked: {m}");
    }
    assert!(m.contains("plain line") && m.contains("token=***") && m.lines().count() == 5, "{m}");
}

#[test]
fn guard_config_reader() {
    use throttle::guard::{config_target_dirs, target_dir_in};
    assert_eq!(
        target_dir_in("[build]\njobs = 2\ntarget-dir = \"/x/y\" # c\n").as_deref(),
        Some("/x/y")
    );
    assert_eq!(target_dir_in("build.target-dir = 'rel'\n").as_deref(), Some("rel"));
    assert_eq!(target_dir_in("[net]\ntarget-dir = \"no\"\n"), None);
    assert_eq!(target_dir_in("[build]\n[other]\ntarget-dir = \"no\"\n"), None);
    let rig = Rig::new();
    let proj = rig.root().join("a/b");
    fs::create_dir_all(rig.root().join("a/.cargo")).unwrap();
    fs::create_dir_all(&proj).unwrap();
    fs::write(rig.root().join("a/.cargo/config.toml"), "[build]\ntarget-dir = \"out\"\n").unwrap();
    let found = config_target_dirs(&proj, None);
    assert_eq!(found, vec![rig.root().join("a/out").to_string_lossy().into_owned()]);
}
