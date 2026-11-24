//! ADR 0119 WP-1: a `Scope` holds every process it starts and ends them when it ends, on every path: a normal end, a panic
//! that unwinds, a cancel, and (Unix, through the orphan guardian) an owner ended by SIGKILL. The stand-ins are this test
//! binary run again on [`stand_in`] (a program Apple ships hides its environment from `ps` on macOS, ADR 0112, so a member
//! found by the scope's marker must be ours), each ending by itself within [`STAND_IN_LIFE`] if everything else fails
//! (ER-14). The positive control (ER-33): a child started outside the scope is still running after the scope ended, so
//! the probe that says "ended" can see a live process.

use paddock_proc::own::{Owned, Waited, has_ended};
use paddock_proc::scope::{HowEnded, Scope};
use paddock_proc::{Limits, ProcessId, command, kill_tree};
use paddock_testkit::wait::{HANG_GUARD, poll_until_within};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// What a stand-in does: unset in a normal test run, where [`stand_in`] returns at once.
const MODE: &str = "PADDOCK_SCOPE_TEST_MODE";
/// The longest a stand-in lives when nothing ends it.
const STAND_IN_LIFE: Duration = Duration::from_secs(60);
/// The test entry that serves as this binary's orphan guardian.
const ENTRY: &str = "scope::guardian_entry";

/// Not a test: the orphan guardian of the owner process below (this binary run with the guardian variable set).
#[test]
fn guardian_entry() {
    paddock_proc::guardian::serve_if_guardian();
}

/// Not a test when run on its own: what this binary does when a test starts it with [`MODE`] set.
#[test]
fn stand_in() {
    let Some(mode) = std::env::var_os(MODE) else {
        return;
    };
    match mode.to_string_lossy().as_ref() {
        // Lives until it is ended (or its life is over).
        "sleep" => std::thread::sleep(STAND_IN_LIFE),
        // Says it runs, then lives until it is ended (the grace measurement asks it to stop).
        "ready" => {
            println!("ready");
            std::thread::sleep(STAND_IN_LIFE);
        }
        // Says where it runs and exits.
        "where" => {
            let tmp = if cfg!(windows) { "TEMP" } else { "TMPDIR" };
            println!("cwd {}", std::env::current_dir().unwrap().display());
            println!("tmp {}", std::env::var(tmp).unwrap_or_default());
        }
        // Starts a grandchild in a process group of its own (it leaves the scope's group kill), names it and lives on.
        "escape" => {
            let mut g = self_as("sleep");
            paddock_proc::own::own_group(&mut g);
            let mut g = g.spawn().unwrap();
            println!("grandchild {}", g.id());
            std::thread::sleep(STAND_IN_LIFE);
            let _ = (g.kill(), g.wait());
        }
        // A grandchild in the child's own group (as a browser has helpers), named; then lives on.
        "parent" => {
            let mut g = self_as("sleep").spawn().unwrap();
            println!("grandchild {}", g.id());
            std::thread::sleep(STAND_IN_LIFE);
            let _ = (g.kill(), g.wait());
        }
        // The owner of a scope: starts one child that has a grandchild, names both and the scope's folder, then blocks until it
        // is killed (or its standard input ends).
        "owner" => {
            paddock_proc::guardian::enable_in_test_binary(ENTRY);
            let under = PathBuf::from(std::env::var("PADDOCK_SCOPE_TEST_UNDER").unwrap());
            let scope = Scope::new("owner", &under).unwrap();
            let mut child = scope.start(piped("parent")).unwrap();
            let grandchild = first_pid(&mut child, "grandchild");
            println!("child {}", child.process().unwrap().pid());
            println!("grandchild {grandchild}");
            println!("folder {}", scope.folder().display());
            println!("ready");
            let _ = std::io::stdin().read_line(&mut String::new());
            std::process::exit(0);
        }
        // An owner that is killed as soon as its first start has returned: names the child and the folder at once.
        "owner-quick" => {
            paddock_proc::guardian::enable_in_test_binary(ENTRY);
            let under = PathBuf::from(std::env::var("PADDOCK_SCOPE_TEST_UNDER").unwrap());
            let scope = Scope::new("owner-quick", &under).unwrap();
            let child = scope.start(self_as("sleep")).unwrap();
            println!("child {}", child.process().unwrap().pid());
            println!("folder {}", scope.folder().display());
            println!("ready");
            let _ = std::io::stdin().read_line(&mut String::new());
            std::process::exit(0);
        }
        // Times its first start, which starts the orphan guardian and waits until it serves, and prints it in µs.
        "first-start" => {
            paddock_proc::guardian::enable_in_test_binary(ENTRY);
            let under = PathBuf::from(std::env::var("PADDOCK_SCOPE_TEST_UNDER").unwrap());
            let scope = Scope::new("first-start", &under).unwrap();
            let began = Instant::now();
            let child = scope.start(self_as("sleep")).unwrap();
            println!("first-start {}", began.elapsed().as_micros());
            drop(child);
        }
        other => panic!("unknown stand-in mode {other}"),
    }
}

/// This binary as a stand-in in `mode`, quiet.
fn self_as(mode: &str) -> Command {
    let mut c = command(std::env::current_exe().unwrap());
    c.args(["--exact", "scope::stand_in", "--nocapture"])
        .env(MODE, mode)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    c
}

/// [`self_as`] with its standard output piped.
fn piped(mode: &str) -> Command {
    let mut c = self_as(mode);
    c.stdout(Stdio::piped());
    c
}

/// Reads the held child's output up to the line `<word> <pid>` and gives the pid.
fn first_pid(held: &mut paddock_proc::scope::Held, word: &str) -> u32 {
    let out = held.take_stdout().expect("piped output");
    let mut r = BufReader::new(out);
    let mut line = String::new();
    loop {
        line.clear();
        assert!(
            r.read_line(&mut line).unwrap() > 0,
            "the child ended before it named its {word}"
        );
        if let Some(pid) = line.trim().strip_prefix(&format!("{word} ")) {
            return pid.parse().unwrap();
        }
    }
}

/// Whether `id` ends within the test kit's hang guard.
fn ends(id: ProcessId) -> bool {
    poll_until_within(HANG_GUARD, || has_ended(id))
}

/// Ends what a failed test would leave behind (ER-14), by identity, then fails with `why`.
fn fail(ids: &[ProcessId], why: &str) -> ! {
    for id in ids {
        kill_tree(*id);
    }
    panic!("{why}");
}

#[test]
fn a_panic_inside_a_scope_ends_its_child() {
    let dir = tempfile::tempdir().unwrap();
    let mut seen = None;
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let scope = Scope::new("panic", dir.path()).unwrap();
        let held = scope.start(self_as("sleep")).unwrap();
        seen = held.process();
        panic!("a test step failed partway");
    }));
    assert!(unwound.is_err());
    let id = seen.expect("the child has an identity");
    if !ends(id) {
        fail(&[id], "the child of a scope that panicked still runs");
    }
}

#[test]
fn a_child_started_outside_the_scope_survives_and_is_not_in_the_summary() {
    let dir = tempfile::tempdir().unwrap();
    let mut outside = self_as("sleep").spawn().unwrap();
    let outside_id = ProcessId::of(outside.id()).unwrap();
    let scope = Scope::new("control", dir.path()).unwrap();
    let inside = scope.start(self_as("sleep")).unwrap().process().unwrap();
    let summary = scope.finish();
    let survived = !has_ended(outside_id);
    let _ = outside.kill();
    let _ = outside.wait();
    if !ends(inside) {
        fail(&[inside], "the scope's own child still runs");
    }
    assert!(
        survived,
        "the probe must see a live process: the child started outside the scope runs on"
    );
    assert!(
        summary.reports.iter().all(|r| r.id != Some(outside_id)),
        "{summary:?}"
    );
    assert!(
        summary.reports.iter().any(|r| r.id == Some(inside)),
        "{summary:?}"
    );
}

#[test]
fn a_cancel_ends_a_wait_and_the_end_then_ends_the_child() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::new("cancel", dir.path()).unwrap();
    let mut held = scope.start(self_as("sleep")).unwrap();
    let id = held.process().unwrap();
    let began = Instant::now();
    let waited = std::thread::scope(|s| {
        let waiter = s.spawn(move || held.wait(STAND_IN_LIFE));
        scope.cancel();
        waiter.join().unwrap()
    });
    assert_eq!(waited, Waited::Cancelled);
    assert!(
        began.elapsed() < HANG_GUARD,
        "the wait ran {:?} after the cancel",
        began.elapsed()
    );
    assert!(
        !has_ended(id),
        "a cancel ends the waits; the end ends the processes"
    );
    let summary = scope.finish();
    if !ends(id) {
        fail(
            &[id],
            "the cancelled scope's child still runs after its end",
        );
    }
    assert_eq!(summary.reports.len(), 1, "{summary:?}");
}

#[test]
fn a_cancel_ends_a_run_in_progress_and_nothing_starts_after_it() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::new("cancel-run", dir.path()).unwrap();
    let began = Instant::now();
    let out = std::thread::scope(|s| {
        let runner = s.spawn(|| scope.run(self_as("sleep"), Limits::new(STAND_IN_LIFE)));
        let started = poll_until_within(HANG_GUARD, || !scope.running().is_empty());
        scope.cancel();
        assert!(started, "the run never started");
        runner.join().unwrap()
    });
    let out = out.expect("the run started");
    assert!(!out.success(), "a cancelled run did not succeed: {out:?}");
    assert!(
        began.elapsed() < HANG_GUARD,
        "the run ran {:?} after the cancel",
        began.elapsed()
    );
    assert!(
        scope.start(self_as("sleep")).is_err(),
        "a start after the cancel"
    );
    assert!(
        scope
            .run(self_as("sleep"), Limits::new(STAND_IN_LIFE))
            .is_err(),
        "a run after the cancel"
    );
    assert!(scope.running().is_empty());
}

#[test]
fn a_childs_working_and_temp_folder_is_the_scopes_folder_which_goes_at_the_end() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::new("folder", dir.path()).unwrap();
    let folder = scope.folder().to_path_buf();
    assert!(
        folder.starts_with(dir.path()) && folder.is_dir(),
        "{}",
        folder.display()
    );
    let mut cmd = self_as("where");
    cmd.stdout(Stdio::piped());
    let out = scope.run(cmd, Limits::new(HANG_GUARD)).unwrap();
    assert!(out.success(), "{out:?}");
    let text = out.stdout_text();
    let canon = |p: &Path| paddock_env::winpath::real(p).unwrap();
    let line = |word: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(&format!("{word} ")))
            .map(PathBuf::from)
            .unwrap_or_else(|| panic!("no {word} line in {text:?}"))
    };
    assert_eq!(canon(&line("cwd")), canon(&folder));
    assert_eq!(canon(&line("tmp")), canon(&folder));
    let summary = scope.finish();
    assert!(!folder.exists(), "the folder stayed");
    assert!(
        matches!(summary.reports[..], [ref r] if matches!(r.how, HowEnded::Exited(Some(_)))),
        "{summary:?}"
    );
}

#[test]
fn a_kept_folder_stays_and_a_childs_own_working_folder_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::new("kept", dir.path()).unwrap().keep_folder();
    let folder = scope.folder().to_path_buf();
    let mut cmd = self_as("where");
    cmd.stdout(Stdio::piped()).current_dir(dir.path());
    let out = scope.run(cmd, Limits::new(HANG_GUARD)).unwrap();
    let cwd = out
        .stdout_text()
        .lines()
        .find_map(|l| l.strip_prefix("cwd ").map(PathBuf::from))
        .unwrap();
    assert_eq!(
        paddock_env::winpath::real(&cwd).unwrap(),
        paddock_env::winpath::real(dir.path()).unwrap()
    );
    drop(scope);
    assert!(folder.is_dir(), "a kept folder is not removed");
}

#[test]
fn ending_a_held_child_reports_how_and_ending_twice_reports_the_first_ending() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::new("end", dir.path()).unwrap();
    let mut held = scope.start(self_as("sleep")).unwrap();
    let id = held.process().unwrap();
    assert_eq!(held.result(), None);
    assert_eq!(held.wait(Duration::ZERO), Waited::Bound);
    assert_eq!(*held.end(), HowEnded::Ended);
    assert_eq!(*held.end(), HowEnded::Ended);
    assert_eq!(held.result(), Some(&HowEnded::Ended));
    assert_eq!(held.wait(Duration::ZERO), Waited::Gone);
    assert!(ends(id));
    let summary = scope.finish();
    assert_eq!(summary.reports.len(), 1, "{summary:?}");
    assert_eq!(summary.reports[0].how, HowEnded::Ended);
}

#[cfg(unix)]
#[test]
fn a_polite_child_is_asked_and_one_that_ignores_the_request_is_ended_after_the_grace() {
    let dir = tempfile::tempdir().unwrap();
    let grace = Duration::from_millis(500);
    let scope = Scope::new("grace", dir.path()).unwrap().with_grace(grace);
    let polite = scope.start(self_as("sleep")).unwrap().process().unwrap();
    let mut deaf = command("/bin/sh");
    // Bounded like every stand-in (ER-14): it ends by itself after 60 one-second sleeps.
    // It says "ready" once the trap is set: a request before that would end it.
    deaf.args([
        "-c",
        "trap '' TERM; echo ready; i=0; while [ $i -lt 60 ]; do sleep 1; i=$((i+1)); done",
    ])
    .stdout(Stdio::piped());
    let mut deaf = scope.start(deaf).unwrap();
    let mut ready = String::new();
    BufReader::new(deaf.take_stdout().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert_eq!(ready.trim(), "ready");
    let deaf = deaf.process().unwrap();
    let began = Instant::now();
    let summary = scope.finish();
    let took = began.elapsed();
    let how = |id| {
        summary
            .reports
            .iter()
            .find(|r| r.id == Some(id))
            .map(|r| r.how)
    };
    assert_eq!(how(polite), Some(HowEnded::Asked), "{summary:?}");
    assert_eq!(how(deaf), Some(HowEnded::Ended), "{summary:?}");
    assert!(
        took >= grace,
        "the deaf child was ended before its grace ({took:?})"
    );
    assert!(ends(polite) && ends(deaf));
}

#[cfg(unix)]
#[test]
fn a_grandchild_that_left_the_group_is_found_by_the_marker_ended_and_reported() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::new("escape", dir.path()).unwrap();
    let mut held = scope.start(piped("escape")).unwrap();
    let child = held.process().unwrap();
    let grandchild =
        ProcessId::of(first_pid(&mut held, "grandchild")).expect("the grandchild runs");
    let summary = scope.finish();
    if !ends(grandchild) {
        fail(
            &[grandchild],
            "the grandchild that left the group still runs",
        );
    }
    let how = |id| {
        summary
            .reports
            .iter()
            .find(|r| r.id == Some(id))
            .map(|r| r.how)
    };
    assert_eq!(how(grandchild), Some(HowEnded::Escaped), "{summary:?}");
    assert!(how(child).is_some(), "{summary:?}");
}

/// The owner is this binary on [`stand_in`] in mode "owner"; SIGKILL through its handle, so none of its own clean-up runs.
#[cfg(unix)]
#[test]
fn an_owner_ended_by_sigkill_leaves_no_process_and_no_folder() {
    owner_killed("owner");
}

/// The owner is killed as soon as its first start returned: its guardian must already serve it by then (a guardian that
/// had not yet checked its parent found pid 1 there and refused, so the child ran on: the stress run of 2026-11-24).
#[cfg(unix)]
#[test]
fn an_owner_ended_by_sigkill_right_after_its_first_start_leaves_nothing() {
    owner_killed("owner-quick");
}

#[cfg(unix)]
fn owner_killed(mode: &str) {
    let dir = tempfile::tempdir().unwrap();
    let mut owner = self_as(mode);
    let log = dir.path().join("guardian.log");
    owner
        .env("PADDOCK_SCOPE_TEST_UNDER", dir.path())
        .env(paddock_proc::guardian::LOG_ENV, &log)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let mut owner = owner.spawn().unwrap();
    let owner_id = ProcessId::of(owner.id()).unwrap();
    let mut out = BufReader::new(owner.stdout.take().unwrap());
    let (mut ids, mut folder) = (Vec::new(), None);
    let mut line = String::new();
    while line.trim() != "ready" {
        line.clear();
        if out.read_line(&mut line).unwrap() == 0 {
            fail(
                &[owner_id],
                "the owner ended before its children were ready",
            );
        }
        match line.split_once(' ') {
            Some(("child" | "grandchild", pid)) => {
                ids.push(ProcessId::of(pid.trim().parse().unwrap()).unwrap())
            }
            Some(("folder", f)) => folder = Some(PathBuf::from(f.trim())),
            _ => {}
        }
    }
    let folder = folder.expect("the owner named its folder");
    owner.kill().unwrap();
    owner.wait().unwrap();
    if paddock_proc::effective_uid() == 0 {
        // Root has no guardian (ADR 0076 amendment 2026-11-09): the killed owner's children stay. Asserted, then ended here.
        let stayed = ids.iter().all(|id| !has_ended(*id));
        for id in &ids {
            kill_tree(*id);
        }
        assert!(
            stayed,
            "a root owner's children were ended without a guardian"
        );
        return;
    }
    let gone = poll_until_within(HANG_GUARD, || {
        ids.iter().all(|id| has_ended(*id)) && !folder.exists()
    });
    if !gone {
        let left: Vec<u32> = ids
            .iter()
            .filter(|id| !has_ended(**id))
            .map(|id| id.pid())
            .collect();
        let why = format!(
            "the killed owner's scope left processes {left:?} (of {:?}) or its folder (there: {}); guardian log:\n{}",
            ids.iter().map(|id| id.pid()).collect::<Vec<_>>(),
            folder.exists(),
            std::fs::read_to_string(&log).unwrap_or_default()
        );
        fail(&ids, &why);
    }
}

/// The measurement behind [`paddock_proc::scope::GRACE`] (ER-12), not a check: run it with
/// `cargo nextest run -p paddock-proc --test it --run-ignored only -E 'test(measure_grace)' --no-capture`. For each kind of
/// child it starts one, waits for its "ready" line, asks it to stop ([`paddock_proc::ask`]) and times the exit seen through
/// the held handle. Prints the median, 99th percentile and worst per kind; the results block is in docs/benchmarks.md.
#[cfg(unix)]
#[test]
#[ignore = "a measurement for docs/benchmarks.md, not a check"]
fn measure_grace() {
    let runs: usize = std::env::var("PADDOCK_SCOPE_GRACE_RUNS")
        .ok()
        .and_then(|r| r.parse().ok())
        .unwrap_or(200);
    let sh = |script: &str| {
        let mut c = command("/bin/sh");
        c.args(["-c", script]);
        c
    };
    type Kind = (&'static str, Box<dyn Fn() -> Command>, usize);
    let mut kinds: Vec<Kind> = vec![
        (
            "rust program (default SIGTERM)",
            Box::new(|| self_as("ready")),
            runs,
        ),
        (
            "sh running sleep (default SIGTERM)",
            Box::new(move || sh("echo ready; sleep 60; true")),
            runs,
        ),
        (
            "sh script trapping SIGTERM, 1 s steps",
            Box::new(move || {
                sh(
                    "trap 'exit 0' TERM; echo ready; i=0; while [ $i -lt 60 ]; do sleep 1; i=$((i+1)); done",
                )
            }),
            runs / 4,
        ),
    ];
    let mut probe = command("node");
    probe.arg("--version");
    if paddock_proc::run_bounded(probe, Limits::new(HANG_GUARD)).is_ok_and(|o| o.success()) {
        kinds.push((
            "node program (default SIGTERM)",
            Box::new(|| {
                let mut c = command("node");
                c.args(["-e", "console.log('ready'); setTimeout(() => {}, 60000)"]);
                c
            }),
            runs,
        ));
    }
    // The first start of a process: the guardian is started and answers that it serves (its SERVING_BOUND).
    let dir = tempfile::tempdir().unwrap();
    let mut firsts: Vec<Duration> = (0..runs / 4)
        .map(|_| {
            let mut c = self_as("first-start");
            c.env("PADDOCK_SCOPE_TEST_UNDER", dir.path());
            let out = paddock_proc::run_bounded(c, Limits::new(HANG_GUARD)).unwrap();
            let us = out
                .stdout_text()
                .lines()
                .find_map(|l| l.strip_prefix("first-start ").map(str::to_owned))
                .unwrap();
            Duration::from_micros(us.trim().parse().unwrap())
        })
        .collect();
    firsts.sort_unstable();
    println!(
        "grace: first start with the guardian: n={} median={:.2?} p99={:.2?} worst={:.2?}",
        firsts.len(),
        firsts[firsts.len() / 2],
        firsts[((firsts.len() as f64 - 1.0) * 0.99).round() as usize],
        firsts[firsts.len() - 1]
    );
    for (name, make, n) in kinds {
        let mut took = Vec::with_capacity(n);
        for _ in 0..n {
            let mut cmd = make();
            cmd.stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            let mut child = cmd.spawn().unwrap();
            let id = ProcessId::of(child.id()).unwrap();
            // A test harness prints its own lines first: read up to "ready". The reader is kept so the pipe stays open.
            let mut out = BufReader::new(child.stdout.take().unwrap());
            let mut line = String::new();
            while line.trim() != "ready" {
                line.clear();
                assert!(
                    out.read_line(&mut line).unwrap() > 0,
                    "{name}: ended before it was ready"
                );
            }
            let asked = Instant::now();
            assert!(paddock_proc::ask(id), "{name}: the ask was not delivered");
            let exited = loop {
                if child.try_wait().unwrap().is_some() {
                    break asked.elapsed();
                }
                if asked.elapsed() > HANG_GUARD {
                    kill_tree(id);
                    panic!("{name}: no exit within {HANG_GUARD:?} of the ask");
                }
                std::thread::sleep(Duration::from_micros(200));
            };
            kill_tree(id);
            let _ = child.wait();
            took.push(exited);
        }
        took.sort_unstable();
        let at = |q: f64| took[((took.len() as f64 - 1.0) * q).round() as usize];
        println!(
            "grace: {name}: n={} median={:.2?} p99={:.2?} worst={:.2?}",
            took.len(),
            at(0.5),
            at(0.99),
            took[took.len() - 1]
        );
    }
}
