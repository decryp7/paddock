//! The agent-launch step of Dyno's `check-bundle.sh` (`--launch-view <agent>`), run as the script runs it against a stand-in
//! `launchctl` (nothing real is started, ER-14). Nightly 9103: the Dyno bundle job failed there because the CI Mac is a
//! LaunchDaemon with no login session (ADR 0120, ER-60); without a session the step is NOT MEASURED, never a failure and never a pass.
// windows-twin: none: it runs a bash script that checks a macOS bundle, with Unix paths and PATH with `:`
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_testkit::{Actions, Case, StandIn, TestCommand};
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    paddock_env::winpath::real(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap()
}

/// How the stand-in `launchctl` answers `submit`.
enum Submit {
    /// Starts the job: the view's output file gets this line.
    Writes(&'static str),
    /// Refuses, saying this on standard error.
    Fails(&'static str),
}

struct Seen {
    code: i32,
    out: String,
    submits: usize,
}

/// One run of `check-bundle.sh --launch-view` with a stand-in `launchctl` (and `sw_vers`, 15.1) first on the PATH. `session`: whether
/// `launchctl print gui/<uid>` finds a user session.
fn run(session: bool, submit: Submit) -> Seen {
    let dir = tempfile::Builder::new().prefix("paddock-cb-").tempdir().unwrap();
    let root = paddock_env::winpath::real(dir.path()).unwrap();
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let print = Actions::new().exit(if session { 0 } else { 1 });
    let submit_actions = match submit {
        // `submit -l <label> -o <view> -e <err> -- <agent> view`: the fifth argument is the view's file.
        Submit::Writes(line) => Actions::new().writes_with("{arg:5}", &format!("{line}\n")),
        Submit::Fails(why) => Actions::new().stderr(why).exit(1),
    };
    let launchctl = StandIn::new(&bin, "launchctl")
        .records_args()
        .when(&Case::new().args_start_with(&["print"]), print)
        .when(&Case::new().args_start_with(&["submit"]), submit_actions)
        .install()
        .unwrap();
    StandIn::new(&bin, "sw_vers")
        .answers(Actions::new().stdout("15.1"))
        .install()
        .unwrap();

    let mut cmd = TestCommand::new("bash");
    cmd.arg(repo().join("apps/dyno/scripts/check-bundle.sh"))
        .args(["--launch-view", "/not/a/real/agent"]);
    cmd.path_first(&bin).env("TMPDIR", &root);
    let out = cmd.output().unwrap();
    let submits = launchctl
        .calls()
        .unwrap()
        .iter()
        .filter(|c| c.first().is_some_and(|a| a == "submit"))
        .count();
    Seen {
        code: out.status.code().unwrap_or(-1),
        out: format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)),
        submits,
    }
}

const GOOD: &str = "identifier=net.decryptology.dyno.agent announced=3 sent=3 cut_down=false";

#[test]
fn without_a_login_session_the_step_says_not_measured_starts_nothing_and_is_neither_pass_nor_failure() {
    let r = run(false, Submit::Writes(GOOD));
    assert_eq!(r.code, 0, "{}", r.out);
    assert!(
        r.out.contains("::warning::") && r.out.contains("NOT MEASURED") && r.out.contains("no login session") && r.out.contains("ER-60"),
        "{}",
        r.out
    );
    assert_eq!(r.submits, 0, "launchctl submit must not be called");
    assert!(
        !r.out.contains("bundled agent socket view"),
        "a skipped step never prints a measurement: {}",
        r.out
    );
}

#[test]
fn with_a_login_session_the_agent_view_is_measured_and_its_line_printed() {
    let r = run(true, Submit::Writes(GOOD));
    assert_eq!(r.code, 0, "{}", r.out);
    assert_eq!(r.submits, 1);
    assert!(
        r.out
            .contains("bundled agent socket view (as a launchd job): identifier=net.decryptology.dyno.agent"),
        "{}",
        r.out
    );
    assert!(!r.out.contains("NOT MEASURED"), "{}", r.out);
}

#[test]
fn a_failing_launchctl_submit_fails_the_check_and_prints_what_launchctl_said() {
    let r = run(true, Submit::Fails("Bad request: Could not find service"));
    assert_ne!(r.code, 0, "{}", r.out);
    assert!(
        r.out
            .contains("::error::launchctl could not start the bundled dyno-agent view: Bad request: Could not find service"),
        "{}",
        r.out
    );
}

#[test]
fn a_view_that_is_not_dynos_own_identifier_fails_the_check() {
    let r = run(
        true,
        Submit::Writes("identifier=com.example.other announced=1 sent=1 cut_down=false"),
    );
    assert_ne!(r.code, 0, "{}", r.out);
    assert!(r.out.contains("code-signing identifier is not net.decryptology.dyno."), "{}", r.out);
}
