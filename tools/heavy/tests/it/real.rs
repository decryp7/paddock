//! The real binary with real processes (Unix: the commands are `sh`). All waits are bounded.
#![cfg(unix)]

use crate::procutil::*;
use paddock_proc::{ProcessId, ask};
use std::fs;

#[test]
fn a_run_takes_the_lock_runs_the_command_and_releases() {
    let w = World::new();
    let (code, _o, e) = heavy_out(&w, &["run", "--no-gate", "E", "--", "sh", "-c", "sleep 2"], &[]);
    assert_eq!(code, 0, "{e}");
    assert!(e.contains("E holds the lock") && e.contains("E done rc=0 at"), "{e}");
    let act = w.read(w.root().join("pitcrew-lock-activity.log"));
    assert!(
        act.contains("E took the lock after") && regex_like(&act, "E done rc=0, ran [2-9]s"),
        "{act}"
    );
    assert!(!w.lock_dir().exists());
    assert_eq!(w.count("pitcrew-heavy.ticket."), 0);
}

/// `needle` with `[2-9]` standing for one digit 2..9 (no regex dependency).
fn regex_like(hay: &str, needle: &str) -> bool {
    (2..=9).any(|d| hay.contains(&needle.replace("[2-9]", &d.to_string())))
}

#[test]
fn the_exit_code_is_passed_on_and_a_failure_is_explained_in_the_log() {
    let w = World::new();
    let log = w.path("run.clef");
    let (code, _o, e) = heavy_env_out(
        &w,
        &[
            "run",
            "--no-gate",
            "--log-file",
            log.to_str().unwrap(),
            "F",
            "--",
            "sh",
            "-c",
            "echo boom-line >&2; exit 3",
        ],
        &[("CI", "1")],
    );
    assert_eq!(code, 3);
    assert!(e.contains("boom-line"), "stderr still reaches the caller: {e}");
    assert!(
        e.lines().filter(|l| l.starts_with("::error::")).count() == 1,
        "exactly one ::error:: line: {e}"
    );
    assert!(
        e.contains("::error::heavy ") && e.contains("exit 0x00000003") && e.contains("exit code 3") && e.contains("Next:"),
        "{e}"
    );
    let text = w.read(&log);
    let failed = text.lines().find(|l| l.contains("the command failed")).expect("failure line");
    assert!(
        failed.contains("\"ExitCode\":3") && failed.contains("boom-line") && failed.contains("\"@l\":\"Error\""),
        "{failed}"
    );
    assert!(failed.contains("sh -c"), "the exact (masked) command is logged: {failed}");
}

fn heavy_env_out(w: &World, args: &[&str], env: &[(&str, &str)]) -> (i32, String, String) {
    heavy_out(w, args, env)
}

#[test]
fn a_command_killed_by_a_signal_is_named() {
    let w = World::new();
    let log = w.path("run.clef");
    let (code, _o, _e) = heavy_out(
        &w,
        &[
            "run",
            "--no-gate",
            "--log-file",
            log.to_str().unwrap(),
            "K",
            "--",
            "sh",
            "-c",
            "kill -9 $$",
        ],
        &[],
    );
    assert_eq!(code, 137);
    assert!(w.read(&log).contains("killed by signal 9 (SIGKILL)"));
}

#[test]
fn a_missing_command_exits_127_and_says_so() {
    let w = World::new();
    let (code, _o, e) = heavy_out(&w, &["run", "--no-gate", "M", "--", "/no/such/program-xyz"], &[]);
    assert_eq!(code, 127);
    assert!(e.contains("cannot run"), "{e}");
    assert!(!w.lock_dir().exists());
}

#[test]
fn usage_errors_exit_2() {
    let w = World::new();
    assert_eq!(heavy_out(&w, &["run", "X"], &[]).0, 64);
    assert_eq!(heavy_out(&w, &["bogus"], &[]).0, 64);
    assert_eq!(heavy_out(&w, &["run", "--wat", "X", "--", "true"], &[]).0, 64);
}

#[test]
fn contenders_are_served_in_queue_order_whatever_their_poll_rate() {
    let w = World::new();
    let (flag, out) = (w.flag(), w.path("out"));
    let (flag, out) = (flag.to_str().unwrap(), out.to_str().unwrap());
    let mut a = heavy(&w, &hold_args("A", flag, out));
    wait_until("A holds", || w.locked());
    let mut b = heavy_env(&w, &echo_args("B", out), &[("PLOCK_POLL", "1.5")]);
    wait_until("B queued", || w.count("pitcrew-heavy.ticket.") >= 1);
    let mut c = heavy_env(&w, &echo_args("C", out), &[("PLOCK_POLL", "0.1")]);
    wait_until("C queued", || w.count("pitcrew-heavy.ticket.") >= 2);
    w.release();
    for g in [&mut a, &mut b, &mut c] {
        assert!(g.finish().success());
    }
    assert_eq!(w.out_words(), ["A", "B", "C"]);
}

#[test]
fn high_goes_before_an_earlier_normal_waiter() {
    for i in 1..=3 {
        let w = World::new();
        let (flag, out) = (w.flag(), w.path("out"));
        let (flag, out) = (flag.to_str().unwrap(), out.to_str().unwrap());
        let mut a = heavy(&w, &hold_args("A", flag, out));
        wait_until("A holds", || w.locked());
        let mut b = heavy_env(&w, &echo_args("B", out), &[("PLOCK_POLL", "0.1")]);
        wait_until("B queued", || w.count("pitcrew-heavy.ticket.") >= 1);
        let mut args = echo_args("H", out);
        args.insert(1, "--high");
        let mut h = heavy_env(&w, &args, &[("PLOCK_POLL", "1.5")]);
        wait_until("H marker", || w.count("pitcrew-heavy.high.") >= 1);
        w.release();
        for g in [&mut a, &mut b, &mut h] {
            assert!(g.finish().success());
        }
        assert_eq!(w.out_words(), ["A", "H", "B"], "run {i}");
    }
}

#[test]
fn a_dead_owner_is_broken_and_a_recycled_pid_too() {
    let w = World::new();
    let dead = dead_pid();
    fs::create_dir(w.lock_dir()).unwrap();
    fs::write(
        w.lock_dir().join("owner"),
        format!("{dead}\nx\nghost\n{}\n/tmp\nsleep 1\n", now_secs()),
    )
    .unwrap();
    let (code, _, e) = heavy_out(&w, &echo_args("Z", w.path("out").to_str().unwrap()), &[]);
    assert_eq!(code, 0, "{e}");
    assert!(
        w.read(w.root().join("pitcrew-lock-breaks.log"))
            .contains("Z broke a stale lock of ghost")
    );

    // a live process that started AFTER the record: the pid was handed on
    let w = World::new();
    let live = sleeper();
    fs::create_dir(w.lock_dir()).unwrap();
    fs::write(
        w.lock_dir().join("owner"),
        format!("{}\nx\nreused\n{}\n", live.pid(), now_secs() - 100),
    )
    .unwrap();
    let t0 = std::time::Instant::now();
    let (code, _, e) = heavy_out(&w, &echo_args("Z", w.path("out").to_str().unwrap()), &[]);
    assert_eq!(code, 0, "{e}");
    assert!(t0.elapsed().as_secs() <= 4);
    let breaks = w.read(w.root().join("pitcrew-lock-breaks.log"));
    assert!(breaks.contains("Z broke a stale lock of reused"), "{breaks}");
}

#[test]
fn a_live_owner_whose_record_is_right_is_not_broken_whatever_tz_and_locale() {
    for (t1, l1, t2, l2) in [
        ("UTC", "de_DE.UTF-8", "Asia/Tokyo", "C"),
        ("America/Los_Angeles", "fr_FR.UTF-8", "UTC", "en_US.UTF-8"),
    ] {
        let w = World::new();
        let (flag, out) = (w.flag(), w.path("out"));
        let (flag, out) = (flag.to_str().unwrap(), out.to_str().unwrap());
        let mut h = heavy_env(&w, &hold_args("Holder", flag, out), &[("TZ", t1), ("LC_ALL", l1)]);
        wait_until("holder", || w.locked());
        let mut wt = heavy_env(&w, &echo_args("Waiter", out), &[("TZ", t2), ("LC_ALL", l2)]);
        wait_until("waiter notes", || {
            w.read(w.root().join("pitcrew-lock-activity.log")).contains("Waiter waiting")
        });
        assert!(
            w.lock_dir().exists() && w.out().is_empty(),
            "TZ={t1} LC_ALL={l1} holder broken by TZ={t2} LC_ALL={l2} waiter"
        );
        assert!(!w.read(w.root().join("pitcrew-lock-breaks.log")).contains("broke"));
        w.release();
        assert!(h.finish().success() && wt.finish().success());
    }
}

/// Epoch second pid 1 started, from the same source heavy reads: the record must be written after its writer started,
/// whatever the age of the machine or container (a fixed "100 s ago" is older than a container started a second ago).
fn init_start_epoch() -> u64 {
    ProcessId::of(1).expect("pid 1 is visible").started().as_micros() / 1_000_000
}

#[test]
fn pid_one_is_alive_even_though_we_may_not_signal_it() {
    let w = World::new();
    fs::create_dir(w.lock_dir()).unwrap();
    fs::write(w.lock_dir().join("owner"), format!("1\nx\ninit\n{}\n", init_start_epoch() + 1)).unwrap();
    let mut g = heavy(&w, &echo_args("W", w.path("out").to_str().unwrap()));
    wait_until("waiting for init", || {
        w.read(w.root().join("pitcrew-lock-activity.log")).contains("held by init")
    });
    assert!(w.lock_dir().exists() && w.out().is_empty());
    fs::remove_dir_all(w.lock_dir()).unwrap();
    assert!(g.finish().success());
}

#[test]
fn the_owner_record_has_six_lines_and_the_command_is_masked() {
    let w = World::new();
    let copy = w.path("rec.txt");
    let lock_owner = w.lock_dir().join("owner");
    let script = format!("cp {} {}", lock_owner.display(), copy.display());
    let (code, _, e) = heavy_out(
        &w,
        &[
            "run",
            "--no-gate",
            "Rec",
            "--",
            "sh",
            "-c",
            &script,
            "api_key=abc",
            "--token",
            "hunter2",
        ],
        &[],
    );
    assert_eq!(code, 0, "{e}");
    let rec = w.read(&copy);
    // six lines, or seven once heavy has added the `child` line (the command may copy the record before that)
    assert!((6..=7).contains(&rec.lines().count()), "{rec}");
    let l: Vec<&str> = rec.lines().collect();
    assert!(l[0].chars().all(|c| c.is_ascii_digit()) && l[2] == "Rec" && l[3].len() == 10);
    assert!(
        l[5].contains("api_key=***") && l[5].contains("--token ***") && !rec.contains("hunter2") && !rec.contains("=abc"),
        "{rec}"
    );
    let log = w.read(w.root().join("pitcrew-lock-activity.log"));
    assert!(!log.contains("hunter2"));
    let clef = fs::read_dir(w.root().join("heavy-logs"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| w.read(e.path()))
        .collect::<String>();
    assert!(!clef.contains("hunter2") && clef.contains("--token ***"), "{clef}");
}

#[test]
fn status_shows_the_holder_and_the_queue() {
    let w = World::new();
    let (flag, out) = (w.flag(), w.path("out"));
    let (flag, out) = (flag.to_str().unwrap(), out.to_str().unwrap());
    let mut a = heavy(&w, &hold_args("S", flag, out));
    wait_until("S holds", || w.locked());
    let (code, o, _) = heavy_out(&w, &["status"], &[]);
    assert_eq!(code, 0);
    assert!(o.contains("held by S pid") && o.contains("command: sh -c"), "{o}");
    w.release();
    assert!(a.finish().success());
    let (_, o, _) = heavy_out(&w, &["status"], &[]);
    assert_eq!(o.lines().next(), Some("free"));
}

#[test]
fn the_home_guard_refuses_a_build_folder_inside_home() {
    let w = World::new();
    let home = w.path("home");
    fs::create_dir_all(&home).unwrap();
    let inside = home.join("target");
    let (code, _, e) = heavy_out(
        &w,
        &[
            "run",
            "--no-gate",
            "G",
            "--",
            "sh",
            "-c",
            "echo ran >> \"$0\"",
            w.path("out").to_str().unwrap(),
        ],
        &[("CARGO_TARGET_DIR", inside.to_str().unwrap())],
    );
    assert_eq!(code, 78, "{e}");
    assert!(e.contains("ER-14") && w.out().is_empty() && !w.lock_dir().exists());
    let (code, _, _) = heavy_out(
        &w,
        &[
            "run",
            "--no-gate",
            "G",
            "--",
            "sh",
            "-c",
            "echo ran >> \"$0\"",
            w.path("out").to_str().unwrap(),
        ],
        &[("CARGO_TARGET_DIR", inside.to_str().unwrap()), ("HEAVY_ALLOW_HOME_TARGET", "1")],
    );
    assert_eq!(code, 0);
    let elsewhere = w.path("elsewhere");
    let (code, _, _) = heavy_out(
        &w,
        &["run", "--no-gate", "G", "--", "true", "--target-dir", elsewhere.to_str().unwrap()],
        &[],
    );
    assert_eq!(code, 0);
    let (code, _, _) = heavy_out(
        &w,
        &["run", "--no-gate", "G", "--", "true", &format!("--target-dir={}", inside.display())],
        &[],
    );
    assert_eq!(code, 78);
}

#[test]
fn the_load_gate_waits_and_stopping_during_it_releases_the_lock() {
    let w = World::new();
    // threshold 0: any load is at or above it, so the gate never opens
    let mut g = heavy_env(
        &w,
        &["run", "--gate-load", "0", "Gate", "--", "true"],
        &[("HEAVY_GATE_POLL", "0.1")],
    );
    wait_until("the gate to close", || {
        w.read(w.root().join("pitcrew-lock-activity.log")).contains("load gate")
    });
    assert!(w.lock_dir().exists(), "the gate waits while holding the lock");
    let id = ProcessId::of(g.pid()).expect("heavy runs");
    assert!(ask(id));
    let st = g.finish();
    assert_eq!(st.code(), Some(143));
    assert!(!w.lock_dir().exists(), "the lock is released on SIGTERM");
    // a high threshold opens at once
    let (code, _, e) = heavy_out(&w, &["run", "--gate-load", "100000", "Gate", "--", "true"], &[]);
    assert_eq!(code, 0);
    assert!(e.contains("gate ok"), "{e}");
}

#[test]
fn sigterm_ends_the_command_and_everything_it_started_and_releases() {
    let w = World::new();
    let pidfile = w.path("child.pid");
    let script = format!("sleep 60 & echo $! > {}; wait", pidfile.display());
    let mut g = heavy(&w, &["run", "--no-gate", "T", "--", "sh", "-c", &script]);
    wait_until("grandchild started", || w.read(&pidfile).trim().parse::<u32>().is_ok());
    let grandchild: u32 = w.read(&pidfile).trim().parse().unwrap();
    let gc = ProcessId::of(grandchild).expect("grandchild runs");
    assert!(ask(ProcessId::of(g.pid()).expect("heavy runs")));
    assert_eq!(g.finish().code(), Some(143));
    wait_until("the grandchild to end", || !gc.is_alive());
    assert!(!w.lock_dir().exists());
    assert_eq!(w.count("pitcrew-heavy.ticket."), 0);
}

#[test]
fn a_queued_run_stopped_by_sigterm_leaves_no_ticket() {
    let w = World::new();
    let (flag, out) = (w.flag(), w.path("out"));
    let (flag, out) = (flag.to_str().unwrap(), out.to_str().unwrap());
    let mut a = heavy(&w, &hold_args("A", flag, out));
    wait_until("A holds", || w.locked());
    let mut b = heavy(&w, &echo_args("B", out));
    wait_until("B queued", || w.count("pitcrew-heavy.ticket.") >= 1);
    assert!(ask(ProcessId::of(b.pid()).expect("B runs")));
    assert_eq!(b.finish().code(), Some(143));
    assert_eq!(w.count("pitcrew-heavy.ticket."), 0);
    w.release();
    assert!(a.finish().success());
}

#[test]
fn waits_are_logged_with_the_holder_and_the_evidence() {
    let w = World::new();
    let (flag, out) = (w.flag(), w.path("out"));
    let (flag, out) = (flag.to_str().unwrap(), out.to_str().unwrap());
    let log = w.path("wait.clef");
    let mut a = heavy(&w, &hold_args("Holder", flag, out));
    wait_until("holder", || w.locked());
    let mut args = echo_args("Waiter", out);
    args.splice(1..1, ["--log-file", log.to_str().unwrap(), "--verbose"]);
    let mut b = heavy(&w, &args);
    wait_until("a wait line", || w.read(&log).contains("\"Reason\":\"held\""));
    let line = w
        .read(&log)
        .lines()
        .find(|l| l.contains("\"Reason\":\"held\""))
        .unwrap()
        .to_string();
    assert!(
        line.contains("\"HolderName\":\"Holder\"")
            && line.contains("\"HolderPid\":")
            && line.contains("\"RecordEpoch\":")
            && line.contains("os start"),
        "{line}"
    );
    w.release();
    assert!(a.finish().success() && b.finish().success());
}

// ---- review fixes -------------------------------------------------------------------------------------------------------

/// Ends a process this test started by identity (rule 53), if it still runs.
fn end_by_identity(pid: u32) {
    if let Some(id) = ProcessId::of(pid) {
        let _ = paddock_proc::end(id);
    }
}

fn pid_from(w: &World, f: &std::path::Path) -> u32 {
    wait_until("a pid file", || w.read(f).trim().parse::<u32>().is_ok());
    w.read(f).trim().parse().expect("pid")
}

#[test]
fn a_background_process_holding_stderr_does_not_hold_heavy_or_the_lock() {
    let w = World::new();
    let pidfile = w.path("bg.pid");
    let script = format!("sleep 12 >/dev/null & echo $! > {}; echo hi >&2", pidfile.display());
    let t0 = std::time::Instant::now();
    let (code, _, e) = heavy_out(&w, &["run", "--no-gate", "B", "--", "sh", "-c", &script], &[]);
    let took = t0.elapsed();
    let bg = pid_from(&w, &pidfile);
    assert_eq!(code, 0, "{e}");
    assert!(took.as_secs() < 8, "heavy waited {took:?} for a background process's stderr");
    assert!(!w.lock_dir().exists(), "the lock is released when the command ends");
    assert!(e.contains("hi"));
    end_by_identity(bg);
}

#[test]
fn sigterm_during_the_stderr_drain_still_stops_heavy_at_once() {
    let w = World::new();
    let pidfile = w.path("bg.pid");
    let log = w.path("drain.clef");
    let script = format!("sleep 30 & echo $! > {}", pidfile.display());
    let args = [
        "run",
        "--no-gate",
        "--log-file",
        log.to_str().unwrap(),
        "D",
        "--",
        "sh",
        "-c",
        &script,
    ];
    // a long drain bound: only the stop request can end the wait
    let mut g = heavy_env(&w, &args, &[("HEAVY_DRAIN_MS", "25000")]);
    let bg = pid_from(&w, &pidfile);
    wait_until("the drain to start", || w.read(&log).contains("draining its stderr"));
    let t0 = std::time::Instant::now();
    assert!(ask(ProcessId::of(g.pid()).expect("heavy runs")));
    let st = g.finish();
    assert_eq!(st.code(), Some(143), "SIGTERM during the drain must give 143, not 0");
    assert!(t0.elapsed().as_secs() < 8);
    assert!(!w.lock_dir().exists());
    end_by_identity(bg);
}

#[test]
fn the_stderr_tail_in_the_log_is_masked() {
    let w = World::new();
    let log = w.path("tail.clef");
    let (code, _, _) = heavy_out(
        &w,
        &[
            "run",
            "--no-gate",
            "--log-file",
            log.to_str().unwrap(),
            "M",
            "--",
            "sh",
            "-c",
            "echo token=SUPERSECRET42 >&2; echo 'npm //reg/:_authToken abc123tok' >&2; exit 4",
        ],
        &[],
    );
    assert_eq!(code, 4);
    let text = w.read(&log);
    assert!(text.contains("the command failed") && text.contains("token=***"), "{text}");
    assert!(!text.contains("SUPERSECRET42") && !text.contains("abc123tok"), "{text}");
}

#[test]
fn the_run_id_is_reused_and_passed_on() {
    let w = World::new();
    let log = w.path("id.clef");
    let seen = w.path("seen");
    let script = format!("echo \"$DEVKIT_RUN_ID\" > {}", seen.display());
    let (code, _, _) = heavy_out(
        &w,
        &[
            "run",
            "--no-gate",
            "--log-file",
            log.to_str().unwrap(),
            "I",
            "--",
            "sh",
            "-c",
            &script,
        ],
        &[("DEVKIT_RUN_ID", "run-from-the-caller")],
    );
    assert_eq!(code, 0);
    assert_eq!(w.read(&seen).trim(), "run-from-the-caller");
    assert!(w.read(&log).lines().all(|l| l.contains("\"SessionId\":\"run-from-the-caller\"")));
    // without one, heavy makes its own and passes that on
    let (code, _, _) = heavy_out(&w, &["run", "--no-gate", "I", "--", "sh", "-c", &script], &[]);
    assert_eq!(code, 0);
    assert!(w.read(&seen).trim().contains('-'));
}

#[test]
fn failures_of_heavy_itself_get_one_annotation_in_ci_with_version_os_and_hex_code() {
    let w = World::new();
    let (code, _, e) = heavy_out(&w, &["run", "X"], &[("CI", "1")]);
    assert_eq!(code, 64);
    let lines: Vec<&str> = e.lines().filter(|l| l.starts_with("::error::")).collect();
    assert_eq!(lines.len(), 1, "{e}");
    assert!(
        lines[0].contains(env!("CARGO_PKG_VERSION"))
            && lines[0].contains(std::env::consts::OS)
            && lines[0].contains("exit 0x00000040")
            && lines[0].contains("Next:"),
        "{}",
        lines[0]
    );
    let inside = w.path("home").join("t");
    let (code, _, e) = heavy_out(
        &w,
        &["run", "--no-gate", "G", "--", "true"],
        &[("CI", "1"), ("CARGO_TARGET_DIR", inside.to_str().unwrap())],
    );
    assert_eq!(code, 78);
    assert!(
        e.lines().filter(|l| l.starts_with("::error::")).count() == 1 && e.contains("exit 0x0000004E"),
        "{e}"
    );
    // outside CI: a plain line, no annotation
    let (_, _, e) = heavy_out(&w, &["run", "X"], &[]);
    assert!(!e.contains("::error::") && e.contains("exit 0x00000040"), "{e}");
}

#[test]
fn a_stop_in_ci_is_annotated() {
    let w = World::new();
    let mut g = heavy_env(
        &w,
        &["run", "--gate-load", "0", "Gate", "--", "true"],
        &[("HEAVY_GATE_POLL", "0.1"), ("CI", "1")],
    );
    wait_until("the gate to close", || w.lock_dir().exists());
    wait_until("the gate wait to be noted", || {
        w.read(w.root().join("pitcrew-lock-activity.log")).contains("load gate")
    });
    assert!(ask(ProcessId::of(g.pid()).expect("heavy runs")));
    assert_eq!(g.finish().code(), Some(143));
    let mut e = String::new();
    use std::io::Read;
    g.child.stderr.take().expect("stderr").read_to_string(&mut e).expect("read");
    assert!(
        e.contains("::error::") && e.contains("exit 0x0000008F") && e.contains("stopped by a signal"),
        "{e}"
    );
}

#[test]
fn the_home_guard_also_reads_cargo_build_target_dir_and_cargo_config() {
    let w = World::new();
    let home = w.path("home");
    fs::create_dir_all(&home).unwrap();
    let inside = home.join("viaenv");
    let (code, _, e) = heavy_out(
        &w,
        &["run", "--no-gate", "G", "--", "true"],
        &[("CARGO_BUILD_TARGET_DIR", inside.to_str().unwrap())],
    );
    assert_eq!(code, 78, "{e}");
    // a project with .cargo/config.toml naming a target dir inside home (relative to the project)
    let proj = home.join("proj");
    fs::create_dir_all(proj.join(".cargo")).unwrap();
    fs::write(proj.join(".cargo/config.toml"), "[build]\ntarget-dir = \"build-out\"\n").unwrap();
    let (code, _, e) = heavy_out_in(&w, Some(proj.as_path()), &["run", "--no-gate", "G", "--", "true"], &[]);
    assert_eq!(code, 78, "{e}");
    // the same config pointing outside home is fine
    let outside = w.path("outside");
    fs::write(
        proj.join(".cargo/config.toml"),
        format!("[build]\ntarget-dir = \"{}\"\n", outside.display()),
    )
    .unwrap();
    let (code, _, e) = heavy_out_in(&w, Some(proj.as_path()), &["run", "--no-gate", "G", "--", "true"], &[]);
    assert_eq!(code, 0, "{e}");
}

#[test]
fn the_owner_record_names_the_command_while_it_runs() {
    let w = World::new();
    let (flag, out) = (w.flag(), w.path("out"));
    let (flag, out) = (flag.to_str().unwrap(), out.to_str().unwrap());
    let mut a = heavy(&w, &hold_args("C", flag, out));
    wait_until("the child line", || w.owner_line(6).starts_with("child "));
    let line = w.owner_line(6);
    let parts: Vec<&str> = line.split(' ').collect();
    let child: u32 = parts[1].parse().expect("child pid");
    assert_ne!(child, a.pid());
    assert!(ProcessId::of(child).is_some_and(|p| p.started().as_micros().to_string() == parts[2]));
    assert_eq!(w.owner_line(0), a.pid().to_string(), "line 1 stays heavy's own pid for the shell");
    w.release();
    assert!(a.finish().success());
}
