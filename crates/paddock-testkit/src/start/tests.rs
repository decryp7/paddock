//! The wait for a started program, tested once here (rule 43): a scripted child (this test binary, re-executed with a mode
//! in its environment) that becomes ready when the test says so, ends at once with an error, or holds until it is ended.

use super::*;
use std::cell::Cell;
use std::io::{BufRead, Read, Write};

const MODE: &str = "PADDOCK_TESTKIT_SCRIPTED";
/// What the scripted child prints before it fails on purpose.
const FAILING: &str = "scripted: failing on purpose";
/// More looks than any start budget ADR 0071 removes (Pitwall's 200 polls, `9c42a60ef`).
const OLD_BUDGET: u64 = 200;

/// The scripted child, when this binary runs as one: `exit:<code>` prints [`FAILING`] to stderr and exits with `<code>`;
/// `ready-on-line:<file>` creates `<file>` once a line arrives on stdin, then holds; `stop-self` stops itself (SIGSTOP), then
/// holds once resumed; `hold` holds. Holding is reading stdin
/// to its end, so the child ends with the test (its stdin closes) and never sleeps. Run by itself, the test does nothing.
#[test]
fn scripted_child() {
    let Ok(mode) = std::env::var(MODE) else {
        return;
    };
    let stdin = std::io::stdin();
    if let Some(code) = mode.strip_prefix("exit:") {
        eprintln!("{FAILING}");
        std::process::exit(code.parse().unwrap_or(1));
    }
    #[cfg(unix)]
    if mode == "stop-self" {
        // SAFETY: raise only signals this process itself; SIGSTOP stops it until the test ends it.
        unsafe { libc::raise(libc::SIGSTOP) };
    }
    if let Some(file) = mode.strip_prefix("ready-on-line:") {
        let mut line = String::new();
        drop(stdin.lock().read_line(&mut line));
        std::fs::write(file, "ready").unwrap();
    }
    let mut rest = Vec::new();
    drop(stdin.lock().read_to_end(&mut rest));
    std::process::exit(0);
}

fn scripted(mode: &str) -> Command {
    let mut c = paddock_proc::command(std::env::current_exe().unwrap());
    c.args([
        "--exact",
        "start::tests::scripted_child",
        "--nocapture",
        "--test-threads",
        "1",
    ])
    .env(MODE, mode)
    .stdin(Stdio::piped())
    .stdout(Stdio::null());
    c
}

fn spawn(dir: &tempfile::TempDir, mode: &str) -> Logged {
    Logged::spawn(&mut scripted(mode), &dir.path().join("stderr.txt")).unwrap()
}

/// A wall clock that moves only when the wait pauses: the guard is reached in microseconds of real time.
struct FakeClock(Cell<Duration>);

impl Clock for FakeClock {
    fn elapsed(&self) -> Duration {
        self.0.get()
    }
    fn pause(&self, d: Duration) {
        self.0.set(self.0.get() + d);
    }
}

fn fake() -> FakeClock {
    FakeClock(Cell::new(Duration::ZERO))
}

/// A program as the test scripts it: running for `exits_after` looks at its liveness, then ended (never, for `None`).
struct Scripted {
    exits_after: Option<u32>,
    looked: u32,
}

impl Started for Scripted {
    fn pid(&self) -> Option<ProcessId> {
        None
    }
    fn liveness(&mut self) -> Liveness {
        self.looked += 1;
        match self.exits_after {
            Some(n) if self.looked > n => Liveness::Exited(ExitInfo {
                status: Some(7),
                stderr_tail: Some("bye".into()),
            }),
            _ => Liveness::Running,
        }
    }
}

// (a) no count of looks decides: a child that becomes ready only after more looks than the old budgets is waited for.
#[test]
fn a_program_that_becomes_ready_after_more_looks_than_any_old_budget_is_waited_for() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("ready");
    let mut child = spawn(&dir, &format!("ready-on-line:{}", marker.display()));
    let mut stdin = child.child().stdin.take().unwrap();
    let mut looks = 0u64;
    let got = wait_ready_or_exit(&mut child, "the scripted child", || {
        looks += 1;
        if looks == OLD_BUDGET + 50 {
            stdin.write_all(b"go\n").unwrap();
            stdin.flush().unwrap();
        }
        marker.exists().then_some(looks)
    })
    .expect_ready("the scripted child");
    assert!(
        got > OLD_BUDGET,
        "ready after {got} looks, more than the old budget of {OLD_BUDGET}"
    );
}

// (b) an end before ready is reported at once, with the program's exit code and error output.
#[test]
fn a_program_that_ends_before_it_is_ready_is_reported_with_its_code_and_error_output() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = spawn(&dir, "exit:3");
    let outcome = wait_ready_or_exit(&mut child, "the failing child", || None::<()>);
    let StartOutcome::Exited(info) = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(info.status, Some(3));
    assert!(
        info.stderr_tail
            .as_deref()
            .is_some_and(|t| t.contains(FAILING)),
        "{info:?}"
    );
    // The same end through `wait_exit`, and through the panic of `expect_ready`, which names it.
    let mut again = spawn(&dir, "exit:4");
    assert_eq!(wait_exit(&mut again, "the failing child").status, Some(4));
    let mut third = spawn(&dir, "exit:5");
    let said = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wait_ready_or_exit(&mut third, "the failing child", || None::<()>)
            .expect_ready("the failing child")
    }))
    .unwrap_err();
    let said = said.downcast_ref::<String>().unwrap();
    assert!(
        said.contains("ended before it was ready")
            && said.contains("exit code 5")
            && said.contains(FAILING),
        "{said}"
    );
}

// (c) the exit is seen at the next look, not after a budget: the mutant "liveness always says running" waits for the guard.
#[test]
fn an_end_is_seen_at_the_next_look_and_a_wait_blind_to_it_runs_into_the_guard() {
    let clock = fake();
    let mut looks = 0u64;
    let mut ending = Scripted {
        exits_after: Some(3),
        looked: 0,
    };
    let outcome = wait_on(&mut ending, "an ending program", HANG_GUARD, &clock, || {
        looks += 1;
        None::<()>
    })
    .unwrap();
    assert_eq!(
        outcome,
        StartOutcome::Exited(ExitInfo {
            status: Some(7),
            stderr_tail: Some("bye".into())
        })
    );
    assert!(
        looks <= 4,
        "the end was seen within one look of it ({looks} looks for 3 running ones)"
    );
    // The mutant: a liveness that never says the program ended. Only the guard ends that wait, and it says hang.
    let mut blind = Scripted {
        exits_after: None,
        looked: 0,
    };
    let hang = wait_on(
        &mut blind,
        "a program nobody sees end",
        HANG_GUARD,
        &fake(),
        || None::<()>,
    )
    .unwrap_err();
    assert!(hang.looks > 100, "{hang:?}");
    assert_eq!(hang.waited, HANG_GUARD);
}

// The real-process twin of (c): a program that ended is reported as ended by its ProcessId too.
#[test]
fn a_process_known_only_by_identity_is_reported_once_it_is_gone() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = spawn(&dir, "hold");
    let id = child.pid().expect("the child's identity");
    let ready =
        wait_ready_or_exit(id, "the held child", || Some("there")).expect_ready("the held child");
    assert_eq!(ready, "there");
    drop(child.child().stdin.take());
    wait_exit(&mut child, "the held child");
    assert!(matches!(
        wait_ready_or_exit(id, "the ended child", || None::<()>),
        StartOutcome::Exited(_)
    ));
}

// (d) the warm-up runs a program and fails loudly on one that cannot run; several are warmed side by side.
#[test]
fn a_warm_up_runs_each_program_and_a_broken_one_fails_the_test() {
    let me = std::env::current_exe().unwrap();
    let args: &[&str] = &["--exact", "start::tests::scripted_child", "--quiet"];
    let first = warm_first_start(&me, args);
    assert_eq!(
        warm_first_start(&me, args),
        first,
        "a second warm-up of the same file is the recorded one"
    );
    assert_eq!(
        warm_first_starts(&[(me.as_path(), args), (me.as_path(), args)]),
        vec![first, first]
    );
    let missing = std::panic::catch_unwind(|| {
        warm_first_start(Path::new("/no/such/program"), &["--version"])
    })
    .unwrap_err();
    assert!(
        missing
            .downcast_ref::<String>()
            .unwrap()
            .contains("/no/such/program")
    );
}

// (e) a stopped program is not ended by the judge; the wall guard ends the wait and the message says hang and stopped.
#[cfg(unix)]
#[test]
fn a_stopped_program_is_waited_for_until_the_guard_and_then_called_a_hang() {
    let dir = tempfile::tempdir().unwrap();
    // The child stops itself (SIGSTOP): no signal is sent to a pid from here (rule 53). Its held handle ends it on drop.
    let mut child = spawn(&dir, "stop-self");
    let pid = child.child().id().to_string();
    let id = child.pid().expect("the child's identity");
    crate::wait_until("the child to stop itself", || {
        paddock_proc::run_state(id) == RunState::Stopped
    });
    let hang = wait_on(&mut child, "the stopped child", HANG_GUARD, &fake(), || {
        None::<()>
    })
    .unwrap_err();
    assert_eq!(
        hang.state, "stopped (paused by a signal or the OS)",
        "{hang:?}"
    );
    let said = hang.to_string();
    assert!(
        said.contains("never became ready or ended") && said.contains("a hang, not a slow start"),
        "{said}"
    );
    assert!(said.contains(&format!("pid {pid}")), "{said}");
}

#[test]
fn a_hang_panics_with_its_message_and_the_try_form_returns_it() {
    let hang = HangGuardExpired {
        what: "the thing".into(),
        pid: None,
        state: "alive",
        waited: HANG_GUARD,
        looks: 9,
    };
    let said = hang.to_string();
    assert!(
        said.starts_with("the thing (pid unknown) never became ready or ended"),
        "{said}"
    );
    // The try form answers at once for a program that is ready at the first look.
    let mut ready = Scripted {
        exits_after: None,
        looked: 0,
    };
    assert_eq!(
        try_wait_ready_or_exit(&mut ready, "a ready program", || Some(1)).unwrap(),
        StartOutcome::Ready(1)
    );
}
